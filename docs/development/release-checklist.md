# North release checklist

Use this checklist for each protected-main qualification and each strict
Git-tagged SemVer release. Main merges qualify package/OCI archives but never
publish OCI images to GHCR. A `vX.Y.Z` tag starts a fresh build and qualification
of package/OCI inputs plus native `north`/`north-daemon` CLI pairs from tag SHA.
CLI assets upload to a draft GitHub Release before tag-built SemVer images publish;
Release becomes public only after both image digests verify. Qualification is
source-SHA-specific and must be rerun for every release commit.
Record source SHA, artifact ID/digest, command, timestamp, result, and CI run URL.
Never carry results across source SHAs. `PASS` means command ran and assertions
passed; `NOT RUN` is not evidence; `OWNER ACTION` needs a human or external
service.

## Source and specs

- [ ] `OWNER ACTION` Confirm clean merged-main source: `git status --short` is
  empty and `HEAD` equals full CI `GITHUB_SHA`.
- [ ] `OWNER ACTION` Confirm manual dispatch supplies a full `source_sha` already
  reachable from protected `main`; manual dispatch qualifies only and never
  publishes images.
- [ ] `OWNER ACTION` Ensure release workflow is present on default branch before
  first manual qualification dispatch. Protect `v*` tag creation, updates, and
  deletion with a ruleset limited to authorized release maintainers.
- [ ] `PASS` Confirm Cargo/web package versions match release manifest; confirm
`version_changed` compares current version with first parent. Initial tag validation
sets `initial_release=true` only when the full fetched tag set has no other strict
SemVer ref after excluding the exact event-tag name. Later tags require a bump.
This checks current refs; deleted historical tags cannot be detected.
- [ ] `PASS` Run `openspec validate --all --strict`.
- [ ] `OWNER ACTION` Confirm affected canonical OpenSpec specs match shipped
  behavior; sync implemented deltas without replacing existing contracts.
- [ ] `OWNER ACTION` Review OpenSpec tasks against executed evidence; do not
  check integration, E2E, or smoke items from implementation alone.

## Validation gates

Run from repository root and paste exact output or CI run URL:

- [ ] `OWNER ACTION` Run `./scripts/validate.sh fast` for this release candidate.
- [ ] `OWNER ACTION` Protect `main` with PR review and required `gate`; prohibit
  direct pushes and bypasses. This repository rule—not a GitHub Environment—
  ensures release workflow runs only after approved merges. Protect `v*` tags
  separately; restrict creation/update/deletion to authorized release maintainers.
- [ ] `OWNER ACTION` Configure test-only existing-PKI material as repository
  Actions secrets. Workflow needs no GitHub Environments and does not deploy North.
- [ ] `OWNER ACTION` Run `./scripts/validate.sh integration` with isolated
  PostgreSQL (`NORTH_TEST_DATABASE_URL`).
- [ ] `OWNER ACTION` `./scripts/validate.sh e2e` for normal browser suite.
- [ ] `OWNER ACTION` `cargo test -p north-transport-integration --test release_qualification -- --ignored`
  against live server/proxy and trusted WSS.
- [ ] `OWNER ACTION` `./scripts/validate.sh smoke` against extracted package.
- [ ] `OWNER ACTION` Run `./scripts/pre-push-validation.sh` with disposable
  PostgreSQL; record whether local `act` parity ran or was unsupported.

### Current implementation snapshot (not release-candidate evidence)

These checks ran on a dirty macOS worktree. Repeat release-specific checks
against the exact protected-main SHA and subsequent Git tag; none satisfies the gates above.
PostgreSQL checks used a disposable PostgreSQL 16 container; no DSN or
credentials are recorded.

