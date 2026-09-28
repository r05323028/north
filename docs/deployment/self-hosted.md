# North self-hosted deployment

The release build targets Linux `x86_64-unknown-linux-gnu` with glibc 2.31
or newer. Linux server and CLI/daemon builds use immutable amd64 builder
`rust:1.97.1-bullseye@sha256:02d78ca3f928195c2a907543de778adfd728ad7e2a24fdc6aef582b7c77842e0`.
`readelf` checks every packaged Linux ELF and rejects required GLIBC symbols
above 2.31. Server and Linux CLI manifests record same baseline. Release Cargo
commands use `--locked`; packaging refuses a dirty or changed source checkout
after builds. Operators install versioned GHCR server and web images with
`docker-compose.yaml`; workflow does not deploy North. Main merges qualify internal package/OCI archives and publish no image to
GHCR. An authorized strict `vX.Y.Z` tag
starts fresh package/OCI and native CLI builds plus qualification from tag SHA;
it does not alias main images. CLI archives upload to a draft GitHub Release
before tag-built SemVer images publish. Release becomes public only after both
image digests verify. Package/OCI workflow artifacts expire after 14 days; CLI
archives become public release assets after finalization. Pin each image to
its recorded digest.


## Prerequisites

- PostgreSQL 16 (PostgreSQL 15+ is expected to work, but 16 is qualification
  target).
- Node.js 22 for the standalone web process.
- A reverse proxy with HTTP/1.1, WebSocket upgrade, and streaming response
  support.
- TLS certificate trusted by browsers and daemon hosts. Certificate must cover
  public hostname; never disable hostname or certificate verification.
- `pi` executable available to daemon host, unless `NORTH_PI_AGENT_COMMAND`
  selects another approved executable.

Create one random OTP key and keep it outside logs, shell history, and source:

```bash
export NORTH_OTP_HMAC_KEY="$(openssl rand -hex 32)"
```

`NORTH_OTP_HMAC_KEY` must be exactly 64 hexadecimal characters. Changing it
invalidates active OTPs; rotate deliberately and re-authenticate users.

North 0.1.0 uses `LogCodeDelivery` by default; verification codes and email
addresses appear in `north-server` logs. Anyone with log access can use an
unexpired code to sign in as that email. Treat logs as an authentication
channel: restrict access and retention, protect backups, and do not forward them
to shared or untrusted sinks or include them in support bundles.

## PostgreSQL and server

Create an empty database and dedicated owner, then set `DATABASE_URL`. Run the
packaged migration command before server startup. It applies compiled SeaORM
migrations and exits; normal startup only verifies the current migration head
and fails before binding if schema is absent or behind. Preflight rejects
unpublished SQLx migration history and partial North schemas without changing
them. Back up any needed data, then manually recreate the disposable database.
North never drops or resets a database automatically.

```bash
export DATABASE_URL='postgres://north:CHANGE_ME@127.0.0.1:5432/north'
export NORTH_OTP_HMAC_KEY="$(openssl rand -hex 32)"
./bin/north-server migrate # no OTP key required by this command
export NORTH_BIND_ADDR='127.0.0.1:8080' # local-only; default is loopback
./bin/north-server
```

After each compatible schema-changing release, run `north-server migrate`
explicitly as one serialized deployment job per database. SeaORM's migration
runner does not provide cross-process locking. Wait for success before starting
or replacing server instances. First 0.1.0 use requires an empty baseline
database; old pre-release databases are not upgradeable.


SeaORM's PostgreSQL connector defaults to `sslmode=prefer`: it tries TLS, then
can fall back to plaintext. This keeps local PostgreSQL setups without TLS
working, but does not require encryption. For remote PostgreSQL, require
certificate and hostname verification with `sslmode=verify-full`; provide the
CA bundle when it is not in the driver's configured roots:

```bash
export DATABASE_URL='postgres://north:CHANGE_ME@db.example:5432/north?sslmode=verify-full&sslrootcert=/etc/north/postgres-ca.pem'
```

The CA file must be readable by the server process, and the database certificate
must match `db.example`. Keep the default for local development; do not use
`sslmode=prefer` as a security guarantee for remote database traffic.

Probe readiness only after process startup:

```bash
curl --fail http://127.0.0.1:8080/healthz
./bin/north-server --version
```

Keep PostgreSQL backups and test restore before upgrades. Upgrade procedure:

1. Back up database and daemon state.
2. Stop old server and web processes; keep proxy route unchanged.
3. Run `north-server migrate` with the same `DATABASE_URL` before starting a
   compatible release. First 0.1.0 use requires the empty baseline database
   described above; migration is never automatic at server startup.
4. Verify `/healthz`, authenticated `/auth/me`, and web asset loading.
5. Restart daemons if protocol or binary version requires it.
6. Keep rollback binary, database backup, and release manifest until verification
   completes. Never roll binaries backward across an applied incompatible
   migration without tested database restore.

