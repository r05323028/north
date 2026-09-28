# Prepare North 0.1.0 release qualification

## Why

North's 0.1.0 product path is implemented across the browser workspace, Rust server, PostgreSQL persistence, daemon protocol, clarification runtime, readiness assessment, and human review surface, but release evidence is incomplete. Current Playwright coverage uses mocked server routes; the invariant ledger therefore marks browser SSE/refetch as only partially enforced. At change start, `./scripts/validate.sh smoke` was unsupported, no runnable `north-server` binary or release package existed, and the repository had no release workflow or operator runbook. Existing PostgreSQL integration is broad, but it does not prove a fresh database, production-shaped startup, or a built release artifact; upgrades from unreleased schemas are not supported.

Release qualification needs one reproducible proof that these existing subsystems compose over their real boundaries. It must also make startup failures, migration behavior, browser transport rules, packaging inputs, and 0.1.0 operating limits explicit before publication. This change adds a narrow operator-facing CLI for daemon setup and lifecycle; it does not add Requirement-management commands.

## What Changes

- Add one production-shaped golden-path qualification covering an authenticated browser/client, North HTTP/SSE, a real server-to-daemon protocol connection, clarification conversation projections, a server-validated readiness assessment, and a human review transition.
- Replace mocked-only SSE confidence with assembled server-backed browser coverage proving SSE is notification-only, canonical HTTP state is refetched after relevant events, reconnect/focus repairs recover state, and the browser never opens WebSockets.
- Add fresh-install migration qualification and production-like startup probes for `DATABASE_URL`, `NORTH_OTP_HMAC_KEY`, migration failure, PostgreSQL failure, and safe missing/invalid configuration errors.
- Package the existing `north-server` process, explicit `north-server migrate` operation, schema-verifying startup, and `/healthz` contract for release qualification; do not add automatic schema mutation to normal startup.
- Add `north` CLI with `setup` and `daemon start|stop|status`; bundle and supervise existing `north-daemon`, reuse its HTTPS device-approval flow, and report backend connection state without exposing credentials. Do not add Requirement-management commands or PAT/OAuth auth.
- Keep `north-server` in the backend OCI image; CLI archives contain only `north` and `north-daemon`.
- On protected-main pushes, run CI, build one Linux x86_64 server package and OCI set from the exact commit, qualify those inputs, and retain artifacts internally; never publish OCI images to GHCR from main.
- On strict `vX.Y.Z` tags, validate the exact main-reachable commit and matching Cargo/web/CLI versions. Permit an unchanged first-parent version only when the event tag is the sole strict SemVer ref in the explicitly fetched tag set; subsequent tags require a first-parent version bump. Build fresh tag-sourced server/web OCI images and CLI+daemon archives for Linux x86_64 plus macOS x86_64/ARM64, and qualify those exact outputs.
- Upload checksummed CLI archives to a draft GitHub Release first, publish the qualified tag-built SemVer OCI images to GHCR next, and publish the GitHub Release only after GHCR succeeds. Keep release-asset `contents: write` separate from OCI `packages: write`; publishers get no TLS secrets.
- Keep secret-free preflight separate from fresh hosted qualification; verify source, versions, checksums, artifact identity, and digests before provisioning existing TLS secrets only in the qualification job. No GitHub Environments, persistent self-hosted runners, or generated CA material.
- Document the minimum supported self-hosted topology, configuration, OTP key generation, database setup, daemon setup, startup, upgrades, known 0.1.0 limitations, release verification, and a concise maintainer checklist.
- Add `Dockerfile.server`, `apps/web/Dockerfile`, and `docker-compose.yaml` for `north-server` and `north-web` OCI images plus PostgreSQL. Main merges qualify internal OCI archives only; strict SemVer tags rebuild and qualify versioned OCI images from tag commit and are the only GHCR publication path. GitHub Release contains only CLI+daemon archives and checksums. Keep operator TLS proxy external and daemon host-managed.

- Build Linux server and CLI/daemon binaries in an immutable glibc 2.31-compatible builder and verify required GLIBC symbol versions in every packaged Linux ELF; record the same baseline in manifests.
- Require `cargo --locked` throughout release production and reject artifacts if packaging changes `HEAD` or source worktree.
- Run Conventional Commit PR-title validation in a dedicated lightweight workflow on PR edits, preserving required-check name while avoiding full CI reruns.
- Require at least one approving review in protected-main rules and verify release tags resolve to the expected full source SHA through GitHub's Git API.

## Capabilities

### New Capabilities

- `release-qualification`: assembled golden-path, browser notification/refetch, fresh-install/startup, built-release smoke, CLI/OCI release evidence, and publication contracts.
- `north-cli`: operator setup and safe lifecycle/status contract for the bundled local daemon.