- `PASS` `./scripts/validate.sh fast`
- `PASS` `openspec validate --all --strict`
- `PASS` `./scripts/validate.sh integration`
- `PASS` `./scripts/validate.sh e2e` (8 tests)
- `PASS` `node --test tests/release/*.test.mjs` (37 tests) — release validation, CLI archives, tag checks, digest conflicts, draft retries, workflow ordering, tag-ref concurrency, and hosted PKI cleanup passed.
- `PASS` Local `./scripts/release.sh qualify` on Node.js 26.7.0 with disposable PostgreSQL 16, ephemeral OTP key, and existing trusted TLS identity: six fresh-install tests, trusted-WSS runtime, and two assembled Playwright E2E tests passed. That earlier run did not cover immutable package/OCI artifacts.
- `PASS` Local Docker Desktop `linux/amd64` emulation of exact package, CLI, and OCI artifacts from clean synthetic source SHA `39d81fa36495c201ed0517ac9b26c8789bc254c9`: secret-free preflight and `./scripts/validate.sh smoke` passed, including migration-first Compose startup, authenticated HTTP, proxy/SSE, trusted WSS, daemon runtime, both assembled Playwright tests, volume persistence, and cleanup. Manifest: version `0.1.0`, previous version `0.1.0`, `version_changed=false`; target `linux/amd64`. Internal OCI manifest digests: `north-server` `sha256:746b39310c03fed46c4e26d31b8f0e536d6e476881a1d21196e424e774062e5b`; `north-web` `sha256:51173839db642a6a5f63bfb0958732af8caccb7f86532979b66f39f7836109ff`. Archive SHA-256: server `8f4b68635ab09bae27d4925e5dcdf8bbb365d69ae68e3edd2703bc5167207042`; web `c5ae1a791c96e4fa688b1ffc74696c6eca803ab8334bc5a97e9491cec9810ba6`. Artifacts: `$HOME/.cache/north-local-emulation.SLTosd/artifacts` (197 MB). These are local artifact digests, not SemVer registry digests. Expected tag destinations use `ghcr.io/r05323028/north-server:vX.Y.Z` and `ghcr.io/r05323028/north-web:vX.Y.Z`; no SemVer refs/digests or GitHub Release assets were published. With the updated gate, this unchanged-version artifact can qualify only when the full fetched tag inventory contains no other strict SemVer ref; hosted tag inventory remains unverified. Docker Desktop host was arm64; local-only Skopeo 1.22.3 shim and temporary-home NSS CA trust were used. Hosted protected-main/tag qualification remains NOT RUN.
- `PASS` Native macOS ARM64 CLI archive smoke from synthetic source SHA `af7d03dd355b61e94a0d87217547d9cecb1e646e`: `scripts/release.sh cli-package aarch64-apple-darwin` verified and executed both extracted binaries; archive SHA-256 `2562f60eb584b7aaa7e5c5408f58c5e2c2e3d802301d02a07e1fade1828874dc`. This source differs from Linux/amd64 artifact and does not qualify hosted tag build or macOS x86_64.
- `PASS` macOS x86_64 CLI archive build and Rosetta smoke from synthetic clean-worktree SHA `744136b37e40392483bd07c7a5daf9b0fb52c963`: `scripts/release.sh cli-package x86_64-apple-darwin` verified archive contents/checksums and executed extracted `north` and `north-daemon`; `file` confirmed both Mach-O x86_64. Archive SHA-256 `10159f0e8b2f431e75c7612de820f1cca5f7916874d14951235f3fb4ba392d35`; artifact `$HOME/.cache/north-x86-cli-artifacts.oBPs6T`. Build ran on Apple Silicon under Rosetta, not native Intel or hosted tag workflow; synthetic SHA is not protected-main/tag evidence.
- `PASS` package preflight probes reject dirty source and clean untagged HEAD before creating output.
- `PASS` qualification preflight labels missing database input `OWNER-ACTION` and absent package/OCI lanes `NOT-RUN`.
- `PASS` `actionlint .github/workflows/release.yml`
- `PASS` `docker compose --file docker-compose.yaml config --quiet` with dummy interpolation values; no images built.
- `PASS` Earlier `./scripts/pre-push-validation.sh` before the final hosted-cleanup-only workflow/test change: disposable PostgreSQL 16, native `ci`, and Act Rust job passed.
- `BLOCKED` Earlier 30-minute pre-push attempt (superseded): native `validate.sh ci` passed; Act Rust job timed out while fetching crates during `cargo check --workspace --all-targets`.
- `PASS` Final `./scripts/pre-push-validation.sh` rerun with `NORTH_PRE_PUSH_TIMEOUT=3600`: disposable PostgreSQL native `ci` passed, and Act `Rust (fmt, clippy, unit+architecture)` completed `scripts/validate.sh rust: OK`; workflow job succeeded.
- Migration compatibility is fresh-install-only for 0.1.0: tests reject old SQLx history and partial schemas without mutation; qualification runs packaged `north-server migrate` before startup, while normal startup verifies schema without DDL. Deployment docs prohibit automatic reset and require manual backup/disposition.
- `PASS` Smoke fail-closed guard: `./scripts/validate.sh smoke` without an artifact exits 2 with `validate.sh: smoke requires NORTH_RELEASE_ARTIFACT_DIR.` This is not artifact qualification.
- `PASS` Local emulated `./scripts/validate.sh smoke` against extracted package/OCI artifacts; see synthetic-SHA evidence above. Hosted main/tag qualification remains `NOT RUN`.
- `PASS` Local preflight verified package manifest/source SHA/checksums, Linux x86_64 ELF binaries, matching CLI/daemon version+target, OCI archive checksums, metadata, platforms, and manifest digests. Source was a clean synthetic snapshot, not protected main.
- `NOT RUN` Hosted main/tag package builds, GitHub Release asset upload, GHCR publication, and permission/registry rehearsal; local emulation does not prove workflow execution on protected-main `GITHUB_SHA` or authorize publishing.