The current deployment uses one server instance. Do not place multiple server
instances behind a load balancer without an external ownership/epoch design.

## Next.js web process

Use `web/` from package output. It is a Next standalone artifact; copy its
`.next/static` directory (the package already includes it) and `public/` when
present. Run one process behind the same reverse proxy origin:

```bash
cd web
HOSTNAME=127.0.0.1 PORT=3000 node server.js
```

Browser API calls and SSE use relative same-origin paths. Do not set a separate
browser API origin. Proxy these server paths to `north-server`:

- `/auth/**`, `/requirements/**`, `/events`
- `/daemon/**`, `/daemons/**`, `/users/**`, `/repositories/**`
- `/daemon/ws` with HTTP/1.1 WebSocket upgrade

`/requirements/{id}` is also a Next.js page route. Route only its browser
document and app-router RSC/prefetch `GET`/`HEAD` requests to Next.js (using
`Sec-Fetch-Dest: document`, `Accept: text/html`, or Next `rsc`/prefetch
headers). Keep headerless/API JSON requests, mutations, and `/requirements/{id}`
subpaths on `north-server`. Do not use header routing as an authorization check.
Proxy every other path to Next.js. Preserve `Cookie`, `Origin`, `Host`, and
`X-Forwarded-Proto`; stream `/events` without buffering or compression that
breaks event delivery. Browser never connects directly to daemon.

### Containers and Kubernetes

Loopback addresses in local examples only work when proxy and application
share a network namespace. For separate containers or pods, bind North
processes to pod interfaces:

```bash
NORTH_BIND_ADDR=0.0.0.0:8080 ./bin/north-server
HOSTNAME=0.0.0.0 PORT=3000 node server.js
```

Expose server and web through internal `ClusterIP` Services; permit ingress
traffic through cluster policy, not public `NodePort`/`LoadBalancer` Services.
Ingress terminates public HTTPS/WSS, redirects HTTP to HTTPS, routes the server
paths above to North over HTTP, and routes remaining paths to Next over HTTP.
Keep `/daemon/ws` upgrade and unbuffered `/events` streaming enabled. North
processes do not load ingress certificates or private keys.

## TLS and daemon

Terminate public TLS at a proxy or at a deployment edge that supports both SSE
and WebSocket. Daemon connects outbound to `wss://PUBLIC_HOST/daemon/ws`; no
inbound daemon port is required. Keep daemon state owner-readable only.

### Install the matching CLI archive

Download the matching `north-cli` archive and `.sha256` sidecar from the
finalized `vX.Y.Z` GitHub Release. Choose target matching host:

- Linux x86_64: `x86_64-unknown-linux-gnu`
- macOS x86_64: `x86_64-apple-darwin`
- macOS ARM64: `aarch64-apple-darwin`

Verify the archive before extracting the matching `north` and `north-daemon`
pair into `~/.local/bin`:

```bash
VERSION=X.Y.Z # replace with release version, without leading v
TARGET=aarch64-apple-darwin # or x86_64-apple-darwin / x86_64-unknown-linux-gnu
ARCHIVE="$HOME/Downloads/north-cli-v${VERSION}-${TARGET}.tar.gz"
(
  cd "$(dirname "$ARCHIVE")"
  shasum -a 256 -c "$(basename "$ARCHIVE").sha256"
)
mkdir -p "$HOME/.local/bin"
tar -xzf "$ARCHIVE" -C "$HOME/.local/bin" north north-daemon
chmod 0755 "$HOME/.local/bin/north" "$HOME/.local/bin/north-daemon"
export PATH="$HOME/.local/bin:$PATH"
north --version
north-daemon --version
```

Add `~/.local/bin` to shell `PATH` persistently if needed. The archive contains
matching binaries and metadata; the sidecar checks the complete archive. For
upgrade, stop the daemon before replacing both binaries from the same archive,
then start it and verify status; preserve `~/.north` across binary upgrades.

### Enroll and manage the daemon

```bash
north setup --server-url https://north.example --label 'build-agent-1'
north daemon status --output json
north daemon stop
north daemon start # normal background operation
```

`north setup` opens the same-origin approval URL in the system browser, or prints
it once if browser opening fails. Approve it in an authenticated browser
session. After claim, setup stores the credential in owner-only
`~/.north/daemon.json` and starts the daemon. The daemon connects outbound to
`wss://north.example/daemon/ws`; no inbound daemon port is required. Use
`north daemon start` runs in the background. For interactive diagnostics instead,
omit that command and run `north daemon start --foreground`; it propagates the
daemon's exit status.

Default state under `~/.north` includes secret `daemon.json`,
`daemon.journal.json`, control socket, status/log files, repository cache and
workspaces, and private Pi session context. Keep the directory owner-only and
protect it in backups. `north daemon stop` preserves enrollment and journal
state. Never copy daemon credentials into server environment or browser storage.

## OCI images and Compose

