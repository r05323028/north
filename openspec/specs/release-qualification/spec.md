# release-qualification Specification


## Purpose

Defines reproducible release evidence for North 0.1.0. Qualification proves that the existing browser, server, PostgreSQL, daemon protocol/runtime, clarification, readiness, and human-review boundaries compose safely without creating a second product path.

## Requirements

### Requirement: Assembled TLS uses existing operator PKI

The assembled release qualification SHALL read an operator-managed PEM server certificate and unencrypted private key from `NORTH_RELEASE_TLS_CERT_FILE` and `NORTH_RELEASE_TLS_KEY_FILE`, or their documented `NORTH_RELEASE_TLS_DIR` defaults. The workflow SHALL run on pushes to `main` after PR merges; repository branch rules SHALL require PR review and the `gate` check and block direct pushes and bypasses. A secret-free `release-preflight` job SHALL validate the full `GITHUB_SHA`, workflow artifact ID/digest, package manifest/checksums, and OCI metadata/archive digests. Only after preflight succeeds SHALL qualification run on a fresh ephemeral `ubuntu-latest` runner, download the same artifact ID, and repeat those checks before a single TLS-provisioning step references repository Actions secrets `NORTH_RELEASE_TLS_CA_PEM`, `NORTH_RELEASE_TLS_CERT_PEM`, and `NORTH_RELEASE_TLS_KEY_PEM`. These secrets SHALL NOT be referenced by preflight or publisher steps. The workflow SHALL NOT require GitHub Environments. Before installing trust, qualification SHALL validate that the CA is one readable PEM certificate with `CA:TRUE`, and validate the leaf/key match, chain, validity, `basicConstraints`, `digitalSignature` key usage, `serverAuth`, and `localhost`/`127.0.0.1` SAN coverage. A full server chain SHALL be leaf-first. The workflow SHALL install only the supplied CA certificate into the ephemeral runner's OS trust store and isolated Chromium NSS database under qualification's temporary HOME. Temporary-HOME cleanup SHALL remove the NSS database, and unconditional workflow cleanup SHALL remove the system trust entry and TLS files. Local qualification SHALL rely on already-installed platform roots and SHALL NOT modify developer machine or keychain trust. Curl, Chromium, and the daemon SHALL retain normal certificate and hostname verification. Qualification SHALL NOT generate a CA or leaf certificate, inject a test CA, bypass verification, or use `NODE_EXTRA_CA_CERTS`; TLS values SHALL NOT enter logs or artifacts. The leaf private key SHALL be test-only, disposable, and never a production credential because release code executes while this identity is available.

#### Scenario: Failed preflight never reaches TLS secrets

- **WHEN** immutable source, artifact ID, manifest, or checksums fail validation in `release-preflight`
- **THEN** the separate secret-bearing job and publication do not run, and the preflight job has no access to TLS secrets

#### Scenario: Existing PKI qualifies hosted assembled transports

- **WHEN** preflight succeeds and the fresh qualification job re-downloads and revalidates the same artifact ID, then receives the existing CA and a matching test-only localhost leaf certificate/private key through repository Actions secrets
- **THEN** it installs that CA in the ephemeral runner's OS trust store and isolated Chromium NSS database under temporary HOME, the proxy/browser/daemon validate chain and hostname normally, and cleanup removes both trust entries and TLS files without changing any operator workstation or requiring a persistent self-hosted runner

#### Scenario: Missing or invalid identity fails before trust installation

- **WHEN** a CA, certificate, or key repository secret is missing, malformed, mismatched, expired, untrusted by the supplied CA, or missing required SAN/EKU
- **THEN** qualification exits with an actionable owner action before installing runner trust or starting server, web, proxy, or daemon processes and does not create replacement CA material

#### Scenario: Local qualification leaves platform trust unchanged

