# Design: North 0.1.0 release qualification

## Context

See `proposal.md` for motivation and scope. Current main has real PostgreSQL integration, server-owned clarification/readiness/review paths, a daemon-initiated WebSocket protocol, browser EventSource refetch logic, and mocked-route Playwright coverage. It does not have a runnable `north-server` binary, a built-release smoke path, a production package contract, or a release workflow. `migrations/README.md` records the consolidated `0001_initial_schema.sql` baseline; `fresh_install` verifies an empty install, while unsupported historical-upgrade coverage is removed. `validate.sh smoke` exits as unsupported. Browser SSE remains partially enforced because existing Playwright routes fulfill `/events` and API responses in-process rather than using a live server/database.

The design must preserve these boundaries:

The existing `server-runtime` capability owns the server process and migration contract; this change packages and qualifies that behavior without changing it. The release-qualification capability owns evidence and delivery gates. Existing daemon/protocol specs remain authoritative for their wire and runtime behavior.

- Browser → North uses HTTP + SSE only; browser SSE carries notification identity/category hints, never business state.
- Server owns PostgreSQL state, Requirement transitions, readiness gates, review authorization, command outbox, and daemon routing.
- Daemon initiates the persistent server connection, invokes a deterministic local agent fixture only in qualification, and reports typed facts/events.
- The pre-0.1.0 history is consolidated under `consolidate-pre-release-migrations`; databases stamped by that history are not automatically upgraded or reset. Freeze the 0.1.0 baseline after release and append later migrations. OTP startup uses the already-implemented `NORTH_OTP_HMAC_KEY` validation and HMAC behavior.

## Goals / Non-Goals

**Goals:**

- Produce one hermetic assembled path that exercises real HTTP, PostgreSQL, SSE, WebSocket protocol, daemon runtime, browser reads, readiness, and human review boundaries.
- Make the release artifact runnable and inspectable without requiring an external model provider.
- Make fresh-install, startup-failure, version, checksum, and operator evidence reproducible.
- Keep qualification hermetic: no external model/provider or network dependency, isolated HOME/XDG state/config directories, deterministic fixture output, bounded timeouts, and cleanup on success or failure.
- Protected-main pushes build and qualify OCI artifacts but never publish OCI images to GHCR. A strict SemVer tag separately builds and qualifies tag-sourced CLI/OCI outputs; upload CLI assets to a draft GitHub Release, publish only SemVer OCI refs to GHCR, then publish the Release, while keeping PR `gate` as merge check.

**Non-goals:**

- Replacing the existing web workspace, server router, daemon protocol, readiness model, or review model.
- Adding Requirement-management commands, OAuth/PAT auth, profiles, a product health dashboard, new auth delivery provider, HA coordination, live migration, ACL, object storage, broker, or generalized deployment platform.
- Adding an insecure daemon transport mode, alternate daemon URL semantics, or a second production server/router path.
- Providing signed provenance or an SBOM in 0.1.0; checksums provide integrity evidence only, with signing/provenance deferred.

## Decisions

### 1. Package existing server process without changing migration policy

The existing `north-server` binary is the release server entrypoint. It will:

1. require `DATABASE_URL` and a valid `NORTH_OTP_HMAC_KEY`;
2. read `NORTH_BIND_ADDR` with its documented loopback default;
3. expose `north-server migrate` as the explicit SeaORM migration operation;
4. verify the current migration head during normal startup without applying schema changes; and
5. expose safe `GET /healthz` and serve the existing authenticated Axum router with bounded graceful shutdown.

Release qualification and Compose SHALL run `north-server migrate` before normal server startup on a fresh database. The process reuses `build_app` and existing persistence/router setup; no alternate server path is added. It logs only safe failure classes and never logs the OTP key, database URL credentials, cookies, codes, or daemon credentials. Configuration, database, or schema-verification failure exits before the listener accepts traffic. Test runs use an ephemeral bind port and isolated HOME/XDG state/config directories.

**Alternative rejected:** using an integration-test-only router as the release artifact leaves no supported server process to package or smoke-test. Automatically mutating schema during ordinary startup conflicts with the existing explicit-migration contract.

### 2. Keep browser same-origin through an operator proxy

The supported production topology is:

