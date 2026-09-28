# North

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/r05323028/north/ci.yml)
![GitHub License](https://img.shields.io/github/license/r05323028/north)
[![Quality gate status](https://sonarcloud.io/api/project_badges/measure?project=north&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=north)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=north&metric=coverage)](https://sonarcloud.io/summary/new_code?id=north)
[![Lines of Code](https://sonarcloud.io/api/project_badges/measure?project=north&metric=ncloc)](https://sonarcloud.io/summary/new_code?id=north)
[![Duplicated Lines (%)](https://sonarcloud.io/api/project_badges/measure?project=north&metric=duplicated_lines_density)](https://sonarcloud.io/summary/new_code?id=north)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=north&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=north)

Self-hosted requirement management: requesters collaborate with an AI agent to turn
ambiguous requests into structured, reviewable requirements.

Status: **under active development** — roadmap lives in `openspec/changes/`.

## Layout

```text
apps/web/            Next.js UI (App Router, Tailwind CSS, shadcn/ui)
web/                 Astro/Starlight human docs site (user guide, contributors, changelog)
crates/
  north-domain/      pure requirement business behavior (no infra)
  north-server/      HTTP/SSE host; owns business state transitions
  north-daemon/      local execution host; reports facts/events
  north-protocol/    wire types shared by server and daemon
  north-persistence/ durable storage implementation
tests/
  architecture/    structural architecture enforcement (runs in cargo test)
docs/                canonical product/architecture/development documentation
crates/north-persistence/src/m0001_initial_schema.rs  SeaORM baseline migration
openspec/            change management (proposal → specs → design → tasks)
```

## Development quickstart

```bash
# Rust (workspace root)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Web app
cd apps/web && npm ci && npm run lint && npm run typecheck && npm run build

# Human documentation site
cd web && npm ci && npm run dev

# Specs
openspec validate --all --strict
```

Start with `AGENTS.md`, then `docs/README.md`.

## Self-hosted deployment

Supported deployment topology and upgrade steps live in [`docs/deployment/self-hosted.md`](docs/deployment/self-hosted.md). Release artifacts use one same-origin TLS proxy for Next.js HTTP, server HTTP/SSE, and daemon WSS. The browser never opens WebSockets. Install the matching `north`/`north-daemon` CLI archive, run `north setup --server-url https://north.example`, and manage the host daemon with `north daemon start`, `north daemon stop`, and `north daemon status`. See the operator guide for archive checksums and protected state paths.

Set `VERSION` in your shell to the target release version (`X.Y.Z`, without `v`), then build and verify an extracted package:

```bash
./scripts/release.sh package "$VERSION"
(cd "dist/north-v$VERSION" && shasum -a 256 -c checksums.sha256)
```

Full release evidence checklist: [`docs/development/release-checklist.md`](docs/development/release-checklist.md).

Protected-main runs build and qualify OCI archives but publish no images to GHCR.
An authorized strict `vX.Y.Z` Git tag starts a fresh build and qualification;
only tag-built images publish under matching SemVer refs, and the GitHub Release
becomes public after both images publish. Example 0.1.0 refs are
`ghcr.io/r05323028/north-server:v0.1.0` and
`ghcr.io/r05323028/north-web:v0.1.0`. Pin images with `@sha256:` digests from
the tag workflow summary. Use
`docker-compose.yaml` from the matching source tag; set `POSTGRES_DB`,
`POSTGRES_USER`, `POSTGRES_PASSWORD`, `DATABASE_URL`, `NORTH_OTP_HMAC_KEY`,
`NORTH_SERVER_IMAGE`, and `NORTH_WEB_IMAGE` before startup:

```bash
git show "v0.1.0:docker-compose.yaml" > docker-compose.yaml
docker compose --file docker-compose.yaml up --detach postgres
docker compose --file docker-compose.yaml run --rm --no-deps north-server migrate
docker compose --file docker-compose.yaml up --detach
```

Compose keeps PostgreSQL data in a named volume and binds app ports to loopback;
TLS proxy stays external, and daemon remains host-managed. See the self-hosted
guide for secrets, backups, proxy routes, and digest pinning.
