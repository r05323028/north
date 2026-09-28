# North release harness

`tests/release/` owns repository-level qualification orchestration. It is not a
production crate and must not become a second application implementation.

## Harness contract

- `common.sh` creates isolated `HOME`, `XDG_CONFIG_HOME`, and
  `XDG_STATE_HOME` directories and cleans child processes/temp files on every
  exit path. On macOS, only the trusted-WSS Cargo test, production-daemon
  fixture, and assembled Playwright process receive original `HOME` so native
  TLS clients can read existing user trust settings; Playwright scopes
  `NODE_USE_SYSTEM_CA=1` to Node TLS clients. Local macOS qualification requires
  Node.js 24.21.0 or newer; 24.21.0 and 26.7.0 passed, while 22.20.0 failed
  user-Keychain TLS validation. This tool floor does not change packaged runtime
  support. XDG paths and Playwright storage stay isolated; every other child
  keeps temporary `HOME`. Hosted qualification can set
  `NORTH_RELEASE_NSS_CA_TRUST=true`; `common.sh` then imports only the public CA
  into Chromium's NSS database under that temporary home. `release_cleanup`
  removes the database with the temporary home.
- Harness commands must use bounded waits, return non-zero on any failed
  assertion, and print exact command/evidence names.
- `NORTH_PI_AGENT_COMMAND` points at the checked-in deterministic fixture; no
  external model, Pi provider, or network service is allowed.
- PostgreSQL is supplied explicitly through `NORTH_TEST_DATABASE_URL` or the
  release command's documented artifact environment. Never use a developer's
  default database implicitly.
- The assembled browser path uses live server responses through the local
  same-origin TLS proxy. Its loopback-only test controls sever and resume active
  SSE transport without synthesizing application events. API response fixtures
  belong in unit/web-boundary tests, not this directory.

## Entrypoints

- `../../scripts/release.sh qualify` — assembled server, web, proxy,
  daemon-runtime fixture, and Playwright qualification. Before browser E2E,
  it verifies fake-agent conversation, readiness, and terminal session
  projections over authenticated HTTP. With `NORTH_RELEASE_ARTIFACT_DIR` set,
  it validates and qualifies extracted artifacts instead of building from source.
- `fake-agent.mjs` — deterministic local agent process.
- `http-proxy.mjs` — local TLS/SSE/WebSocket route fixture.

Fixture self-checks use supplied operator-managed TLS identity and normal
certificate verification; they never generate a CA or modify platform trust:

```sh
node tests/release/fake-agent.mjs --self-check
node --use-system-ca tests/release/http-proxy.mjs --self-check \
  --cert-file "$NORTH_RELEASE_TLS_CERT_FILE" \
  --key-file "$NORTH_RELEASE_TLS_KEY_FILE"
```

Run assembled qualification with an explicitly isolated PostgreSQL URL:

```sh
NORTH_TEST_DATABASE_URL=postgres://... NORTH_OTP_HMAC_KEY=$(openssl rand -hex 32) \
  ./scripts/release.sh qualify
```

The harness uses private server logs for OTP delivery. Local runs require one
persistent, operator-managed TLS server identity trusted by the host platform.
Set `NORTH_RELEASE_TLS_DIR` (default
`~/.config/north/release-tls`) or set `NORTH_RELEASE_TLS_CERT_FILE` and
`NORTH_RELEASE_TLS_KEY_FILE` directly. Defaults inside that directory are
`server.crt` and `server.key`. Certificate must be leaf-first (a full chain may follow), cover
`localhost` and `127.0.0.1`, include `serverAuth` and
`digitalSignature` key usage, not be a CA, remain valid for at least 24 hours
and already be valid, and match an unencrypted private key. Keep the directory
mode `0700` and key mode
`0600`. Local runs require the existing issuer in the normal platform trust
store; the harness and proxy self-check never alter workstation trust. In CI,
secret-free `release-preflight` verifies source and artifact first. A fresh
`ubuntu-latest` job in protected `release-qualification` re-downloads the same
artifact ID, repeats checks, and installs the existing `NORTH_RELEASE_TLS_CA_PEM`
root into ephemeral system trust, installs `libnss3-tools`, and sets
`NORTH_RELEASE_NSS_CA_TRUST=true` so Chromium trusts the CA through the isolated
per-run NSS database under temporary `HOME`. `release_cleanup` removes that
profile; workflow cleanup removes system trust and TLS files. Use test-only
leaf/key, never production credentials. After qualification, protected
`release-publish` requires review; it has no TLS secrets. Publisher creates a
draft, verifies asset names/bytes, then publishes without overwriting existing
releases. No path creates a CA, injects `NODE_EXTRA_CA_CERTS`, or disables
verification.

Set `NORTH_RELEASE_ARTIFACT_DIR` to use extracted `bin/` and `web/` artifacts
instead of workspace builds. This selects fail-closed artifact mode: the
qualification command checks manifest, checksums, Linux target, ELF binaries,
and versions before launching packaged processes. Invalid artifacts never
fall back to source builds. `./scripts/validate.sh smoke` requires this variable.

All release commands must leave `tests/release/` and its temporary state clean.
