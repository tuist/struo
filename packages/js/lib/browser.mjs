import initialize, * as wasm from "./wasm/struo_cache_core.js";
import { openDatabase, transaction, measure } from "./indexeddb.mjs";

export class StruoError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = "StruoError";
  }
}

let ready = false;
let initialization;

/** Initialize once before using synchronous digests or creating a cache. */
export async function init(options) {
  initialization ??= initialize(options).catch((error) => {
    initialization = undefined;
    throw error;
  });
  await initialization;
  ready = true;
}

function requireReady() {
  if (!ready) throw new StruoError("Call and await init() before using Struo");
}

function bytes(value) {
  if (typeof value === "string") return new TextEncoder().encode(value);
  if (value instanceof Uint8Array) return value;
  throw new TypeError("bytes must be a Uint8Array or string");
}

function validateDigest(value) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    throw new StruoError("expected a lowercase BLAKE3 hex digest");
  }
}

function validateString(value) {
  if (typeof value !== "string") throw new TypeError("expected a string");
}

export function digest(value) {
  requireReady();
  return wasm.digest(bytes(value));
}

export class ActionKey {
  constructor(namespace) {
    requireReady();
    validateString(namespace);
    this.key = new wasm.ActionKey(namespace);
  }

  addBytes(label, value) {
    validateString(label);
    this.key.addBytes(label, bytes(value));
    return this;
  }

  addDigest(label, value) {
    validateString(label);
    validateDigest(value);
    this.key.addDigest(label, value);
    return this;
  }

  digest() {
    return this.key.digest();
  }

  free() {
    this.key.free();
  }
}

export class Cache {
  constructor(options = {}) {
    requireReady();
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      throw new TypeError("Cache options must be an object");
    }
    if (Object.keys(options).some((key) => key !== "databaseName")) {
      throw new TypeError("Browser caches accept only databaseName, not filesystem or workspace options");
    }
    const { databaseName = "struo" } = options;
    if (typeof databaseName !== "string" || databaseName.length === 0) {
      throw new TypeError("databaseName must be a non-empty string");
    }
    this.databaseName = databaseName;
    this.database = null;
    this.closed = false;
  }

  version() {
    return wasm.version();
  }

  digest(value) {
    return digest(value);
  }

  async run(stores, mode, work) {
    if (this.closed) throw new StruoError("Cache is closed");
    try {
      this.database ??= openDatabase(this.databaseName).catch((error) => {
        this.database = null;
        throw error;
      });
      return await transaction(await this.database, stores, mode, work);
    } catch (cause) {
      throw new StruoError(`IndexedDB cache operation failed: ${cause.message}`, { cause });
    }
  }

  async putBlob(value) {
    const payload = bytes(value).slice();
    const key = digest(payload);
    await this.run(["blobs"], "readwrite", (tx) => {
      tx.objectStore("blobs").put({ bytes: payload, size: payload.byteLength }, key);
    });
    return key;
  }

  async getBlob(key) {
    validateDigest(key);
    const record = await this.run(["blobs"], "readonly", (tx, done) => {
      tx.objectStore("blobs").get(key).onsuccess = (event) => done(event.target.result);
    });
    if (!record) throw new StruoError(`Blob not found: ${key}`);
    if (digest(record.bytes) !== key) throw new StruoError(`Blob integrity check failed: ${key}`);
    return record.bytes;
  }

  async hasBlob(key) {
    validateDigest(key);
    return this.run(["blobs"], "readonly", (tx, done) => {
      tx.objectStore("blobs").count(key).onsuccess = (event) => done(event.target.result > 0);
    });
  }

  async putActionResult(result, key) {
    validateDigest(key);
    if (!result || !Number.isInteger(result.exitCode) || result.exitCode < -2147483648 || result.exitCode > 2147483647) {
      throw new TypeError("exitCode must be a signed 32-bit integer");
    }
    const { stdout = null, stderr = null, outputs = {} } = result;
    if (stdout !== null) validateDigest(stdout);
    if (stderr !== null) validateDigest(stderr);
    if (!outputs || typeof outputs !== "object" || Array.isArray(outputs)) {
      throw new TypeError("outputs must be a record of blob digests");
    }
    for (const value of Object.values(outputs)) validateDigest(value);
    const normalized = { exitCode: result.exitCode, stdout, stderr, outputs: { ...outputs } };
    const size = new TextEncoder().encode(JSON.stringify(normalized)).byteLength;
    await this.run(["actions"], "readwrite", (tx) => {
      tx.objectStore("actions").put({ result: normalized, size }, key);
    });
    return true;
  }

  async getActionResult(key) {
    validateDigest(key);
    return this.run(["actions"], "readonly", (tx, done) => {
      tx.objectStore("actions").get(key).onsuccess = (event) => {
        done(event.target.result?.result ?? null);
      };
    });
  }

  async forgetAction(key) {
    validateDigest(key);
    return this.run(["actions"], "readwrite", (tx, done) => {
      const store = tx.objectStore("actions");
      store.count(key).onsuccess = (event) => {
        done(event.target.result > 0);
        store.delete(key);
      };
    });
  }

  async stats() {
    return this.run(["blobs", "actions"], "readonly", (tx, done) => {
      const stats = { blobCount: 0, blobBytes: 0, actionCount: 0, actionBytes: 0 };
      measure(tx.objectStore("blobs"), (count, size) => {
        stats.blobCount = count;
        stats.blobBytes = size;
      });
      measure(tx.objectStore("actions"), (count, size) => {
        stats.actionCount = count;
        stats.actionBytes = size;
      });
      done(stats);
    });
  }

  async clear() {
    await this.run(["blobs", "actions"], "readwrite", (tx) => {
      tx.objectStore("blobs").clear();
      tx.objectStore("actions").clear();
    });
  }

  async close() {
    this.closed = true;
    if (this.database) (await this.database).close();
  }
}
