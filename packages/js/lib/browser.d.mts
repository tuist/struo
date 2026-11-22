export type { ActionResult, CacheStats } from "./types.js";
import type { ActionResult, CacheStats } from "./types.js";

export type WasmInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

/** Await before creating caches or computing synchronous digests. */
export function init(options?: { module_or_path: WasmInput | Promise<WasmInput> }): Promise<void>;
export class StruoError extends Error {}
export function digest(bytes: Uint8Array | string): string;

export class ActionKey {
  constructor(namespace: string);
  addBytes(label: string, bytes: Uint8Array | string): this;
  addDigest(label: string, digest: string): this;
  digest(): string;
  /** Release the Wasm hash state when finished with this builder. */
  free(): void;
}

export interface CacheOptions {
  databaseName?: string;
}

export class Cache {
  constructor(options?: CacheOptions);
  version(): string;
  digest(bytes: Uint8Array | string): string;
  putBlob(bytes: Uint8Array | string): Promise<string>;
  getBlob(digest: string): Promise<Uint8Array>;
  hasBlob(digest: string): Promise<boolean>;
  putActionResult(result: ActionResult, actionDigest: string): Promise<boolean>;
  getActionResult(actionDigest: string): Promise<ActionResult | null>;
  forgetAction(actionDigest: string): Promise<boolean>;
  stats(): Promise<CacheStats>;
  /** Clear blobs and action results in this database atomically. */
  clear(): Promise<void>;
  /** Close this instance without deleting its persisted data. */
  close(): Promise<void>;
}
