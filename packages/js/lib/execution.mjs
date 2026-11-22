import { StruoError } from "./browser.mjs";
import { TAB_PROTOCOL, send, validTask } from "./tab-protocol.mjs";
export { serveTab } from "./tab-worker.mjs";
export { OAuth2Client } from "./oauth2.mjs";
export { GitLabExecutor } from "./gitlab.mjs";

export class TabExecutor {
  #channel;
  #pending = new Map();
  #closed = false;

  constructor({ channelName = "struo-execution" } = {}) {
    this.#channel = new BroadcastChannel(channelName);
    this.#channel.onmessage = ({ data }) => {
      if (data?.protocol !== TAB_PROTOCOL) return;
      if (data.type === "available") {
        for (const [id, pending] of this.#pending) {
          if (!pending.worker) send(this.#channel, { type: "request", id, task: pending.task });
        }
        return;
      }
      const pending = this.#pending.get(data.id);
      if (!pending) return;
      if (data.type === "offer" && !pending.worker) {
        pending.worker = data.worker;
        send(this.#channel, { type: "claim", id: data.id, worker: data.worker });
      } else if (data.worker === pending.worker && data.type === "result") {
        pending.resolve({ result: data.result, workerId: data.worker });
      } else if (data.worker === pending.worker && data.type === "error") {
        pending.reject(new StruoError(data.message));
      }
    };
  }

  async execute(task, { signal, timeoutMs = 30_000 } = {}) {
    if (this.#closed) throw new StruoError("Executor is closed");
    if (!validTask(task)) throw new TypeError("Provide an operation, digest inputs, string args, and positive memoryBytes");
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
      throw new TypeError("timeoutMs must be positive");
    }
    signal?.throwIfAborted();
    task = structuredClone(task);
    const id = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const cancel = () => {
        send(this.#channel, { type: "cancel", id });
        finish(reject, signal?.aborted ? signal.reason : new StruoError("Execution timed out; completion is unknown"));
      };
      const timer = setTimeout(cancel, timeoutMs);
      const finish = (settle, value) => {
        clearTimeout(timer);
        signal?.removeEventListener("abort", cancel);
        this.#pending.delete(id);
        settle(value);
      };
      this.#pending.set(id, {
        task, worker: null,
        resolve: (value) => finish(resolve, value),
        reject: (reason) => finish(reject, reason),
      });
      signal?.addEventListener("abort", cancel, { once: true });
      try {
        send(this.#channel, { type: "request", id, task });
      } catch (error) { finish(reject, error); }
    });
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    for (const [id, pending] of this.#pending) {
      send(this.#channel, { type: "cancel", id });
      pending.reject(new StruoError("Executor closed; completion is unknown"));
    }
    this.#channel.close();
  }
}