Qualification prerequisites:

- Protect `main` with repository rules requiring PR review and the `gate` job;
  block direct pushes and bypasses. Workflow YAML cannot enforce these settings.
- Configure test-only existing-PKI material as repository Actions secrets:
  `NORTH_RELEASE_TLS_CA_PEM`, `NORTH_RELEASE_TLS_CERT_PEM`, and
  `NORTH_RELEASE_TLS_KEY_PEM`. No GitHub Environments are required; never use
  production TLS credentials. Only qualification job references these secrets.
- GitHub-hosted `ubuntu-latest`; no self-hosted runner;
- Hosted qualification installs `libnss3-tools` and imports the supplied CA into
  ephemeral OS trust plus the isolated Chromium NSS database under temporary
  `HOME`; qualification cleanup removes the NSS database with that `HOME`, and
  workflow cleanup removes system trust. Local qualification leaves this opt-in
  disabled; local Docker emulation may enable `NORTH_RELEASE_NSS_CA_TRUST=true`
  only inside its disposable runner.
- CA secret is one existing `CA:TRUE` certificate with no private key; test
  leaf chain and unencrypted key match, leaf has `serverAuth`, and SAN covers
  `localhost` and `127.0.0.1`;
- isolated PostgreSQL database, never developer default;
- 0.1.0 `LogCodeDelivery` is the configured OTP sink; verify server/container
  log access, retention, backups, and forwarding are restricted before exposing
  login to users;
- for local qualification, operator-managed certificate/key and issuer CA
  already trusted by the local host;
- on macOS, local qualification requires Node.js 24.21.0 or newer; 24.21.0 and
  26.7.0 passed, while 22.20.0 failed user-Keychain TLS validation. This is a
  qualification-tool floor only, not packaged runtime support;
- `NORTH_OTP_HMAC_KEY` supplied out of band;
- no `NODE_EXTRA_CA_CERTS`, certificate bypass, temporary CA, or local
  platform keychain mutation. Hosted job imports the existing CA only into
  ephemeral trust and removes it in unconditional cleanup. Do not mark release
  qualification/publication passed until the required secrets and branch rules
  are verified.