```text
browser ── HTTPS/HTTP + SSE ── operator TLS/reverse proxy ── Next web
                                      ├─ server HTTP routes + /events
                                      └─ /daemon/ws (WSS) ── north-server
north-daemon ── outbound WSS ── same public origin ── /daemon/ws
north-server ── PostgreSQL
```

The proxy routes server-owned prefixes (`/auth`, `/requirements`, `/events`, `/daemon`, `/daemons`, `/users`, `/repositories`) to `north-server` and the remaining web paths to Next. It must preserve cookies, SSE streaming, and WebSocket upgrade for `/daemon/ws`. The browser therefore sees one origin and never needs a daemon URL or browser WebSocket. A vendor-neutral route table plus one minimal example configuration goes in the operator runbook; North does not add a proxy dependency or manage certificates.

The release web artifact uses the existing Next application in production mode. The package includes the production output and its Node runtime metadata; no browser API rewrite or alternate client transport is introduced. The daemon's configured `--server-url` is the same public HTTPS origin, so setup approval and WSS use one operator-visible address.

**Alternative rejected:** adding a Rust static-file server or a second browser API namespace would duplicate Next routing and create a new product delivery path. Next rewrites alone do not provide the required daemon WebSocket deployment contract.

### 3. Compose qualification from real boundaries

Create a release qualification harness under the existing validation/tooling surfaces, not under `crates/`. It will launch or connect to:

- a fresh/isolated PostgreSQL database;
- the built `north-server` binary;
- the built Next production artifact;
- a local TLS reverse-proxy fixture that reads a persistent operator-managed server certificate and key, mirrors the documented same-origin route table, preserves SSE, and upgrades `/daemon/ws` to WSS; and
- the existing daemon runtime qualification peer, using platform trust roots, which performs the production North handshake/coordination contract against the live server with the deterministic agent fixture.

The browser test will use the real web page and live HTTP/SSE responses. It will authenticate through the existing request-code/verify endpoints, reading the intentional `LogCodeDelivery` test output from a private test log rather than adding a test-only auth endpoint. It will create a Requirement and conversation through canonical APIs. The daemon side will use `NORTH_PI_AGENT_COMMAND` pointed at a checked-in deterministic fixture that emits one structured assessment; no network model/provider is allowed.

The assembled golden-path test will assert:

1. authenticated browser/client state is created by server responses;
2. requester message persistence precedes clarification intent;
3. server selects/pins a daemon session and sends real protocol commands;
4. daemon facts project to conversation/activity and readiness evidence;
5. server revision/session/repository gates produce Ready;
6. browser fetches Review Packet from canonical HTTP state; and
7. a reviewer action produces the expected guarded human transition and durable state.

A separate focused protocol/integration assertion will cover invalid/stale/duplicate assessment facts. The qualification peer is the existing daemon runtime/protocol path, not a hand-written HTTP mock or an in-process server handler. It connects through the fixture's `wss://` endpoint and verifies the operator-provided certificate through platform trust roots rather than disabling certificate validation or generating a test CA; the production URL parser and HTTPS/WSS-only CLI contract remain unchanged. Local qualification never changes workstation trust; the hosted workflow imports the existing CA into its ephemeral OS trust store and removes it in an unconditional cleanup step. The built daemon artifact is smoke-tested with its version/help and runtime configuration checks. Existing component tests remain; this test is the composition proof, not a replacement implementation.

### 4. Prove SSE semantics with live state changes

Add a server-backed Playwright scenario separate from current mocked-route tests. It will record browser network requests and WebSocket events while a live server/database/daemon fixture changes canonical state.

The scenario will:

- trigger a server-side Requirement/conversation/readiness change and verify an SSE notification is followed by canonical GET/refetch;
- deliver duplicate or delayed notifications and assert no duplicate business state;
- interrupt the SSE connection, change canonical state while disconnected, then use reconnect, page focus, or visibility recovery to prove the current state is refetched;
- provide misleading payload-shaped notification data where the transport permits it and assert the DOM follows the HTTP response, not the hint; and
- fail if Playwright observes any browser WebSocket.

The existing structural `browser_never_opens_websockets` test remains a second, mechanical guard. Only after this assembled scenario passes will `docs/development/invariants.md` move the SSE/refetch row from Partially Enforced to Enforced.

### 5. Fresh-install and startup qualification

