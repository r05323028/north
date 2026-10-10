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
- [x] `PASS` Active `main` ruleset `21581438` requires one approving review,
  `Rust (fmt, clippy, unit+architecture)`, `PR title (Conventional Commit)`, and
  `merge gate`; squash-only and linear history remain enforced, with no bypass actors.
  Status-check freshness remains an owner action (`strict_required_status_checks_policy=false`).
- [ ] `OWNER ACTION` Protect `v*` tags separately; restrict creation, update, and
  deletion to authorized release maintainers.
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

Workspace validation below ran on the macOS worktree. Local release package,
OCI, and CLI receipts use clean synthetic source SHA
`d09dcbfa74993304bfbf8867b1143adac576534f` (parent
`bcdaf19447001803d018ef699f36efade3546398`), not a protected-main or tag
commit. These synthetic receipts are local implementation evidence only.
PostgreSQL checks used disposable PostgreSQL 16; no DSN or credentials are
recorded.

- `PASS` `./scripts/validate.sh fast`
- `PASS` `openspec validate --all --strict`
- `PASS` `./scripts/validate.sh integration`
- `PASS` `./scripts/validate.sh e2e` (8 tests)
- `PASS` `node --test tests/release/*.test.mjs` (48 tests) — release validation, pinned GLIBC checks, locked Cargo/source-cleanliness guards, PR-title event routing, CLI archives, recursive Git tag identity, pre-mutation SHA checks, missing-release error classification, digest conflicts, draft retries, workflow ordering, and hosted PKI cleanup passed.
- `PASS` Local `./scripts/release.sh qualify` on Node.js 26.7.0 with disposable PostgreSQL 16, ephemeral OTP key, and existing trusted TLS identity: six fresh-install tests, trusted-WSS runtime, and two assembled Playwright E2E tests passed. That earlier run did not cover immutable package/OCI artifacts.
- `PASS` Exact-source Linux package/OCI preflight from clean synthetic SHA `d09dcbfa74993304bfbf8867b1143adac576534f`: package manifest version `0.1.0`, target `x86_64-unknown-linux-gnu`, GLIBC baseline `2.31`, `version_changed=false`; 60 MB/1,353 files, with `north-server` and `north-daemon` verified as Linux x86_64 ELF. Package verifier and checksums passed. Package-produced OCI archives target `linux/amd64`; `verify-release-images.mjs` and archive checksums passed. Internal manifest digests: `north-server` `sha256:426e9cf44495282558d43281c59895e203f54d8240bcb9551e22b9f0a81b31a4` (archive SHA-256 `5e1e69800b95d7135c01aa3ef2f4b2c89fcefd2be197777a90bd4ad22d1e5d64`); `north-web` `sha256:b70e5aa257effc4e2c6a114f093a8967b819d5cc65fb8e7c7d9fe1180384dcbe` (archive SHA-256 `f42df092c1140cb88d94ca85335d3ac17d62d4e900b64548ce7f9add169462d0`). These are local OCI archive digests, not SemVer registry digests. Artifacts are under `$HOME/.cache/north-local-emulation.current.5NugBg/artifacts/final`; nothing was published. Expected tag destinations remain `ghcr.io/r05323028/north-server:vX.Y.Z` and `ghcr.io/r05323028/north-web:vX.Y.Z`; the unchanged-version gate still requires the full fetched tag inventory to contain no other strict SemVer ref, which remains unverified on a hosted workflow.
- `PASS` `./scripts/validate.sh smoke` on the exact package/OCI inputs ran in a disposable Linux x86_64 container with PostgreSQL 16, operator-managed TLS, and CA/NSS trust confined to that container; exit 0 with 22 release PASS markers, including packaged fresh-install startup migration, optional CLI and migration-failure coverage, invalid-OTP redaction, Compose image startup migration, same-origin TLS proxy, authenticated owner session, trusted daemon WSS, assembled browser SSE golden path, PostgreSQL volume persistence, and cleanup. Docker Desktop was arm64 with Linux/amd64 emulation. No registry push or GitHub Release publication occurred.
- `PASS` Package-backed Buildx `--load` builds of `north-server:qualification` and `north-web:qualification`: both `linux/amd64`, version `0.1.0`, source label `d09dcbfa74993304bfbf8867b1143adac576534f`, source URL `https://github.com/r05323028/north`. Server runtime user `65534:65534`; web runtime user `node`; server CA bundle present; runtime GLIBC `2.36`. Images remained local; no `--push`.
- `PASS` All three `north` + `north-daemon` CLI archives were built from the same clean synthetic source SHA `d09dcbfa74993304bfbf8867b1143adac576534f`; archive metadata/checksums passed and each extracted pair executed on its target OS. SHA-256: Linux x86_64 `0bf4836b1e2dbc2c430bcd25131e1a191b822f76273059ddb302e5b310335eb7`; macOS x86_64 `54fd18d4c4e4346e8c2c6001dbf855b8d009fbe173020fe66465f38921d32c9b`; macOS ARM64 `6d66c59183e8b549296f9ed5673356eb33ecc820a9c983bdd61ce3dd6b9caf3b`. Linux used the pinned x86_64 Rust builder; macOS ARM64 ran natively; macOS x86_64 ran under Rosetta on Apple Silicon, not physical Intel hardware. Archives are under `$HOME/.cache/north-local-emulation.current.5NugBg/artifacts/final/cli`; no archive was published.
- `PASS` package preflight probes reject dirty source and clean untagged HEAD before creating output.
- `PASS` qualification preflight labels missing database input `OWNER-ACTION` and absent package/OCI lanes `NOT-RUN`.
- `PASS` `actionlint .github/workflows/ci.yml .github/workflows/pr-title.yml .github/workflows/release.yml`
- `PASS` `docker compose --file docker-compose.yaml config --quiet` with dummy interpolation values; this config check builds no images.
- `PASS` Earlier `./scripts/pre-push-validation.sh` before the final hosted-cleanup-only workflow/test change: disposable PostgreSQL 16, native `ci`, and Act Rust job passed.
- `BLOCKED` Earlier 30-minute pre-push attempt (superseded): native `validate.sh ci` passed; Act Rust job timed out while fetching crates during `cargo check --workspace --all-targets`.
- `PASS` `./scripts/pre-push-validation.sh`: disposable PostgreSQL native `ci` and Act `Rust (fmt, clippy, unit+architecture)` passed; Act completed `scripts/validate.sh rust: OK`. Release-only edits made while Act ran were covered by the 48-test release suite.
- `PASS` Fresh-install migration qualification is scoped to 0.1.0: migration tests reject old SQLx history and partial schemas without mutation; the exact d09 packaged server auto-migrated a fresh schema before listening, and the DDL-privilege failure path exited before listening. The Compose image also auto-migrated before traffic and retained PostgreSQL volume state. Deployment docs prohibit automatic reset and require manual backup/disposition. Hosted protected-main/tag migration evidence remains NOT RUN.
- `PASS` Smoke fail-closed guard: `./scripts/validate.sh smoke` without an artifact exits 2 with `validate.sh: smoke requires NORTH_RELEASE_ARTIFACT_DIR.` This is not artifact qualification.
- `PASS` Exact-source local Linux/amd64 `./scripts/validate.sh smoke` is recorded above for synthetic SHA `d09dcbfa74993304bfbf8867b1143adac576534f`; hosted protected-main/tag qualification remains `NOT RUN`.
- `PASS` Artifact verifiers confirmed package/OCI manifests, source labels, archive checksums, Linux x86_64 ELF target, and matching CLI/daemon version+target for the exact synthetic source SHA above. No GHCR or GitHub Release destination was accessed.
- `NOT RUN` Hosted main/tag package builds, GitHub Release asset upload, GHCR publication, and permission/registry rehearsal; local emulation does not prove workflow execution on protected-main `GITHUB_SHA` or authorize publishing.

Qualification prerequisites:

- Verify active `main` ruleset requires one approving review and the existing
  Rust, PR-title, and `merge gate` contexts. Confirm ruleset state using the explicit
  `gh api` readback in `docs/development/ci.md`; workflow YAML cannot enforce it.
- Build Linux server and CLI/daemon binaries in the pinned glibc 2.31 builder;
  verify all packaged Linux ELF GLIBC requirements are at most 2.31 and manifest
  metadata matches. Release Cargo commands use `--locked`; package steps verify
  source `HEAD` and cleanliness after build/assembly.
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