### Modified Capabilities

- Existing Requirement, conversation, readiness, review, daemon, and server-runtime capabilities remain authoritative; `north-cli` wraps existing daemon enrollment/runtime without alternate business-state behavior.

## Contract markers

The following are release invariants, not optional implementation details:

- Browser communication remains HTTP + SSE only; SSE payloads are notification hints and never canonical business state.
- The server remains the sole readiness and Requirement-state authority; daemon assessment events are facts bound to the exact Requirement revision and session/run identity.
- `NORTH_OTP_HMAC_KEY` remains fail-closed and is exercised by the production-shaped startup path without secret leakage.
- Main package/OCI inputs are built once from each immutable merged-main commit and carried by one 14-day Actions artifact; qualification consumes those exact inputs. Main and manual-qualification paths never publish OCI images to GHCR.
- A strict matching SemVer tag rebuilds and qualifies tag-sourced OCI images plus version-matched `north`/`north-daemon` CLI archives. Only tag-built images publish to GHCR, under SemVer refs. The draft GitHub Release receives checksummed CLI archives before GHCR publication; it becomes public only after both exact image digests verify. Main OCI digests remain internal qualification evidence, not registry tags.
- Compose preserves same-origin browser HTTP/SSE and daemon WSS through the operator-managed TLS proxy; `north-daemon` remains host-managed.

The server binary shape, web packaging mechanism, local orchestration script names, and internal workflow package layout are implementation choices constrained by those invariants. No alternate internal-call test path may replace the assembled qualification path.

## Dependencies

- Current main implementations and archived changes for the Requirement workspace, conversations, daemon protocol/runtime, readiness, human review, runtime retention, public endpoint abuse protection, and OTP at-rest hardening.
- Materialized canonical `email-auth` and `otp-at-rest-hardening` specs from the archived `harden-otp-at-rest` change, including migration 0019, fail-closed OTP configuration, and unchanged session/daemon credential hashing.
- Existing `./scripts/validate.sh` profiles, CI `gate`, PostgreSQL-backed integration suites, Playwright web-boundary suite, and architecture tests.
- Docker Engine/Buildx/Compose on release qualification runners and a GHCR package namespace writable by the release workflow token; package visibility remains an owner-controlled setting.
- Repository-provided Rust/Node toolchains, PostgreSQL, and an operator-provided TLS/reverse-proxy boundary for production daemon HTTPS/WSS. Qualification SHALL use a deterministic fake agent through `NORTH_PI_AGENT_COMMAND`; it SHALL not depend on an installed Pi provider or external model.

## Non-goals

The 0.1.0 release does not add or pull forward:

- multi-server / HA connection ownership epochs;
- automatic session live migration;
- retry-idempotent recovery of lost daemon setup claim responses;
- a dedicated scheduler solely for setup-row cleanup;
- per-Requirement ACL;
- object storage; or
- an external message broker;
- a North-managed TLS reverse proxy or certificate lifecycle;
- a containerized daemon, Kubernetes/Helm deployment, or multi-architecture OCI images (CLI support is separately limited to Linux x86_64 and macOS x86_64/ARM64); or
- image signing, provenance attestations, or SBOM generation.

Existing single-server, workspace-wide access, opportunistic setup cleanup, one-shot setup claim, local daemon journal, log delivery, and other documented trade-offs remain intentional unless a qualification test proves they prevent a safe release. New CLI surface is limited to setup and daemon lifecycle/status; no Requirement workflow commands are added.

## Documentation impact

Implementation of this change updates these canonical documents in the same change:

- `docs/development/invariants.md` — replace the browser SSE/refetch partial status only if assembled evidence passes; name the exact test and retain honest statuses for all deferred boundaries.
- `docs/development/testing.md` — add the named assembled E2E, fresh-install/startup, built-release smoke commands, prerequisites, and truthful layer classification.
- `docs/development/ci.md` — document main qualification without GHCR publication, tag-built CLI/OCI SemVer-only publication order, permissions, required PR/`gate` and tag rules, repository TLS secrets, and separation from the PR `gate`.
- `docs/architecture/overview.md` and `docs/architecture/server-daemon-protocol.md` — record the supported release topology and server/daemon/browser boundary only if the implementation changes the executable delivery surface.
- `docs/deployment/self-hosted.md` — CLI install/setup and daemon lifecycle plus minimum operator setup and upgrade runbook for GHCR/Compose, including the external proxy boundary.
- `docs/development/release-checklist.md` — maintainer checklist for merged commit/version checks, qualification, CLI archive checksums, draft-release ordering, source SHA/internal OCI evidence, and SemVer registry digests.
- `README.md` — link the 0.1.0 operator and maintainer documents and replace development-only delivery ambiguity with the supported artifact path.
