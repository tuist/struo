import { TAB_PROTOCOL, send, validTask } from "./tab-protocol.mjs";

export function serveTab({ handlers, memoryLimitBytes, channelName = "struo-execution" }) {
  if (!Number.isSafeInteger(memoryLimitBytes) || memoryLimitBytes <= 0) {
    throw new TypeError("memoryLimitBytes must be a positive integer");
  }
  const operations = new Map(Object.entries(handlers));
  if ([...operations.values()].some((handler) => typeof handler !== "function")) {
    throw new TypeError("handlers must contain functions");
  }
  const workerId = crypto.randomUUID();
  const channel = new BroadcastChannel(channelName);
  const jobs = new Map();
  let reserved = 0;
  let closed = false;
  const release = (id) => {
    const job = jobs.get(id);
    if (!job) return;
    clearTimeout(job.timer);
    reserved -= job.task.memoryBytes;
    jobs.delete(id);
    if (!closed) send(channel, { type: "available" });
  };
  channel.onmessage = ({ data }) => {
    if (data?.protocol !== TAB_PROTOCOL || typeof data.id !== "string") return;
    const { id, task } = data;
    if (data.type === "request") {
      if (jobs.has(id) || !validTask(task) || !operations.has(task.operation) || task.memoryBytes > memoryLimitBytes - reserved) return;
      const job = { task, controller: new AbortController(), running: false };
      reserved += task.memoryBytes;
      jobs.set(id, job);
      job.timer = setTimeout(() => release(id), 2_000);
      send(channel, { type: "offer", id, worker: workerId });
    } else if (data.type === "claim") {
      const job = jobs.get(id);
      if (data.worker !== workerId) {
        if (job && !job.running) release(id);
        return;
      }
      if (!job) {
        send(channel, { type: "error", id, worker: workerId, message: "Worker reservation expired; dispatch a new job" });
        return;
      }
      if (job.running) return;
      job.running = true;
      clearTimeout(job.timer);
      Promise.resolve().then(() => operations.get(job.task.operation)(job.task, {
        signal: job.controller.signal, workerId,
      })).then((result) => {
        if (!closed && !job.controller.signal.aborted) send(channel, { type: "result", id, worker: workerId, result });
      }).catch(() => {
        if (!closed && !job.controller.signal.aborted) {
          send(channel, { type: "error", id, worker: workerId, message: "Worker operation failed" });
        }
      }).finally(() => release(id));
    } else if (data.type === "cancel") {
      const job = jobs.get(id);
      if (!job) return;
      job.controller.abort();
      if (!job.running) release(id);
    }
  };
  return {
    workerId,
    close() {
      if (closed) return;
      closed = true;
      for (const [id, job] of jobs) {
        job.controller.abort();
        if (!job.running) release(id);
      }
      channel.close();
    },
  };
}