The pre-0.1.0 history is consolidated by `consolidate-pre-release-migrations`. Add/retain an ignored PostgreSQL integration test or release harness command with an isolated database/schema that starts empty, invokes `north-server migrate` through the existing SeaORM runner, and asserts the single baseline head and required tables. Normal server startup verifies that head and does not apply DDL. Historical upgrade evidence for unpublished schemas is intentionally removed; after publication, keep the baseline immutable and append later migrations.

Test the explicit migration command on a fresh database and on migration failure. Run the built server under a matrix of:

- current migration head + valid `DATABASE_URL` + valid 64-character hex `NORTH_OTP_HMAC_KEY`;
- missing key;
- malformed/short/non-hex key;
- unavailable PostgreSQL; and
- missing or behind schema.

The valid case must serve `/healthz` as a process/liveness signal and a basic authenticated request. Failure cases must exit before accepting routes and must be checked for secret-free output. The qualification command records `NORTH_TEST_DATABASE_URL` prerequisites explicitly instead of silently skipping; operator docs must not treat `/healthz` as continuous database readiness.

### 6. Package one immutable release input

Main pushes build the server/web package and OCI set from one immutable merge SHA. A strict `vX.Y.Z` tag must target a main-reachable commit. The first-release exception applies only when the explicitly fetched tag set has no other strict SemVer ref after excluding the event tag by exact name; later releases require a matching first-parent version bump. Its separate build creates fresh tag-sourced package/OCI inputs plus version-matched CLI archives. Both paths verify Cargo/web versions and full source SHA. Each path uploads its exact inputs under one immutable workflow artifact ID:

- `north-server` binary;
- `north-daemon` binary;
- production Next web output plus runtime metadata;
- checked-in migrations and the operator/release docs;
- version/commit manifest; and
- SHA-256 checksum file.

The Linux server package and Linux CLI/daemon archive build on amd64 with `rust:1.97.1-bullseye@sha256:02d78ca3f928195c2a907543de778adfd728ad7e2a24fdc6aef582b7c77842e0`; builder must report glibc 2.31. `readelf --version-info --wide` verifies required GLIBC symbols for every packaged Linux ELF, with any version above 2.31 or unreadable binary failing closed. Both server and Linux CLI manifests record `glibc_baseline: 2.31`; macOS CLI manifests record `null`. Cargo metadata/build/test commands use `--locked`, and package flows recheck `HEAD` and clean source after build and assembly. The CLI/daemon release matrix remains Linux x86_64, macOS x86_64, and macOS ARM64. Qualification downloads each exact built input; publishers never rebuild after qualification.

**Alternative rejected:** bundling `north-server` into the CLI would duplicate the backend OCI delivery path; rebuilding in a publisher would let qualified and published artifacts drift. The CLI matrix is limited to the three declared targets until additional targets are qualified.

### 7. Separate CI, qualification, package, and publication jobs

Add `.github/workflows/release.yml` with explicit main-push and `v*` tag-push paths plus qualification-only manual dispatch. Main pushes run after PR merges. Repository rules are part of the trust boundary: they SHALL require at least one approving PR review and the existing required checks (`Rust (fmt, clippy, unit+architecture)`, `PR title (Conventional Commit)`, and `merge gate`), retain squash-only merge and linear history, block direct pushes and bypasses, and restrict creation/update/deletion of `v*` tags to authorized release maintainers. The workflow does not create GitHub Environments.

A separate lightweight PR-title workflow SHALL run on `opened`, `synchronize`, `reopened`, `ready_for_review`, `review_requested`, and `edited`, preserving required context `PR title (Conventional Commit)`; edited events SHALL NOT trigger full CI.