## Artifact and publication

- [ ] `PASS` Build once with `NORTH_RELEASE_SOURCE_SHA="$SOURCE_SHA"
  NORTH_RELEASE_OUTPUT_DIR="$RELEASE_DIR/package" ./scripts/release.sh package`
  from exact clean merged-main SHA; package build takes no tag argument and
  does not create a SemVer alias.
- [ ] `PASS` Inspect `manifest.json`: version, source commit, first-parent
  version-change flag, target, glibc baseline, Node major, server/daemon/web versions.
- [ ] `PASS` Inspect package: executable `bin/north-server`, executable
  `bin/north-daemon`, `web/server.js`, `.next/static`, compiled SeaORM
  migrator in `bin/north-server`, operator docs, and release docs.
- [ ] `PASS` Verify package and OCI checksum files from the retained release
  artifact before qualification or publication.
- [ ] `OWNER ACTION` Confirm protected-main qualification loads exact OCI
  archives locally and makes no GHCR writes.
- [ ] `OWNER ACTION` Confirm main and manual workflows have no GHCR publisher
  or `packages: write`; only strict SemVer tag workflow publishes OCI images.
- [ ] `OWNER ACTION` Confirm tag matrix builds and natively smoke-tests the three
  CLI targets, verifies archive metadata/checksums, then uploads six assets
  (three archives plus SHA-256 sidecars) to a draft Release.
- [ ] `PASS` Verify `oci/images.json` source, version, platform, manifest digests,
  and both OCI archive checksums before qualification.
- [ ] `PASS` Verify the containerized server rejects malformed OTP keys with a
  safe diagnostic that omits the key and database password.
- [ ] `PASS` Qualify exact OCI archives with Compose; verify named-volume
  persistence and the external proxy/host-daemon boundary.
- [ ] `PASS` Compare tag GHCR SemVer digests with `images.json`; main OCI
  digests remain internal qualification evidence only.
- [ ] `OWNER ACTION` Push an authorized strict SemVer tag to a main-reachable
  commit. First tag requires no other strict SemVer ref in the fetched tag set;
  subsequent tags require a first-parent version bump. Confirm validation checks
  peeled target, Cargo/CLI/web versions, and current release code.
- [ ] `OWNER ACTION` Confirm `create-cli-release-draft` uploads checksummed CLI
  assets without clobbering, `publish-semver-images` publishes exact tag-built
  OCI archives only after qualification, and finalization depends on both images.
- [ ] `PASS` Compare SemVer registry digests with tag-built OCI manifest digests
  and record separately from main SHA digests (they may differ); confirm matching
  retries are idempotent and conflicting destinations abort before either copy.
- [ ] `OWNER ACTION` Pin deployed images as
  `ghcr.io/r05323028/north-server@sha256:<digest>` and
  `ghcr.io/r05323028/north-web@sha256:<digest>`; confirm GHCR visibility.
- [ ] `OWNER ACTION` Confirm signing, provenance/SLSA attestations, and SBOM
  publication status for this release; document any omissions.

## Post-publication (optional operator checks; not workflow deployment)

The workflow publishes qualified GHCR images only. Run these checks only if an
operator chooses to deploy them:

- [ ] `OWNER ACTION` Verify `/healthz` through the operator-managed route.
- [ ] `OWNER ACTION` Sign in with fresh OTP; confirm browser API and `/events`
  stream use same origin.
- [ ] `OWNER ACTION` Approve one daemon and verify connected status, command
  delivery, event ACKs, readiness assessment, and human review transition.
- [ ] `OWNER ACTION` Verify backup and restore procedure against current schema.
- [ ] `OWNER ACTION` Record workflow run URL, Actions artifact ID/digest, source
  SHA, GHCR image digests, SeaORM database migration head, and owner-action exceptions.
