# Proposal

## Why

The Compose stack publishes Next.js on `:3000` and North server on `:8080`, but browser API calls use relative same-origin paths. Direct access to the web port therefore sends `/auth/**` and other server routes to Next.js, which returns 404. Operators need an opt-in local edge that preserves the browser's existing origin without changing the release Compose contract.

## What Changes

- Add an optional Compose overlay that places a lightweight HTTP reverse proxy on the existing loopback web port and moves Next.js to a separate local port to avoid a host-port conflict.
- Route server API, SSE, and daemon WebSocket paths to `north-server`; route pages and assets to `north-web`.
- Route `/requirements/{id}` document/RSC/prefetch `GET`/`HEAD` requests to Next.js while keeping API requests, mutations, and subpaths on the server.
- Document local overlay usage and distinguish it from production deployments, which continue using an operator-managed TLS proxy.
- Keep `docker-compose.yaml`, `docker-compose.build.yaml`, release images, browser API URLs, and authentication behavior unchanged.

## Capabilities

### New Capabilities
- `local-compose-proxy`: Optional local Compose same-origin routing for North web, server, SSE, and daemon WebSocket traffic.

### Modified Capabilities
- None.

## Impact

- Adds a local-only Compose proxy overlay and Caddyfile; does not add a proxy to release Compose or manage TLS certificates.
- Updates `docs/deployment/self-hosted.md` with local-stack instructions.
- Depends on existing Compose services, their default bridge network, and existing `/auth`, `/requirements`, `/events`, `/daemon`, `/daemons`, `/users`, and `/repositories` contracts.