1. **CI qualification** — run supported repository validation on the exact full `GITHUB_SHA`.
2. **Build/package** — use a full-history checkout, require a clean tree and `HEAD == GITHUB_SHA`, derive one matching semver from `north-server` Cargo metadata and `apps/web/package.json`, and compare it with the first parent's version. Use `cargo --locked` and recheck `HEAD` plus source cleanliness after build/package. Build the internal package and both OCI images once. Upload package, OCI archives, checksums, and image metadata/digests under one workflow artifact ID with 14-day retention.
3. **Release preflight** — no TLS secrets. Download by immutable artifact ID and verify upload digest, source SHA, package manifest/checksums, OCI archive checksums, and OCI manifest digests.
4. **Release qualification** — fresh `ubuntu-latest` runner after preflight. Re-download and reverify the same artifact, then reference the three existing TLS repository secrets only in the TLS-provisioning step. Install the existing CA in ephemeral trust and remove trust/files with unconditional cleanup. Run the extracted-artifact and Compose qualification.
5. **No main GHCR publisher** — protected-main qualification retains its package/OCI artifact internally and loads exact OCI archives into local Compose. Main and manual-qualification paths have no `packages: write` job and never write GHCR. Only the strict tag path can publish OCI images.
6. **Build and qualify tag outputs** — a strict `v*` tag push runs tag CI, builds fresh package/OCI inputs and Linux/macOS CLI archives from the exact tag commit, and runs secret-free preflight plus fresh hosted qualification on those same artifact IDs. Require tag ancestry and workflow/source agreement with `origin/main`; explicitly fetch all tags. Permit unchanged first-parent version only when no other strict tag ref currently exists (exclude the event tag by exact name); subsequent tags require a version bump. Deleted historical tags cannot be detected.
7. **Upload CLI assets to a draft Release** — resolve/dereference the GitHub tag ref and compare its full commit SHA with the expected source before draft creation and after each release state transition; do not treat `targetCommitish` as tag identity. Retain `--verify-tag`, then upload only the three checksummed CLI+daemon archives. This job alone receives `contents: write`; it has no TLS secrets or package write.
8. **Publish SemVer OCI** — after draft assets upload, publish the exact qualified tag-built server/web OCI archives to `vX.Y.Z` GHCR refs. Preflight both destinations, accept only matching existing digests, and fail on conflicts. This job alone receives `packages: write`; it has no TLS secrets, source checkout, or rebuild.
9. **Finalize Release** — publish the draft only after both OCI images succeed; a failed image publication leaves CLI assets in an unpublished draft. This job receives `contents: write` only. Main pushes and manual dispatch remain qualification-only for OCI; neither can publish to GHCR. No app deployment or Environment approval is introduced.

Workflow artifacts are internal and expire after 14 days. Package checksums, CLI archive checksums, OCI archive SHA-256, OCI manifest digests, and registry manifest digests are distinct evidence. Main qualification records internal OCI manifest digests but creates no registry refs; tag publication compares SemVer registry digests with exact tag-built, tag-qualified OCI metadata. GitHub's PR `gate` remains the merge check; protected-main and protected-tag rules remain external trust requirements. Every action is pinned consistently with repository CI policy; any SHA/version/artifact mismatch blocks publication.

### 8. OCI images and Compose deployment

Publish no OCI image tags after a main merge. Only a strict `vX.Y.Z` tag publishes `north-server` and `north-web` SemVer refs to GHCR from freshly built and qualified tag archives. A strict `vX.Y.Z` tag builds and qualifies fresh server/web OCI images from that tag; initial-release eligibility uses the complete currently fetched strict tag-ref set, while subsequent releases require an exact first-parent version bump. SemVer GHCR refs point to tag-built digests and can differ from main SHA-image digests. CLI release archives contain `north` and `north-daemon`, never `north-server`; the server remains inside the backend OCI image. `Dockerfile.server` and `apps/web/Dockerfile` package outputs from `scripts/release.sh package`; they do not compile Rust or rebuild Next independently. The server image contains the packaged `north-server` binary in a non-root, glibc-compatible runtime with CA roots (glibc 2.31 minimum). `north-server migrate` uses the existing SeaORM migration runner; ordinary server startup verifies schema state without applying DDL. The web image contains the existing Next standalone output and runs on Node 22. Both images target Linux x86_64 only for 0.1.0.

`docker-compose.yaml` pulls exact supplied SemVer image refs or digest pins for operators; CI Compose loads qualified OCI archives locally without GHCR access and runs PostgreSQL 16, `north-server`, and `north-web`. On a fresh database, run `docker compose run --rm north-server migrate` before normal `docker compose up`; startup verifies schema state and does not apply DDL. PostgreSQL data lives in a named volume. Compose does not build images on operator machines and does not include a TLS proxy or daemon. The server listens on `0.0.0.0:8080` inside its container; Next listens on `0.0.0.0:3000`. Publish these ports on host loopback for a same-host operator-managed TLS proxy. The proxy routes server-owned paths and `/daemon/ws` to `north-server`, all other paths to Next, preserves cookies and WebSocket upgrade, and streams `/events` without buffering. The host-managed daemon connects to the same public HTTPS origin. No browser API origin is configured at image build time; browser paths remain relative and same-origin.

