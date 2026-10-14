# north-cli Specification Delta

## Purpose

Defines the minimal operator CLI that configures and supervises the local North daemon. The command shape takes inspiration from [Multica's CLI and Agent Daemon Guide](https://github.com/multica-ai/multica/blob/main/CLI_AND_DAEMON.md); North reuses its own daemon approval and transport contracts rather than copying Multica authentication or implementation.

## ADDED Requirements

### Requirement: CLI setup enrolls and starts the local daemon

The `north` CLI SHALL provide `north setup --server-url HTTPS_URL [--label LABEL]` as the one-command setup path. It SHALL reuse `north-daemon setup` and its owner-only, atomic mode-0600 `~/.north/daemon.json` state. It SHALL start the bundled daemon only after credential claim succeeds. It SHALL NOT add another enrollment API or credential type.

#### Scenario: Browser approval completes setup

- **WHEN** an operator runs `north setup --server-url https://north.example` and approves the displayed request in an authenticated same-origin browser session
- **THEN** the CLI opens the server-provided same-origin approval URL directly when supported, stores the claimed daemon credential with owner-only permissions, and starts the bundled daemon without exposing credentials

#### Scenario: Invalid or expired setup fails closed

- **WHEN** the server URL is not HTTPS, approval is denied, the request expires, or setup polling reaches a terminal server response
- **THEN** setup exits non-successfully, reports a safe actionable error, writes no partial credential, and does not start the daemon

#### Scenario: Browser opening is unavailable

- **WHEN** the platform has no supported browser opener or opening fails
- **THEN** the CLI prints the validated same-origin URL once on stdout, omits it from persistent state and daemon logs/status, continues bounded polling until expiry, and keeps TLS verification enabled

#### Scenario: Setup credentials stay out of diagnostics

- **WHEN** setup tokens or daemon credentials are handled during setup or daemon runtime
- **THEN** daemon logs and status omit both values; only the claimed daemon credential is stored in `daemon.json` after approval

### Requirement: CLI supervises the bundled daemon safely

The CLI SHALL supervise `north-daemon` through an owner-only Unix-domain control socket. `north daemon start` SHALL background by default and support `--foreground`; `stop` SHALL await graceful confirmation within a bounded timeout; `status` SHALL report process/backend state as text or stable JSON. `~/.north` SHALL hold PID/instance/status/log metadata; credentials remain in `daemon.json`. PID metadata SHALL never authorize signals.

#### Scenario: Start survives CLI exit

- **WHEN** an operator runs `north daemon start` with valid setup state
- **THEN** the bundled daemon continues after the CLI exits, writes its log/status under `~/.north`, and status identifies its managed process

#### Scenario: Foreground mode streams daemon output

- **WHEN** an operator runs `north daemon start --foreground`
- **THEN** the CLI runs the bundled daemon attached to the terminal and propagates its exit status without creating a background PID record

#### Scenario: Stale PID or socket cannot stop an unrelated process

- **WHEN** PID metadata is stale or reused, or the control socket is unresponsive or reports a mismatched instance ID
- **THEN** stop uses only the owner-only control socket and matching instance ID; it reports stale/unresponsive state, sends no process signal, and does not unlink an unresponsive socket after a timed-out probe

#### Scenario: Stop preserves enrollment

- **WHEN** the managed daemon confirms graceful shutdown through its control socket
- **THEN** the CLI clears only live process/control metadata and retains daemon credentials, journal, and repository state

#### Scenario: Repeated start and stop operations are stable

- **WHEN** an operator repeats start or stop while the daemon is already running or stopped
- **THEN** the CLI reports stable status, starts no duplicate daemon, and does not delete credentials, the journal, or repository state

### Requirement: CLI backend status reflects protocol readiness

The bundled daemon SHALL expose `starting`, `connecting`, `connected`, `reconnecting`, `failed`, and `stopped`, with server URL, daemon ID, and process state. `connected` SHALL require hello/welcome, reconciliation snapshot, and coordination readiness. Retryable socket failures SHALL clear `connected` before backoff; terminal protocol/authentication failures SHALL report `failed` without credentials. Text and JSON output SHALL be stable and parseable.

#### Scenario: Connected means coordination ready

- **WHEN** the daemon completes the existing authenticated handshake, reconciliation, and readiness gate
- **THEN** `north daemon status` reports `connected`

#### Scenario: Retry clears stale connection state

- **WHEN** a connected daemon loses its socket and enters retryable backoff
- **THEN** status reports `reconnecting` rather than stale `connected`, then returns to `connected` only after a new readiness gate

#### Scenario: Terminal failure is safe

- **WHEN** daemon authentication or protocol validation fails terminally
- **THEN** status reports `failed` with a safe failure class and without exposing credentials

#### Scenario: Stale status is not reported connected

- **WHEN** the persisted status file says `connected` but the control socket is missing or does not answer with the matching instance ID
- **THEN** status reports the process as `stale` or `stopped` and does not report `connected`

### Requirement: CLI assets target supported operator hosts

SemVer CLI archives SHALL target Linux x86_64, macOS x86_64, and macOS ARM64. Each archive SHALL contain `north` and `north-daemon` built for the same target and version, target metadata, and a SHA-256 checksum. The Linux backend `north-server` binary SHALL remain in the `north-server` OCI image and SHALL NOT be included in CLI archives. CLI and daemon version mismatch SHALL fail packaging or preflight.

#### Scenario: Extracted archive is self-contained

- **WHEN** an operator extracts a published CLI archive on its declared target
- **THEN** `north --version`, `north daemon --help`, and the bundled daemon version succeed and match the archive metadata

#### Scenario: Unsupported target is not mislabeled

- **WHEN** a build does not produce the declared OS/architecture pair
- **THEN** archive creation and publication fail rather than labeling the binary for another target