- **WHEN** a developer runs `scripts/release.sh qualify` locally
- **THEN** it uses the operator-managed identity with existing platform trust or fails with an owner action, and it never changes local trust stores or prompts for trust authorization
- **AND** macOS local qualification requires Node.js 24.21.0 or newer for Node TLS to consume user-Keychain roots; versions below that floor fail before artifact startup with an owner action. Node 22.20.0 failed this trust check; Node 24.21.0 and 26.7.0 passed. This is a local qualification-tool requirement, not a change to the packaged web runtime.
- **AND** on macOS, only the trusted-WSS Cargo test, production-daemon fixture, and assembled Playwright process receive the original `HOME` so Rustls and Chromium can load user-domain trust settings; Playwright scopes `NODE_USE_SYSTEM_CA=1` to its Node TLS clients; XDG config/state, Playwright storage/profile state, and all other child-process homes remain isolated

### Requirement: Assembled golden path is release-qualified

The 0.1.0 release qualification SHALL include one deterministic production-shaped test that starts with an authenticated browser or client, uses the canonical North HTTP APIs, crosses a real server-to-daemon connection using the supported protocol, persists clarification conversation and runtime projections, accepts a readiness assessment only through the server's existing revision/session gates, and completes one authorized human review transition. The test SHALL assert durable responses after canonical reads and SHALL fail when any boundary is bypassed or when a second internal-only implementation path is used. The daemon-side agent decision SHALL be deterministic and local to the qualification environment; qualification SHALL NOT depend on an external model or provider.

#### Scenario: Clarification reaches review through real boundaries

- **WHEN** the qualification client authenticates, creates and discusses a Requirement, submits a clarification message, and the connected daemon reports clarification facts followed by a valid readiness assessment with an explicitly empty blockers list
- **THEN** the server persists the conversation and activity, binds the assessment to the current Requirement revision and run/session, promotes the Requirement to Ready only after its existing gates pass, and serves a review packet from canonical HTTP state

#### Scenario: Human review completes from the canonical packet

- **WHEN** an authorized reviewer reads the current review packet produced by the golden path and submits one supported review decision with its returned assessment and state identities
- **THEN** the server performs the existing guarded lifecycle transition, increments state version exactly once, retains immutable readiness evidence, and the browser/client observes the resulting state through a canonical read

#### Scenario: Invalid or stale daemon evidence cannot complete the path

- **WHEN** the qualification daemon sends an assessment for a different Requirement, an old revision, or a duplicate event
- **THEN** the server rejects or deduplicates it under existing protocol/readiness rules without promoting or double-mutating the Requirement, and the qualification records that negative boundary

### Requirement: Browser SSE remains notification-only in assembled coverage

The release qualification SHALL exercise the browser against an assembled server-backed environment and SHALL prove that authenticated SSE notifications are only identity/category hints. After relevant notifications, duplicate notifications, missed notifications followed by reconnect, browser focus, or visibility recovery, the browser SHALL refetch canonical HTTP state for the affected Requirement/workspace. The browser SHALL never treat SSE payloads as authoritative Requirement, conversation, readiness, activity, run, or review-packet state and SHALL never open a WebSocket.

#### Scenario: Relevant event refetches canonical state

- **WHEN** the assembled server commits a Requirement, conversation, readiness, activity, or run change and the browser receives its SSE notification
- **THEN** the browser performs the applicable canonical HTTP refetch and renders the server response rather than applying business state from the SSE payload

#### Scenario: Reconnect and refocus recover missed state

- **WHEN** the browser misses a notification or the SSE connection disconnects and later reconnects, regains focus, or becomes visible
- **THEN** the workspace retains safe last-known data while refetching canonical state and eventually renders the current server-backed Requirement/workspace without SSE replay

#### Scenario: Duplicate and misleading hints are harmless

- **WHEN** duplicate, delayed, unrelated, malformed, or business-state-shaped SSE payloads arrive
- **THEN** the browser performs at most harmless canonical repair for relevant identity, ignores payload fields as truth, does not duplicate messages or transitions, and does not open a WebSocket