Compose requires operators to supply `DATABASE_URL`, PostgreSQL credentials, and a stable 64-character hexadecimal `NORTH_OTP_HMAC_KEY`; no secret values are baked into images or committed Compose defaults. The current server reads these values from environment variables, not Docker secret files; file-based secret support is outside this delivery scope. `/healthz` remains a process/liveness signal, not database readiness. The operator runbook documents persistent-volume backup/restore and key preservation.

The main package job builds server/web OCI once from release-package outputs and uploads the package, OCI archives, checksums, image metadata, artifact ID, and upload digest in one 14-day artifact. Preflight and Compose qualification verify and run those exact main OCI archives locally; main workflow has no registry publisher. The tag publisher consumes only its tag-qualified artifact without source checkout or rebuild and records SemVer registry digests. The tag build separately creates package/OCI and CLI archives from the exact tag source, then preflight and qualification verify those bytes. After qualification, the CLI publisher uploads only CLI archives/checksums to a draft GitHub Release; the GHCR publisher consumes the exact tag OCI archives without rebuilding and publishes SemVer images. The Release becomes public only after GHCR succeeds. Release publishers have separate permissions (`contents: write` vs `packages: write`) and no TLS secrets.

**Alternatives rejected:** building images independently in Dockerfiles would duplicate the release build; publishing server/web package tarballs would add a second operator delivery path; putting the proxy or daemon in Compose would pull certificate lifecycle and host repository/agent access into North's container contract. Multi-architecture OCI images and Kubernetes remain deferred; CLI archives are limited to the declared host targets.

### 9. North CLI and daemon supervision

