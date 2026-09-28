#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createServer as createHttpServer, request as httpRequest } from "node:http";
import { createServer as createHttpsServer, request as httpsRequest } from "node:https";
import { connect as tcpConnect } from "node:net";
import { connect as tlsConnect } from "node:tls";
import { once } from "node:events";
import { URL } from "node:url";

const SERVER_PREFIXES = [
  "/auth",
  "/requirements",
  "/events",
  "/daemon",
  "/daemons",
  "/users",
  "/repositories",
];

function serverPath(pathname, request) {
  const requirementPage = /^\/requirements\/[^/]+$/.test(pathname);
  const browserPageRequest =
    requirementPage &&
    (request?.method === "GET" || request?.method === "HEAD") &&
    (request.headers["sec-fetch-dest"] === "document" ||
      request.headers.rsc === "1" ||
      request.headers["next-router-prefetch"] ||
      request.headers["next-router-segment-prefetch"] ||
      String(request.headers.accept ?? "").toLowerCase().includes("text/html"));
  if (browserPageRequest) return false;
  return SERVER_PREFIXES.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`));
}

function certificates(supplied) {
  if (!supplied) {
    throw new Error("operator-managed --cert-file and --key-file are required");
  }
  return {
    key: readFileSync(supplied.keyFile),
    cert: readFileSync(supplied.certFile),
  };
}

function targetRequest(target, req, res) {
  const url = new URL(req.url ?? "/", target);
  const headers = { ...req.headers, host: url.host, "x-forwarded-proto": "https" };
  const requestImpl = url.protocol === "https:" ? httpsRequest : httpRequest;
  const upstream = requestImpl({
    protocol: url.protocol,
    hostname: url.hostname,
    port: url.port || (url.protocol === "https:" ? 443 : 80),
    path: `${url.pathname}${url.search}`,
    method: req.method,
    headers,
  }, (upstreamResponse) => {
    res.writeHead(upstreamResponse.statusCode ?? 502, upstreamResponse.headers);
    upstreamResponse.pipe(res);
  });
  upstream.on("error", () => {
    if (!res.headersSent) res.writeHead(502);
    res.end("proxy upstream unavailable\n");
  });
  req.on("aborted", () => upstream.destroy());
  req.pipe(upstream);
}

function targetUpgrade(target, req, client, head) {
  const url = new URL(req.url ?? "/", target);
  const port = Number(url.port || 80);
  const upstream = tcpConnect({ host: url.hostname, port }, () => {
    const headers = Object.entries({ ...req.headers, host: url.host })
      .filter(([name]) => name !== "proxy-connection")
      .map(([name, value]) => `${name}: ${Array.isArray(value) ? value.join(", ") : value}`)
      .join("\r\n");
    upstream.write(`${req.method} ${url.pathname}${url.search} HTTP/1.1\r\n${headers}\r\n\r\n`);
    if (head.length) upstream.write(head);
    client.pipe(upstream);
    upstream.pipe(client);
  });
  const close = () => {
    client.destroy();
    upstream.destroy();
  };
  upstream.on("error", close);
  client.on("error", () => upstream.destroy());
}

function listen(server, host = "127.0.0.1", port = 0) {
  server.listen(port, host);
  return once(server, "listening").then(() => server.address());
}

export function createProxy({ serverUrl, webUrl, certFile, keyFile, host = "127.0.0.1", port = 0 }) {
  if (Boolean(certFile) !== Boolean(keyFile)) {
    throw new Error("--cert-file and --key-file must be supplied together");
  }
  const tls = certificates(certFile && keyFile ? { certFile, keyFile } : undefined);
  const activeEventStreams = new Set();
  const pendingEventStreams = new Set();
  let eventStreamsPaused = false;
  const forwardEventStream = (req, res) => {
    activeEventStreams.add(res);
    res.once("close", () => activeEventStreams.delete(res));
    targetRequest(serverUrl, req, res);
  };
  const server = createHttpsServer({ key: tls.key, cert: tls.cert }, (req, res) => {
    const pathname = new URL(req.url ?? "/", `https://${req.headers.host ?? "localhost"}`).pathname;
    const disconnectPath = "/__release_test__/sse/disconnect";
    const reconnectPath = "/__release_test__/sse/reconnect";
    if (pathname === disconnectPath || pathname === reconnectPath) {
      if (req.method !== "POST") {
        res.writeHead(405, { allow: "POST" });
        res.end();
        return;
      }
      if (pathname === disconnectPath) {
        eventStreamsPaused = true;
        for (const response of activeEventStreams) response.destroy();
      } else {
        eventStreamsPaused = false;
        for (const pending of [...pendingEventStreams]) {
          pendingEventStreams.delete(pending);
          pending.req.off("aborted", pending.remove);
          pending.res.off("close", pending.remove);
          if (!pending.req.aborted && !pending.req.destroyed && !pending.res.destroyed) {
            forwardEventStream(pending.req, pending.res);
          }
        }
      }
      res.writeHead(204);
      res.end();
      return;
    }
    if (pathname === "/events") {
      if (eventStreamsPaused) {
        const pending = { req, res, remove: null };
        pending.remove = () => pendingEventStreams.delete(pending);
        pendingEventStreams.add(pending);
        req.once("aborted", pending.remove);
        res.once("close", pending.remove);
      } else {
        forwardEventStream(req, res);
      }
      return;
    }
    targetRequest(serverPath(pathname, req) ? serverUrl : webUrl, req, res);
  });
  server.on("upgrade", (req, client, head) => {
    const pathname = new URL(req.url ?? "/", `https://${req.headers.host ?? "localhost"}`).pathname;
    targetUpgrade(serverPath(pathname) ? serverUrl : webUrl, req, client, head);
  });

  return {
    server,
    tls,
    async start() {
      const address = await listen(server, host, port);
      return {
        host,
        port: typeof address === "object" && address ? address.port : port,
        url: `https://localhost:${typeof address === "object" && address ? address.port : port}`,
      };
    },
    async close() {
      if (!server.listening) return;
      server.closeAllConnections?.();
      await new Promise((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      });
    },
  };
}

