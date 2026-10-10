# Tasks

## 1. Local proxy configuration

- [x] 1.1 Add `docker-compose.proxy.yaml` as an opt-in overlay; publish the proxy on its configured loopback port and document the distinct `NORTH_WEB_HOST_PORT` needed to avoid a port collision. Verify merged config for base+proxy and base+build+proxy; confirm release Compose alone is unchanged.
- [x] 1.2 Add ordered Caddy routes for server prefixes and exact `/requirements/{id}` document/RSC/prefetch `GET`/`HEAD` requests. Validate Caddyfile and smoke route selection through the running stack for page, API, subpath, `/events`, and `/daemon/ws` requests without sending state-changing requests.

## 2. Operator guidance and integration validation

- [x] 2.1 Document local proxy commands for release images and optional local source builds; distinguish loopback HTTP from production operator-managed TLS proxy. Verify commands match Compose file order and existing port variables.
- [x] 2.2 Run strict OpenSpec validation, focused Compose/Caddy and HTTP route checks, relevant repository validation, and pre-push validation based on the actual changed-file set. Review diff without altering pre-existing work.
