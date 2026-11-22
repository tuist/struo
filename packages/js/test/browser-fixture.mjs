import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";

export async function browserFixture() {
  const root = process.env.SDK_PACKAGE_ROOT
    ? pathToFileURL(`${process.env.SDK_PACKAGE_ROOT}/`)
    : new URL("../", import.meta.url);
  const server = createServer(async (request, response) => {
    const pathname = new URL(request.url, "http://localhost").pathname;
    if (pathname === "/" || pathname === "/callback") {
      response.setHeader("Content-Type", "text/html");
      return response.end("<!doctype html><title>Browser execution tests</title>");
    }
    if (!/^\/lib\/[a-zA-Z0-9_./-]+$/.test(pathname) || pathname.includes("..")) {
      response.writeHead(404).end();
      return;
    }
    try {
      const bytes = await readFile(new URL(`.${pathname}`, root));
      response.setHeader("Content-Type", pathname.endsWith(".wasm") ? "application/wasm" : "text/javascript");
      response.end(bytes);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ["--no-sandbox"] });
    return {
      browser, origin: `http://127.0.0.1:${server.address().port}`,
      async close() { await browser.close(); await new Promise((resolve) => server.close(resolve)); },
    };
  } catch (error) { server.close(); throw error; }
}