function args(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index];
    if (!name.startsWith("--")) throw new Error(`unexpected argument: ${name}`);
    if (name === "--self-check") {
      values.set(name, true);
      continue;
    }
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`);
    values.set(name, value);
    index += 1;
  }
  return values;
}

async function proxySelfCheck(certFile, keyFile) {
  if (!certFile || !keyFile) {
    throw new Error("--self-check requires operator-managed --cert-file and --key-file");
  }
  const serverUpstream = createHttpServer((req, res) => {
    if (req.url === "/events") {
      res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
      res.end('data: {"kind":"change"}\n\n');
      return;
    }
    res.end("server\n");
  });
  serverUpstream.on("upgrade", (_req, socket) => {
    socket.write("HTTP/1.1 101 Switching Protocols\\r\\nConnection: Upgrade\\r\\nUpgrade: websocket\\r\\n\\r\\n");
    socket.end();
  });
  const webUpstream = createHttpServer((_req, res) => res.end("web\n"));
  const serverAddress = await listen(serverUpstream);
  const webAddress = await listen(webUpstream);
  const proxy = createProxy({
    serverUrl: `http://127.0.0.1:${serverAddress.port}`,
    webUrl: `http://127.0.0.1:${webAddress.port}`,
    certFile,
    keyFile,
  });
  try {
    const address = await proxy.start();
    const fetch = (path, { method = "GET", headers = {} } = {}) => new Promise((resolve, reject) => {
      const request = httpsRequest({ hostname: "localhost", port: address.port, path, method, headers, rejectUnauthorized: true }, (response) => {
        let body = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => { body += chunk; });
        response.on("end", () => resolve({ status: response.statusCode, headers: response.headers, body }));
      });
      request.on("error", reject);
      request.end();
    });
    const web = await fetch("/");
    assert.equal(web.status, 200);
    assert.equal(web.body, "web\n");
    const requirementPage = "/requirements/release-probe";
    assert.equal((await fetch(requirementPage, { headers: { accept: "text/html", "sec-fetch-dest": "document" } })).body, "web\n");
    assert.equal((await fetch(requirementPage, { headers: { rsc: "1" } })).body, "web\n");
    assert.equal((await fetch(requirementPage, { headers: { "next-router-prefetch": "1" } })).body, "web\n");
    assert.equal((await fetch(requirementPage, { headers: { "next-router-segment-prefetch": "1" } })).body, "web\n");
    assert.equal((await fetch(requirementPage)).body, "server\n");
    assert.equal((await fetch(`${requirementPage}/readiness`, { headers: { rsc: "1" } })).body, "server\n");
    assert.equal((await fetch(requirementPage, { method: "POST", headers: { accept: "text/html" } })).body, "server\n");
    const events = await fetch("/events");
    assert.equal(events.status, 200);
    assert.match(events.headers["content-type"], /text\/event-stream/);
    assert.match(events.body, /change/);

    await new Promise((resolve, reject) => {
      const socket = tlsConnect({ host: "localhost", port: address.port, rejectUnauthorized: true }, () => {
        socket.write("GET /daemon/ws HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n");
      });
      let response = "";
      socket.setEncoding("utf8");
      socket.on("data", (chunk) => {
        response += chunk;
        if (response.startsWith("HTTP/1.1 101")) {
          socket.destroy();
          resolve();
        }
      });
      socket.on("error", reject);
      setTimeout(() => reject(new Error("WSS upgrade timeout")), 2_000).unref();
    });
    console.log("proxy self-check: OK");
  } finally {
    await proxy.close();
    serverUpstream.close();
    webUpstream.close();
  }
}

async function main() {
  const options = args(process.argv.slice(2));
  if (options.get("--self-check")) {
    return proxySelfCheck(options.get("--cert-file"), options.get("--key-file"));
  }
  const serverUrl = options.get("--server-url");
  const webUrl = options.get("--web-url");
  const certFile = options.get("--cert-file");
  const keyFile = options.get("--key-file");
  if (!serverUrl || !webUrl || !certFile || !keyFile) {
    throw new Error("--server-url, --web-url, --cert-file, and --key-file are required");
  }
  const proxy = createProxy({
    serverUrl,
    webUrl,
    certFile,
    keyFile,
    port: Number(options.get("--port") ?? 0),
  });
  const address = await proxy.start();
  console.log(JSON.stringify(address));
  const stop = async () => {
    await proxy.close();
    process.exit(0);
  };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`release proxy: ${error.message}`);
    process.exit(1);
  });
}