### Requirement: Fresh installation and startup are qualified

A release qualification run SHALL apply the checked-in initial-schema baseline to an empty PostgreSQL database and SHALL verify that the server starts only after migrations and mandatory configuration succeed. It SHALL exercise a production-shaped `DATABASE_URL` and a valid `NORTH_OTP_HMAC_KEY`, and SHALL assert clear safe failure before serving routes when PostgreSQL is unavailable, the key is missing or invalid, or migration application fails. Databases stamped by the unreleased pre-0.1.0 history are unsupported: startup SHALL NOT reset or rewrite them, and the operator documentation SHALL require manual recreation before first-release use. After 0.1.0, applied migrations SHALL remain immutable and new migrations SHALL follow the existing head. Startup and failure output SHALL not disclose the OTP key or other credentials.

#### Scenario: Empty database reaches current migration head

- **WHEN** a fresh PostgreSQL database is given to the built release
- **THEN** the single initial-schema migration applies, the recorded head matches the repository baseline, and a basic authenticated HTTP workflow can start without manual schema edits

#### Scenario: Pre-release database is not reset automatically

- **WHEN** the 0.1.0 server starts against a database stamped by the unreleased migration history
- **THEN** startup fails before accepting traffic without dropping or rewriting database state, and the operator must back up and manually recreate that database before first-release use

#### Scenario: Required configuration is exercised

- **WHEN** the built server starts with a reachable PostgreSQL database and a valid 64-character hexadecimal `NORTH_OTP_HMAC_KEY`
- **THEN** it validates configuration, applies the baseline before accepting traffic, exposes its operational readiness surface, and can serve authenticated requests

#### Scenario: Mandatory startup failures are safe

- **WHEN** PostgreSQL is unavailable, migrations fail, or `NORTH_OTP_HMAC_KEY` is missing or invalid
- **THEN** startup exits non-successfully before serving authenticated routes, identifies the configuration or migration class without secret material, and does not enable an unkeyed OTP path

### Requirement: Built release artifacts are reproducible and smoke-qualified

The main-merge release process SHALL build the server/web package and OCI inputs from the exact clean commit on protected `main`, verify `HEAD == GITHUB_SHA`, and qualify exact package/OCI inputs but SHALL NOT publish images to GHCR. A Git tag SHALL NOT be required for main builds; only strict SemVer tags publish OCI images. A strict `vX.Y.Z` tag SHALL identify a main-reachable commit whose Cargo/web versions equal the tag and SHALL trigger a separate build of fresh server/web OCI inputs plus matching `north` and `north-daemon` CLI archives for Linux x86_64, macOS x86_64, and macOS ARM64. An unchanged first-parent version is permitted only when the full fetched tag set contains no other strict SemVer tag ref beyond the exact event tag; later releases require a first-parent version bump. The tag build SHALL verify Cargo/web versions against the tag, record source/target/version metadata, and upload package, OCI archives, CLI archives, and checksums under one immutable workflow artifact ID with 14-day retention. Secret-free preflight and fresh hosted qualification SHALL download and verify that same artifact. Qualification SHALL smoke the packaged stack, CLI/daemon pair, and exact tag-built Compose images. Main publication SHALL consume the qualified main artifact; tag publication SHALL consume the qualified tag artifact; neither publisher SHALL rebuild after qualification or access TLS secrets. Tag publication SHALL upload only CLI archives/checksums to a draft GitHub Release, publish the qualified SemVer OCI images to GHCR, then make the Release public only after OCI publication succeeds. CLI Release jobs require `contents: write`; OCI publisher requires `packages: write`; permissions SHALL remain separate.

#### Scenario: Tag input remains immutable

- **WHEN** the workflow runs for a push to protected `main` after PR merge
- **THEN** qualification, artifact and internal OCI metadata generation, and checksum generation all identify the same full merge commit SHA and fail if the checked-out revision or embedded versions disagree; main OCI images remain internal and are not published to GHCR

