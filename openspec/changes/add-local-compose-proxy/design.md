# Design

## Context

See proposal.md for motivation. `docker-compose.yaml` intentionally remains image-only and publishes North server and Next.js separately; `docker-compose.build.yaml` is an independent local source-build overlay. The supported production deployment uses an operator-managed TLS proxy. Compose's project default bridge already provides DNS connectivity between `north-proxy`, `north-server`, and `north-web`.

## Goals / Non-Goals

**Goals:**

- Add local same-origin routing while preserving the existing browser origin and host port.
- Keep release Compose and the existing build overlay behavior unchanged when the new overlay is omitted.
- Preserve API, session, SSE, and daemon WebSocket semantics, including the `/requirements/{id}` page/API collision.

**Non-Goals:**

- Replace or manage the production TLS proxy, certificates, or public ingress.
- Change Next.js API URLs, authentication, server routes, or browser WebSocket behavior.
- Publish the proxy beyond loopback or hide the existing loopback server port.

## Decisions

### Use a separate opt-in Compose overlay

Add `docker-compose.proxy.yaml`; do not modify `docker-compose.yaml` or `docker-compose.build.yaml`. Compose appends port lists, so the proxy overlay uses `NORTH_PROXY_HOST_PORT` (default `3000`) while the operator sets `NORTH_WEB_HOST_PORT` to a distinct loopback port (default choice: `3001`) for the Next.js container. Pass that override to every Compose command using the overlay. This avoids Compose-specific YAML merge tags and leaves base behavior unchanged when the overlay is omitted. The optional overlay may be combined after either the base file alone or the existing source-build overlay.

### Use Caddy as local HTTP edge

Run the official `caddy:2-alpine` image with a read-only Caddyfile mount. Caddy listens on container port 80 and binds only `127.0.0.1:${NORTH_PROXY_HOST_PORT:-3000}` on the host. It shares Compose's default bridge and reaches upstreams by service DNS (`north-server:8080`, `north-web:3000`). No `network_mode: host` or separate browser API origin is introduced. Production remains behind operator-managed TLS.

### Route by path plus request semantics

Use an ordered Caddy `route` block. First route exact `/requirements/{id}` `GET`/`HEAD` browser document (`Sec-Fetch-Dest: document` or `Accept: text/html`) and Next RSC/prefetch headers to Next.js. Then route server prefixes (`/auth`, `/requirements`, `/events`, `/daemon`, `/daemons`, `/users`, `/repositories`) to North server; send all remaining traffic to Next.js. Exact one-segment matching prevents subpaths and mutations from being mistaken for page requests. Caddy preserves end-to-end headers by default and adds standard `X-Forwarded-*` metadata; no auth header or cookie rewriting is needed.

Caddy reverse proxy handles HTTP streaming and WebSocket upgrades. Do not add response buffering, compression, or a separate WebSocket listener.

### Document local and production paths separately

The runbook will set `NORTH_WEB_HOST_PORT=3001` when invoking Compose with the proxy overlay, leaving `NORTH_PROXY_HOST_PORT` at `3000`; source builds add `docker-compose.build.yaml` before the proxy overlay. The proxy overlay is loopback-only HTTP for local use. Production instructions continue to require an operator-managed TLS proxy and documented route table.

## Risks / Trade-offs

- `/requirements/{id}` headers can be absent or vary between Next versions → keep method + exact path + explicit document/RSC/prefetch matchers; headerless/API requests and subpaths fall through to North server; validate both route classes.
- Caddy image must be fetched when not cached → Compose uses the official maintained v2 Alpine image; the proxy remains optional and loopback-only.
- Local HTTP is not suitable for public deployment → clearly scope overlay to local development; production TLS, trust, and certificate lifecycle remain operator-owned.
- Both services publish loopback ports in proxy mode → require distinct `NORTH_WEB_HOST_PORT` and `NORTH_PROXY_HOST_PORT` values; document the default local pair and validate merged base/build/proxy configuration.

## Migration Plan

1. Set `NORTH_WEB_HOST_PORT` to a free loopback port distinct from `NORTH_PROXY_HOST_PORT` (for example, `3001` and `3000`).
2. Validate base and merged Compose configurations and Caddyfile before starting the overlay.
3. Start with base + proxy overlay (add build overlay only for source-built images); Next.js remains reachable on its secondary local port and Caddy owns the browser origin.
4. Verify `/`, auth API routing, requirement page/API selection, SSE path, and daemon WebSocket upgrade path.
5. To roll back, stop the proxy overlay without removing named volumes, then start the base Compose stack again. Do not use `down --volumes`.