Follow the user-selected Multica command shape without copying its auth model or implementation ([CLI and Agent Daemon Guide](https://github.com/multica-ai/multica/blob/main/CLI_AND_DAEMON.md)). Provide only `north setup --server-url HTTPS_URL [--label LABEL]`, `north daemon start [--foreground]`, `north daemon stop`, and `north daemon status [--output json]`. `north setup` wraps the existing `north-daemon setup` HTTPS device flow and same-origin browser approval, prints/opens the server-provided approval URL, saves the existing 0600 daemon credential state, then starts the sibling daemon. No OAuth/PAT or Requirement commands.

The archive places `north` and `north-daemon` together. Background start redirects logs under `~/.north`, records instance/PID metadata, and survives CLI exit; foreground mode attaches to the process. Stop verifies that the process identity matches the managed daemon before signaling, waits for graceful shutdown, and never kills a stale/reused PID. The daemon atomically writes local connection state and owns an owner-only Unix control socket for status and graceful stop. Every control request carries the daemon instance ID; PID is informational and never signaled. `connected` follows only the existing welcome, reconciliation, and coordination-ready gate, while retryable transport failure clears it before backoff. CLI trusts connection status only when the matching live socket responds; stale socket probes never unlink a potentially live socket. Status includes safe server/device/process/connection facts and never credentials. Profiles and service-manager integration are not part of v1.

### 10. Documentation and evidence ownership

Add `docs/deployment/self-hosted.md` with required PostgreSQL, proxy, web, server, and daemon services; `DATABASE_URL`, `NORTH_OTP_HMAC_KEY`, bind/public URL, and daemon-agent configuration; `openssl rand -hex 32` key generation; migrations/startup; `/healthz` liveness semantics; setup approval; daemon state/journal paths; limitations; upgrades; backups; and checksum/tag verification.

Add `docs/development/release-checklist.md` with maintainer checks for clean main/tag, strict specs, `validate.sh` profiles, PostgreSQL qualification, Playwright/browser transport evidence, internal artifact/version/checksum review, OCI image digests, and post-publication verification. Each check must distinguish pass, not-run, and owner action.

Update testing/CI/invariant/architecture docs only where the shipped executable topology or evidence status changes. Existing deferred statuses remain unchanged.

## Risks / Trade-offs

- **Release packaging crosses crate, web, workflow, and docs boundaries.** → Reuse the existing `north-server`, migration runner, and web/daemon commands; add no alternate business path.
- **Local WSS fixture can drift from production proxy behavior.** → Require persistent operator-certificate verification, SSE streaming, cookie forwarding, and WebSocket upgrade checks in the fixture; keep the documented route table and proxy smoke evidence together. The local harness never generates a CA or changes workstation trust; CI imports only the existing operator CA into its ephemeral hosted runner and removes it during cleanup.
- **Validation state could leak into the secret-bearing job.** → Run preflight without secrets, then qualify on a fresh runner, re-download by immutable artifact ID, and repeat verification before importing the existing CA.
- **Repository-level TLS secrets have broader reach than Environment secrets.** → Require main branch rules that enforce PR review and `gate`, block direct pushes/bypass, reference TLS secrets only in the post-preflight provisioning step, and never reference them in publisher/preflight jobs.
- **LogCodeDelivery is intentionally used to obtain a test OTP.** → Capture only a private test log, assert response bodies/cookies contain no code, and keep production operator documentation clear that delivery remains the configured sink.
- **Proxy configuration can break SSE or WebSocket upgrades.** → Ship route/upgrade requirements, exercise both live SSE and daemon connection in smoke, and include a proxy verification step.
- **CLI archives could be mislabeled or fail on a target host.** → Build and smoke each declared Linux/macOS target separately; keep backend OCI Linux x86_64-only.
- **Main or tag events could bypass trust boundaries or mislabel outputs.** → Require protected-main PR/`gate` and authorized `v*` tag rules; use full-history checkout, compare peeled tag target with `GITHUB_SHA`, require main ancestry and matching Cargo/web tag versions; permit an unchanged first-parent version only when no other strict SemVer ref currently exists in the explicitly fetched tag set; and require current workflow code. Main qualification consumes only main-built artifacts and performs no GHCR publication; tag publication consumes only fresh tag-built/qualified artifacts. Checksums do not prove publisher authenticity; signing/provenance/SBOM stays deferred.
- **Tag-built images or assets can drift from main qualification.** → Rebuild on the strict tag as requested, then qualify those exact artifacts; publish the same artifact bytes, and keep the GitHub Release draft unpublished if GHCR publication fails.
- **GHCR tags can be changed by another package writer between preflight and publication.** → Limit `packages: write` to OCI publishers and restrict release-tag operations to authorized maintainers. Preflight both destinations against tag-qualified digests, verify results, and accept only matching existing tags. GHCR package administrators remain able to mutate tags outside workflow guarantees.

- **Container images could drift from the qualified archive.** → Build Docker images only from the immutable package outputs, carry OCI artifacts through the same artifact ID, and qualify/publish without rebuilding.
- **Repeated or racing tag publishers could replace immutable SemVer refs.** → Serialize by tag, accept an existing ref only when its registry manifest digest matches the tag-qualified artifact, and fail on conflicts.
- **Compose could bypass the supported same-origin boundary or lose operator data.** → Keep the TLS proxy external, bind published app ports to loopback, preserve the named PostgreSQL volume, and qualify SSE/WSS routing plus persistence.

## Migration Plan

1. Keep the consolidated fresh-install baseline; document that pre-release databases require manual recreation, and qualify explicit `north-server migrate` followed by schema-verifying startup.
2. Add fresh-install/startup and assembled browser/daemon evidence; update `validate.sh` and docs only after commands run.
3. Add the minimal daemon-managing CLI and target-specific CLI archives; keep `north-server` in backend OCI.
4. Add package manifest/checksums and qualify exact main artifacts; do not publish OCI images to GHCR from main or manual dispatch.
5. On a strict protected SemVer tag, rebuild package/OCI/CLI from the tagged commit, qualify exact outputs, upload CLI assets to a draft GitHub Release, publish tag-built SemVer OCI images, then publish the Release. Verify archive checksums, internal OCI digests, and SemVer registry digests.
6. For rollback, stop the new server, restore a compatible database backup, and redeploy the prior qualified artifact. Do not reuse pre-release development databases with 0.1.0. After publication, never roll binaries backward across an incompatible applied migration without a tested database restore; rotate the OTP key only deliberately, since outstanding codes must be reissued.

## Open Questions

Resolved: CLI bundles and supervises the daemon; `north-server` remains in backend OCI; CLI release targets are Linux x86_64 and macOS x86_64/ARM64; strict tags rebuild and qualify OCI; CLI assets upload to a draft GitHub Release before GHCR, and the Release publishes only after image publication. Compose still includes PostgreSQL, server, web; operator supplies TLS proxy and GHCR visibility.
