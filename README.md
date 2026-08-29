# North

![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/r05323028/north/ci.yml)
![GitHub License](https://img.shields.io/github/license/r05323028/north)

Self-hosted requirement management: requesters collaborate with an AI agent to turn
ambiguous requests into structured, reviewable requirements.

Status: **0.1.0 under active development** — roadmap lives in `openspec/changes/`.

## Layout

```text
apps/web/            Next.js UI (App Router, Tailwind CSS, shadcn/ui)
crates/
  north-domain/      pure requirement business behavior (no infra)
  north-server/      HTTP/SSE host; owns business state transitions
  north-daemon/      local execution host; reports facts/events
  north-protocol/    wire types shared by server and daemon
  north-persistence/ durable storage implementation
tests/
  architecture/    structural architecture enforcement (runs in cargo test)
docs/                canonical product/architecture/development documentation
migrations/          versioned SQL migrations
openspec/            change management (proposal → specs → design → tasks)
```

## Development quickstart

```bash
# Rust (workspace root)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Web
cd apps/web && npm ci && npm run lint && npm run typecheck && npm run build

# Specs
openspec validate --all --strict
```

Start with `AGENTS.md`, then `docs/README.md`.
