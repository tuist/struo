import { StruoError } from "./browser.mjs";
import { secureURL, readJSON, readBounded } from "./browser-http.mjs";

export class GitLabExecutor {
  #base;
  #auth;

  constructor({ url = "https://gitlab.com", project, auth }) {
    const base = secureURL(url);
    if (base.search || !project || !auth?.getAccessToken) throw new TypeError("Provide a GitLab project and OAuth2 client");
    this.#base = `${base.href.replace(/\/$/, "")}/api/v4/projects/${encodeURIComponent(project)}`;
    this.#auth = auth;
  }

  async #request(path, { signal, redirect = "error", ...options } = {}) {
    signal ??= AbortSignal.timeout(30_000);
    signal.throwIfAborted();
    const token = await new Promise((resolve, reject) => {
      const finish = (settle, value) => { signal.removeEventListener("abort", abort); settle(value); };
      const abort = () => finish(reject, signal.reason);
      signal.addEventListener("abort", abort, { once: true });
      Promise.resolve().then(() => this.#auth.getAccessToken()).then(
        (value) => finish(resolve, value), (error) => finish(reject, error),
      );
    });
    return fetch(`${this.#base}${path}`, {
      ...options, credentials: "omit", redirect, signal,
      headers: { ...options.headers, Authorization: `Bearer ${token}` },
    });
  }

  async dispatch({ ref, variables = {} }, { signal } = {}) {
    if (typeof ref !== "string" || !ref || !variables || typeof variables !== "object" || Array.isArray(variables)) {
      throw new TypeError("Provide a ref and a record of string variables");
    }
    const entries = Object.entries(variables).map(([key, value]) => {
      if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(key) || typeof value !== "string") throw new TypeError("Invalid pipeline variable");
      return { key, value, variable_type: "env_var" };
    });
    const body = JSON.stringify({ ref, variables: entries });
    if (new TextEncoder().encode(body).length > 64 * 1024) throw new StruoError("Pipeline request exceeds 64 KiB");
    try {
      return await readJSON(await this.#request("/pipeline", {
        method: "POST", headers: { "Content-Type": "application/json" }, body, signal,
      }));
    } catch (cause) {
      throw new StruoError("Pipeline submission failed; creation may have succeeded. Check project pipelines before retrying", { cause });
    }
  }

  async get(pipelineId, { signal } = {}) {
    return readJSON(await this.#request(`/pipelines/${this.#id(pipelineId)}`, { signal }));
  }

  async cancel(pipelineId) {
    return readJSON(await this.#request(`/pipelines/${this.#id(pipelineId)}/cancel`, { method: "POST" }));
  }

  async execute(request, { signal, timeoutMs = 300_000, pollIntervalMs = 2_000 } = {}) {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isFinite(pollIntervalMs) || pollIntervalMs <= 0) {
      throw new TypeError("Timeout and poll interval must be positive");
    }
    signal?.throwIfAborted();
    const deadline = AbortSignal.timeout(timeoutMs);
    const combined = signal ? AbortSignal.any([signal, deadline]) : deadline;
    const pipeline = await this.dispatch(request, { signal: combined });
    try {
      while (true) {
        const result = await this.get(pipeline.id, { signal: combined });
        if (["success", "failed", "canceled", "skipped"].includes(result.status)) return result;
        await new Promise((resolve, reject) => {
          const abort = () => { clearTimeout(timer); reject(combined.reason); };
          const timer = setTimeout(() => { combined.removeEventListener("abort", abort); resolve(); }, pollIntervalMs);
          combined.addEventListener("abort", abort, { once: true });
          if (combined.aborted) abort();
        });
      }
    } catch (error) {
      try { await this.cancel(pipeline.id); }
      catch { throw new StruoError(`Execution interrupted and pipeline ${pipeline.id} could not be cancelled; check GitLab`); }
      throw error;
    }
  }

  async jobs(pipelineId, { signal } = {}) {
    return readJSON(await this.#request(`/pipelines/${this.#id(pipelineId)}/jobs?per_page=100`, { signal }));
  }

  async storeArtifact({ jobId, path, cache, maxBytes, signal }) {
    if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) throw new TypeError("maxBytes must be positive");
    if (typeof path !== "string" || !path || path.split("/").some((part) => !part || part === "." || part === "..")) {
      throw new TypeError("Use a relative artifact path without dot segments");
    }
    const encoded = path.split("/").map(encodeURIComponent).join("/");
    const response = await this.#request(`/jobs/${this.#id(jobId)}/artifacts/${encoded}`, { signal, redirect: "follow" });
    if (!response.ok) { await response.body?.cancel(); throw new StruoError(`Artifact request failed (HTTP ${response.status})`); }
    return cache.putBlob(await readBounded(response, maxBytes));
  }

  #id(value) {
    if (!Number.isSafeInteger(value) || value <= 0) throw new TypeError("ID must be a positive integer");
    return value;
  }
}