#### Scenario: Manual dispatch uses immutable source

- **WHEN** maintainers manually dispatch qualification with a full `source_sha` for a commit already on protected `main`
- **THEN** every job checks out that exact SHA, verifies `HEAD` and package manifest match it, and GitHub Release/GHCR publication remain unavailable because manual dispatch is qualification-only

#### Scenario: Packaged stack passes smoke

- **WHEN** maintainers run the named 0.1.0 smoke command on the built artifacts with PostgreSQL and required configuration
- **THEN** the smoke runs `north-server migrate` explicitly before normal startup, the server verifies the current migration head and serves authenticated HTTP without applying DDL during startup, the web artifact reaches canonical state through the supported same-origin deployment path, SSE streams without buffering, the daemon/runtime boundary connects through the supported authenticated WSS path, and the smoke run records a deterministic qualification result

#### Scenario: Same-origin proxy preserves both live transports

- **WHEN** the extracted release is served through the documented TLS/reverse-proxy route table
- **THEN** browser document and Next RSC/prefetch requests for `/requirements/{id}` reach Next.js, API JSON requests, mutations, and API subpaths reach `north-server`, cookies and SSE streaming remain intact, the daemon WSS handshake reaches the server with certificate verification enabled, and no browser WebSocket is opened

#### Scenario: Failed qualification blocks publication

- **WHEN** any migration, startup, authenticated workflow, browser transport, daemon boundary, artifact checksum, or version check fails
- **THEN** no release is published and the failing evidence identifies its command and artifact revision

### Requirement: OCI images are qualified and published from immutable release inputs

The release SHALL publish Linux x86_64 `north-server` and `north-web` OCI images to GHCR. `north-server` SHALL contain the backend server binary; the local CLI bundle SHALL NOT contain or replace that server image. Dockerfiles SHALL package outputs from the immutable package build. Each protected-main merge SHALL qualify one immutable source-SHA package/OCI artifact and SHALL NOT publish any OCI image to GHCR. Main and manual qualification SHALL have no GHCR publisher permission; internal OCI reference names SHALL never be pushed.

A strict `vX.Y.Z` tag SHALL run a tag-build path, not a SHA-image promotion. It SHALL peel annotated or lightweight tags to a commit and require that commit to equal `GITHUB_SHA`, be reachable from `origin/main`, and contain current `.github/workflows/release.yml` and tag-publisher code matching `origin/main`. Cargo and web package versions SHALL equal the tag. Tag qualification SHALL explicitly fetch all tag refs before version validation. The current event tag SHALL be excluded only by its exact ref name. If no other strict `vX.Y.Z` tag ref currently exists, validation SHALL emit the exact `initial_release=true`; this permits a matching tag whose `version_changed` metadata is false because first-parent and tagged versions are equal. If any other strict tag ref exists—reachable or not—the initial exception SHALL be false, and a release with an unchanged first-parent version SHALL fail. This check describes currently fetched refs and SHALL NOT claim to detect previously deleted tags. Missing or malformed validation output SHALL fail closed. The OCI publisher SHALL accept `version_changed=false` only when the same workflow run's validated `initial_release` output is true. Authorized maintainers SHALL control creation/update/deletion of `v*` tags.

The tag path SHALL build fresh package/OCI inputs and CLI archives from the tagged commit, then preflight and qualify those exact outputs before publication. CLI archives SHALL contain matching `north` and `north-daemon` binaries for Linux x86_64, macOS x86_64, or macOS ARM64, plus version/target metadata and SHA-256 checksums. The CLI publisher SHALL upload only these assets to a draft GitHub Release before the GHCR job. The GHCR publisher SHALL preflight both `vX.Y.Z` image destinations, publish the exact tag-qualified OCI archives without rebuilding, verify destination digests, and accept only already-matching tags idempotently; conflicting tags fail without overwrite. The Release SHALL become public only after both SemVer images publish successfully. A failed GHCR publish leaves the Release draft unpublished.

