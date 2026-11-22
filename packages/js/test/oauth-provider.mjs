import { createServer } from "node:http";
import { createHash, randomUUID } from "node:crypto";

// This is a protocol fixture, not a live GitLab instance or identity provider.
export async function oauthProvider(appOrigin) {
  const codes = new Map();
  const tokens = new Set();
  const refreshes = new Map();
  const pipelines = new Map();
  const events = [];
  let nextId = 1;
  let refreshGate;
  let refreshStarted;
  const server = createServer(async (request, response) => {
    response.setHeader("Access-Control-Allow-Origin", appOrigin);
    response.setHeader("Access-Control-Allow-Headers", "authorization,content-type");
    response.setHeader("Access-Control-Allow-Methods", "GET,POST,OPTIONS");
    if (request.method === "OPTIONS") return response.writeHead(204).end();
    const url = new URL(request.url, "http://localhost");
    const json = (status, value) => {
      response.writeHead(status, { "Content-Type": "application/json" }).end(JSON.stringify(value));
    };
    try {
      if (url.pathname === "/oauth/authorize") {
        const params = Object.fromEntries(url.searchParams);
        if (params.code_challenge_method !== "S256" || params.client_id !== "test-client" ||
            params.redirect_uri !== `${appOrigin}/callback`) return json(400, {});
        const code = randomUUID();
        codes.set(code, params);
        const callback = new URL(params.redirect_uri);
        callback.searchParams.set("code", code);
        callback.searchParams.set("state", params.state);
        events.push("authorize");
        return response.writeHead(302, { Location: callback.href }).end();
      }
      let body = "";
      for await (const chunk of request) {
        body += chunk;
        if (body.length > 128 * 1024) return json(413, {});
      }
      if (url.pathname === "/oauth/token") {
        const params = new URLSearchParams(body);
        if (params.has("client_secret") || params.get("client_id") !== "test-client") return json(400, {});
        if (params.get("grant_type") === "authorization_code") {
          const code = codes.get(params.get("code"));
          codes.delete(params.get("code"));
          const challenge = createHash("sha256").update(params.get("code_verifier") ?? "").digest("base64url");
          if (!code || code.code_challenge !== challenge || code.redirect_uri !== params.get("redirect_uri")) return json(400, {});
          events.push("exchange:S256-verified");
        } else if (params.get("grant_type") === "refresh_token") {
          const old = refreshes.get(params.get("refresh_token"));
          if (!old) return json(400, {});
          refreshes.delete(params.get("refresh_token"));
          tokens.delete(old);
          if (refreshGate) { refreshStarted(); await refreshGate; }
          if (url.searchParams.has("delay")) await new Promise((resolve) => setTimeout(resolve, 100));
          events.push("refresh");
        } else return json(400, {});
        const access = randomUUID();
        const refresh = randomUUID();
        tokens.add(access);
        refreshes.set(refresh, access);
        return json(200, {
          access_token: access, refresh_token: refresh, token_type: "Bearer",
          expires_in: url.searchParams.has("short") && params.get("grant_type") === "authorization_code" ? 1 : 3600,
        });
      }
      if (url.pathname === "/oauth/revoke") {
        const token = new URLSearchParams(body).get("token");
        tokens.delete(refreshes.get(token) ?? token);
        refreshes.delete(token);
        events.push("revoke");
        return response.writeHead(200).end();
      }
      if (!tokens.has(request.headers.authorization?.replace(/^Bearer /, ""))) return json(401, {});
      if (url.pathname === "/api/v4/projects/example%2Fcompiler/pipeline" && request.method === "POST") {
        const params = JSON.parse(body);
        const id = nextId++;
        const pipeline = { id, status: "running", sha: "a".repeat(40), ref: params.ref, web_url: `${appOrigin}/pipeline/${id}` };
        if (params.ref !== "pending") {
          const value = params.variables.find((variable) => variable.key === "WASM_BASE64")?.value;
          try {
            const module = await WebAssembly.compile(Buffer.from(value, "base64"));
            const instance = await WebAssembly.instantiate(module);
            pipeline.artifact = `${instance.exports.answer()}\n`;
            pipeline.status = "success";
            events.push("compile:answer=42");
          } catch { pipeline.status = "failed"; }
        }
        pipelines.set(id, pipeline);
        events.push(`dispatch:${id}`);
        return json(201, pipeline);
      }
      const pipelineMatch = url.pathname.match(/^\/api\/v4\/projects\/example%2Fcompiler\/pipelines\/(\d+)(\/cancel|\/jobs)?$/);
      if (pipelineMatch) {
        const pipeline = pipelines.get(Number(pipelineMatch[1]));
        if (!pipeline) return json(404, {});
        if (pipelineMatch[2] === "/cancel") { pipeline.status = "canceled"; events.push(`cancel:${pipeline.id}`); }
        if (pipelineMatch[2] === "/jobs") return json(200, [{ id: pipeline.id, name: "compile", status: pipeline.status }]);
        return json(200, pipeline);
      }
      const artifact = url.pathname.match(/^\/api\/v4\/projects\/example%2Fcompiler\/jobs\/(\d+)\/artifacts\/answer.txt$/);
      if (artifact) {
        const pipeline = pipelines.get(Number(artifact[1]));
        if (!pipeline?.artifact) return json(404, {});
        return response.writeHead(200, { "Content-Type": "text/plain" }).end(pipeline.artifact);
      }
      return json(404, {});
    } catch { if (!response.headersSent) json(500, {}); else response.end(); }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}`, events,
    pauseRefresh() {
      let resume;
      refreshGate = new Promise((resolve) => { resume = resolve; });
      const started = new Promise((resolve) => { refreshStarted = resolve; });
      return Object.assign(() => { refreshGate = undefined; resume(); }, { started });
    },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
