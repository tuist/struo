import { StruoError } from "./browser.mjs";

export function secureURL(value) {
  const url = new URL(value);
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if ((url.protocol !== "https:" && !(url.protocol === "http:" && loopback)) || url.username || url.password || url.hash) {
    throw new TypeError("Use an HTTPS URL without credentials or a fragment (HTTP is allowed on loopback)");
  }
  return url;
}

export async function readBounded(response, maxBytes) {
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) throw new TypeError("maxBytes must be positive");
  const reader = response.body?.getReader();
  if (!reader) return new Uint8Array();
  const chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > maxBytes) throw new StruoError("Provider response exceeds its byte limit");
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return bytes;
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

export async function readJSON(response) {
  if (!response.ok) {
    await response.body?.cancel();
    throw new StruoError(`Provider request failed (HTTP ${response.status})`);
  }
  return JSON.parse(new TextDecoder().decode(await readBounded(response, 1024 * 1024)));
}