Main and tag build/preflight/qualification paths SHALL carry exact package/OCI inputs under one immutable Actions artifact per run with 14-day retention and verify artifact ID/digest, source SHA, versions, checksums, and OCI metadata. Compose qualification SHALL load the exact archives locally. Main artifact OCI digests are internal qualification evidence only; no main publisher or GHCR tag exists. The tag publisher SHALL consume only its tag-qualified artifact without checkout or rebuild and record tag-built SemVer registry digests. OCI publisher SHALL receive `packages: write` only; GitHub Release draft/finalization jobs SHALL receive `contents: write` only. No publisher receives TLS secrets. Manual dispatch SHALL remain qualification-only. Operators SHALL pin registry digests for byte identity.

#### Scenario: Main qualification does not publish OCI images

- **WHEN** a protected-main merge passes CI, preflight, and assembled qualification
- **THEN** the workflow retains the verified OCI archives internally and publishes no image or registry tag to GHCR

#### Scenario: Initial strict tag may reuse current version

- **WHEN** an authorized maintainer pushes a strict `vX.Y.Z` tag whose Cargo/web versions match the tag and the full fetched tag set contains no other strict SemVer tag ref after excluding only the exact event tag name
- **THEN** validation emits `initial_release=true` and the same workflow run may publish qualified OCI metadata with `version_changed=false` when first-parent and tag versions match
- **AND** another strict tag ref counts even when it points at the release commit or another branch; deleted historical tags are not observable by this check

#### Scenario: Subsequent strict tag builds and publishes a complete release

- **WHEN** an authorized maintainer pushes `vX.Y.Z` at a main-reachable commit whose matching first-parent version differs from the tag version
- **THEN** validation emits a strict boolean initial-release result, and the tag path builds and qualifies fresh package/OCI inputs and native CLI archive pairs from that SHA, uploads CLI assets to a draft GitHub Release, publishes the tag-built OCI images to SemVer refs, then publishes the Release

#### Scenario: Existing SemVer ref blocks unchanged-version exception

- **WHEN** any other strict `vX.Y.Z` tag ref exists in the fetched repository and the current tag version equals the first-parent version
- **THEN** validation sets `initial_release=false` and rejects the release before publication, including when the other ref points to another branch or the same commit

#### Scenario: Missing or malformed initial-release output fails closed

- **WHEN** tag inventory cannot be read, the same-run `initial_release` output is missing or not exactly `true` or `false`, or the OCI publisher receives no valid initial-release flag
- **THEN** tag publication exits non-successfully before any GHCR write and the GitHub Release remains a draft

#### Scenario: Draft Release stays private until both images publish

- **WHEN** CLI archive upload succeeds but either SemVer OCI publication fails
- **THEN** the GitHub Release remains a draft and its CLI assets are not public
- **WHEN** both tag-built OCI images publish with verified digests
- **THEN** the finalizer publishes the draft Release

#### Scenario: Invalid tag or unavailable artifact fails closed

- **WHEN** a tag is not strict SemVer, does not resolve to its event commit, is not reachable from main, lacks a first-parent version bump when `initial_release=false`, has missing or malformed initial-release output, mismatched Cargo/CLI/web versions, stale release code, or a required package, OCI, or CLI artifact is missing or invalid
- **THEN** no SemVer image is published and no CLI Release becomes public

#### Scenario: Conflicting SemVer destination blocks both image copies

- **WHEN** either existing SemVer destination has a different manifest digest from its tag-qualified OCI archive
- **THEN** the publisher fails before copying either image, overwrites neither destination, and leaves the GitHub Release as a draft

#### Scenario: Matching SemVer destinations are idempotent