Release workflow builds Linux x86_64 images from one checked package. Each
successful main qualification creates no GHCR refs. An authorized strict
`vX.Y.Z` tag builds and qualifies fresh package/OCI inputs from tag SHA, then
publishes those images only to SemVer refs. Example 0.1.0 refs:

- `ghcr.io/r05323028/north-server:v0.1.0`
- `ghcr.io/r05323028/north-web:v0.1.0`

The main `publish-images` summary records SHA-image digests; the
`publish-semver-images` summary records tag-built SemVer digests. Tag and main
digests may differ; compare each with its own qualified OCI manifest and record
both. Pin image references as
`ghcr.io/r05323028/north-server@sha256:<digest>` and
`ghcr.io/r05323028/north-web@sha256:<digest>`. GHCR package visibility is an
owner setting; authenticate with `docker login ghcr.io` when packages are private.

Use `docker-compose.yaml` from the same source tag as images:

```bash
git show "v0.1.0:docker-compose.yaml" > docker-compose.yaml
```

Create a private `.env` once; keep it out of version control. Keep the database
password and OTP key stable across restarts and upgrades:

```bash
umask 077
db_password=$(openssl rand -hex 24)
otp_key=$(openssl rand -hex 32)
cat > .env <<EOF
POSTGRES_DB=north
POSTGRES_USER=north
POSTGRES_PASSWORD=$db_password
DATABASE_URL=postgres://north:$db_password@postgres:5432/north
NORTH_OTP_HMAC_KEY=$otp_key
NORTH_SERVER_IMAGE=ghcr.io/r05323028/north-server:v0.1.0
NORTH_WEB_IMAGE=ghcr.io/r05323028/north-web:v0.1.0
EOF
chmod 600 .env
```

Replace image tags in `.env` with the recorded `@sha256:` digests when pinning.
Start and inspect the stack:

```bash
docker compose --file docker-compose.yaml up --detach
docker compose --file docker-compose.yaml ps
```

Compose runs PostgreSQL 16, server, and web only. PostgreSQL uses named volume
`postgres_data`; `docker compose down` preserves it, while `down --volumes`
deletes it. Back up and test restore before upgrades. Server and web host ports
bind to loopback (`8080` and `3000` by default) for the external TLS proxy.
Keep the documented same-origin route table: API/SSE and `/daemon/ws` go to the
server; page, RSC, and other web requests go to Next. Compose includes no proxy
or daemon. Run `north-daemon` on its managed host and connect outbound to the
same public `https://` origin; never put daemon credentials in Compose.

## Image verification and release limits

Get exact SHA and SemVer image digests from the corresponding protected
`publish-images` or `publish-semver-images` job summary and inspect those
immutable references before use:

```bash
SERVER_IMAGE='ghcr.io/r05323028/north-server@sha256:<digest>'
WEB_IMAGE='ghcr.io/r05323028/north-web@sha256:<digest>'
docker buildx imagetools inspect "$SERVER_IMAGE"
docker buildx imagetools inspect "$WEB_IMAGE"
```

Authenticate with `docker login ghcr.io` first when GHCR packages are private.
The workflow's 14-day Actions artifact contains package and OCI archives used
only for preflight and qualification; operators install from GHCR images and
Compose. Maintainer-only extracted-package smoke and OCI qualification are
covered by [`docs/development/testing.md`](../development/testing.md) and the
[release checklist](../development/release-checklist.md).

Only `release-qualification` reads test-only repository TLS secrets. The
`publish-semver-images` tag job receives `packages: write`; draft upload and
finalization jobs receive `contents: write` only. No publisher receives TLS
secrets or both write permissions, and no GitHub Environments are used. Main
and manual qualification create no public release or GHCR refs. A strict tag
creates a draft GitHub Release containing only CLI archives and checksum
sidecars; it becomes public only after both SemVer OCI digests verify. Protect
`v*` Git tags with a ruleset that restricts creation, update, and deletion to
authorized release maintainers. Local
maintainer qualification needs an existing operator-managed TLS identity: set
`NORTH_RELEASE_TLS_DIR`, or `NORTH_RELEASE_TLS_CERT_FILE` and
`NORTH_RELEASE_TLS_KEY_FILE`, and install its issuer through the normal local
platform trust store. Hosted qualification imports the existing CA only into
ephemeral trust and cleans it unconditionally. The tag workflow publishes a
GitHub Release containing only CLI archives and checksum sidecars after both
SemVer OCI images succeed; main and manual qualification create no public
release. No workflow creates a CA, changes workstation trust, injects
`NODE_EXTRA_CA_CERTS`, or disables verification.

Current topology limitations: one server instance; no external broker, HA
ownership epochs, or automatic live migration; and no OS-enforced repository
sandbox. Daemon workspace and dirty-tree checks provide process-level isolation
only. The workflow does not publish signing, provenance attestations, or an
SBOM; image digests identify content but are not attestations.
