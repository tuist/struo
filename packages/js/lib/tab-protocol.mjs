import { StruoError } from "./browser.mjs";

export const TAB_PROTOCOL = "struo.tab-execution.v1";

export function send(channel, message) {
  if (new TextEncoder().encode(JSON.stringify(message)).length > 64 * 1024) {
    throw new StruoError("Execution message exceeds 64 KiB; pass cache digests instead of payloads");
  }
  channel.postMessage({ ...message, protocol: TAB_PROTOCOL });
}

export function validTask(task) {
  return task && typeof task.operation === "string" && task.operation.length > 0 &&
    Number.isSafeInteger(task.memoryBytes) && task.memoryBytes > 0 &&
    task.inputs && typeof task.inputs === "object" && !Array.isArray(task.inputs) &&
    Object.values(task.inputs).every((value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value)) &&
    Array.isArray(task.args) && task.args.every((value) => typeof value === "string");
}