- **WHEN** the tag publisher is retried and existing SemVer tags already reference the corresponding tag-qualified manifest digests
- **THEN** it succeeds without replacing either tag

#### Scenario: Compose qualification fails closed

- **WHEN** either image fails to start or Compose qualification detects failed authenticated HTTP, SSE, daemon WSS, or persistence checks
- **THEN** main has no GHCR publisher; tag qualification failure publishes no SemVer image, and no CLI Release becomes public

#### Scenario: Image publication never rebuilds

- **WHEN** a strict tag publication follows successful qualification
- **THEN** the publisher downloads and verifies that tag run's qualified OCI artifact without source checkout or rebuild, publishes only its intended SemVer tags, records registry digests, and fails on conflicting existing tags. Main and manual qualification never publish OCI images to GHCR.

### Requirement: Compose deployment preserves North's supported topology

The provided `docker-compose.yaml` SHALL run PostgreSQL 16 with a named persistent data volume, plus the `north-server` and `north-web` release images. On a fresh database, release qualification and operator instructions SHALL run `north-server migrate` explicitly before normal server startup; normal startup SHALL verify current schema state without applying DDL. Compose SHALL use versioned images rather than building on operator machines, and SHALL NOT include the daemon or terminate/manage TLS. The server and web processes SHALL bind their container interfaces; published host ports SHALL default to loopback for the operator-managed same-host TLS proxy. The proxy SHALL route server-owned HTTP paths, SSE, and `/daemon/ws` to `north-server` and remaining paths to Next, preserving cookies, SSE streaming, and WebSocket upgrade. The daemon SHALL remain host-managed and connect outbound to the same public HTTPS origin. Compose SHALL require externally supplied database credentials and a stable valid `NORTH_OTP_HMAC_KEY`, without embedding secret values. `/healthz` SHALL be documented only as process/liveness, not database readiness.

#### Scenario: Compose starts supported services with persistent storage

- **WHEN** an operator supplies valid runtime configuration and starts the documented Compose file
- **THEN** PostgreSQL, server, and web use the documented versioned images; the operator or qualification command runs `north-server migrate` before normal startup verifies schema and accepts traffic; and PostgreSQL state survives container recreation while its named volume is retained

#### Scenario: External proxy preserves browser and daemon routes

- **WHEN** browser and host-managed daemon use the documented public origin
- **THEN** browser HTTP and notification-only SSE reach the correct services, daemon WSS reaches `north-server` with normal certificate verification, and browser opens no WebSocket

#### Scenario: Compose configuration does not bake in secrets

- **WHEN** required database or OTP configuration is missing or invalid
- **THEN** startup fails closed without substituting a default credential, and no secret value is present in the Compose file or image layers

### Requirement: Operator and maintainer release evidence is complete

The 0.1.0 release SHALL ship documentation that names the minimum supported services, required environment variables, strict OTP key generation and preservation rules, database setup and migration behavior, server/web/daemon startup and connection steps, known intentional limitations, upgrade expectations, release verification, and a concise maintainer checklist. Documentation SHALL distinguish executed qualification evidence from prerequisites or owner actions and SHALL not claim deferred or environment-dependent checks passed when they did not run.

#### Scenario: Operator can verify a self-hosted release

- **WHEN** an operator follows the 0.1.0 setup and verification runbook with the published CLI assets and OCI images
- **THEN** the operator can install `north` plus its bundled daemon, enroll and inspect the daemon connection, identify required services/configuration, start the backend/web stack, complete a basic authenticated workflow, and verify CLI checksums, image digests, and migration evidence

#### Scenario: Deferred architecture remains explicit

- **WHEN** maintainers review 0.1.0 release documentation and qualification output
- **THEN** multi-server ownership epochs, automatic live migration, lost setup-claim recovery, dedicated setup cleanup scheduling, per-Requirement ACL, object storage, and external message broker remain named as outside 0.1.0 unless a concrete safe-release blocker is recorded
