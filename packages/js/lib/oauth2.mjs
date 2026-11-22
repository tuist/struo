import { StruoError } from "./browser.mjs";
import { secureURL, readJSON } from "./browser-http.mjs";

function random() {
  return base64url(crypto.getRandomValues(new Uint8Array(32)));
}

function base64url(bytes) {
  return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

export class OAuth2Client {
  #config;
  #key;
  #tokens;
  #refresh;
  #exchanges = new Set();
  #generation = 0;

  constructor({ authorizationEndpoint, tokenEndpoint, revocationEndpoint, clientId, redirectUri, scopes = [] }) {
    const redirect = secureURL(redirectUri);
    if (redirect.origin !== location.origin || redirect.search) throw new TypeError("redirectUri must be a same-origin URL without a query");
    if (typeof clientId !== "string" || !clientId || !Array.isArray(scopes) || scopes.some((scope) => typeof scope !== "string" || /\s/.test(scope))) {
      throw new TypeError("Provide a clientId and an array of scope names");
    }
    this.#config = {
      authorizationEndpoint: secureURL(authorizationEndpoint).href,
      tokenEndpoint: secureURL(tokenEndpoint).href,
      revocationEndpoint: revocationEndpoint ? secureURL(revocationEndpoint).href : null,
      clientId, redirectUri: redirect.href, scopes: [...scopes],
    };
    this.#key = `struo.oauth2:${JSON.stringify([this.#config.authorizationEndpoint, clientId, redirect.href])}`;
  }

  async authorizationURL() {
    this.#generation++;
    this.#tokens = undefined;
    this.#refresh = undefined;
    const verifier = random();
    const state = random();
    const challenge = base64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
    sessionStorage.setItem(this.#key, JSON.stringify({ verifier, state, createdAt: Date.now() }));
    const url = new URL(this.#config.authorizationEndpoint);
    for (const [name, value] of Object.entries({
      response_type: "code", client_id: this.#config.clientId, redirect_uri: this.#config.redirectUri,
      scope: this.#config.scopes.join(" "), state, code_challenge: challenge, code_challenge_method: "S256",
    })) url.searchParams.set(name, value);
    return url.href;
  }

  async completeAuthorization(callbackUrl = location.href) {
    const callback = new URL(callbackUrl);
    const redirect = new URL(this.#config.redirectUri);
    const pending = JSON.parse(sessionStorage.getItem(this.#key) ?? "null");
    if (callback.origin !== redirect.origin || callback.pathname !== redirect.pathname ||
        callback.searchParams.getAll("state").length !== 1 || !pending ||
        callback.searchParams.get("state") !== pending.state || Date.now() - pending.createdAt > 600_000) {
      throw new StruoError("OAuth callback state, redirect, or expiry check failed");
    }
    sessionStorage.removeItem(this.#key);
    if (callback.href === location.href) history.replaceState(history.state, "", redirect.href);
    if (callback.searchParams.has("error")) throw new StruoError("OAuth authorization was denied");
    const codes = callback.searchParams.getAll("code");
    if (codes.length !== 1 || !codes[0]) throw new StruoError("OAuth callback must contain one authorization code");
    await this.#exchange({ grant_type: "authorization_code", code: codes[0], code_verifier: pending.verifier });
  }

  #exchange(parameters) {
    const generation = this.#generation;
    let cleanup;
    const exchange = this.#requestTokens(parameters).then(async (tokens) => {
      if (generation !== this.#generation) {
        cleanup = this.#revoke(tokens);
        await cleanup;
        throw new StruoError("OAuth session changed during token exchange");
      }
      this.#tokens = tokens;
    });
    // Logout waits for revocation, not the expected rejection of a superseded exchange.
    const completion = exchange.then(() => {}, () => cleanup);
    this.#exchanges.add(completion);
    const forget = () => this.#exchanges.delete(completion);
    completion.then(forget, forget);
    return exchange;
  }

  async #requestTokens(parameters) {
    const response = await fetch(this.#config.tokenEndpoint, {
      method: "POST", credentials: "omit", redirect: "error",
      body: new URLSearchParams({ ...parameters, client_id: this.#config.clientId, redirect_uri: this.#config.redirectUri }),
      signal: AbortSignal.timeout(30_000),
    });
    const tokens = await readJSON(response);
    if (typeof tokens.access_token !== "string" || !tokens.access_token ||
        typeof tokens.token_type !== "string" || tokens.token_type.toLowerCase() !== "bearer" ||
        !Number.isFinite(tokens.expires_in) || tokens.expires_in <= 0) {
      throw new StruoError("OAuth server returned an invalid bearer token response");
    }
    return { ...tokens, expiresAt: Date.now() + tokens.expires_in * 1000 };
  }

  async getAccessToken() {
    if (!this.#tokens) throw new StruoError("OAuth login is required");
    if (Date.now() >= this.#tokens.expiresAt - 30_000) {
      if (!this.#tokens.refresh_token) { this.#tokens = undefined; throw new StruoError("OAuth login has expired"); }
      const generation = this.#generation;
      this.#refresh ??= this.#exchange({ grant_type: "refresh_token", refresh_token: this.#tokens.refresh_token })
        .catch((error) => { if (generation === this.#generation) this.#tokens = undefined; throw error; })
        .finally(() => { if (generation === this.#generation) this.#refresh = undefined; });
      await this.#refresh;
    }
    if (!this.#tokens) throw new StruoError("OAuth login is required");
    return this.#tokens.access_token;
  }

  async logout() {
    this.#generation++;
    this.#refresh = undefined;
    const tokens = this.#tokens;
    this.#tokens = undefined;
    sessionStorage.removeItem(this.#key);
    const results = await Promise.allSettled([this.#revoke(tokens), ...this.#exchanges]);
    const failure = results.find((result) => result.status === "rejected");
    if (failure) throw failure.reason;
  }

  async #revoke(tokens) {
    if (!tokens || !this.#config.revocationEndpoint) return;
    const response = await fetch(this.#config.revocationEndpoint, {
      method: "POST", credentials: "omit", redirect: "error", signal: AbortSignal.timeout(30_000),
      body: new URLSearchParams({ client_id: this.#config.clientId, token: tokens.refresh_token ?? tokens.access_token }),
    });
    await response.body?.cancel();
    if (!response.ok) throw new StruoError(`OAuth revocation failed (HTTP ${response.status})`);
  }
}
