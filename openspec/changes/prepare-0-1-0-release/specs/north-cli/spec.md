# north-cli Specification Delta

## Purpose

Defines the minimal operator CLI that configures and supervises the local North daemon. The command shape takes inspiration from [Multica's CLI and Agent Daemon Guide](https://github.com/multica-ai/multica/blob/main/CLI_AND_DAEMON.md); North reuses its own daemon approval and transport contracts rather than copying Multica authentication or implementation.

## ADDED Requirements

### Requirement: CLI setup enrolls and starts the local daemon

The `north` CLI SHALL provide `north setup --server-url HTTPS_URL [--label LABEL]` as the one-command daemon setup path. It SHALL reuse the existing `north-daemon setup` device flow: request setup over HTTPS, present the server-provided same-origin approval URL, wait only until request expiry, claim the approved daemon credential, and store it in the existing owner-only `~/.north/daemon.json` state using the existing atomic mode-0600 behavior. It SHALL pass the same-origin approval URL directly to the system browser when supported. If browser opening fails, it SHALL print the URL once on stdout as an explicit operator fallback; that display SHALL not be persisted or copied to daemon logs/status. Credentials and setup tokens SHALL never appear in daemon logs or status output. After successful claim, it SHALL start the bundled daemon in the background. It SHALL NOT add OAuth/PAT tokens, user-session storage, or a second daemon enrollment API.

#### Scenario: Browser approval completes setup

- **WHEN** an operator runs `north setup --server-url https://north.example` and approves the displayed request in an authenticated same-origin browser session
- **THEN** the CLI stores the claimed daemon credential with owner-only permissions and starts the bundled daemon without exposing the credential

#### Scenario: Invalid or expired setup fails closed

- **WHEN** the server URL is not HTTPS, approval is denied, the request expires, or setup polling reaches a terminal server response
- **THEN** setup exits non-successfully, reports a safe actionable error, writes no partial credential, and does not start the daemon

#### Scenario: Browser opening is unavailable

- **WHEN** the platform has no supported browser opener or opening fails
- **THEN** the CLI prints the validated same-origin approval URL and continues bounded polling without weakening TLS verification

### Requirement: CLI supervises the bundled daemon safely

Each CLI release archive SHALL contain matching `north` and `north-daemon` binaries. `north daemon start` SHALL run the sibling daemon in the background by default; `--foreground` SHALL run it interactively for diagnostics. `north daemon stop` SHALL request graceful shutdown over an owner-only Unix-domain control socket and wait for confirmation within a bounded timeout; timeout SHALL fail closed without signaling a PID. `north daemon status` SHALL query the live control socket and report process/backend connection state in human-readable output and `--output json`. Local PID/instance/status/log metadata SHALL live under `~/.north`; daemon credentials remain in `daemon.json`. The daemon SHALL verify the instance ID on control requests. PID metadata is informational only: stale or reused PIDs SHALL never cause process signals. An unresponsive existing socket SHALL be reported stale/unresponsive and SHALL NOT be unlinked based only on a timed-out probe. Repeated start/stop operations SHALL return stable status without duplicating daemons or deleting credentials.

#### Scenario: Start survives CLI exit

- **WHEN** an operator runs `north daemon start` with valid setup state
- **THEN** the bundled daemon continues after the CLI exits, writes its log/status under `~/.north`, and status identifies its managed process

#### Scenario: Foreground mode streams daemon output

- **WHEN** an operator runs `north daemon start --foreground`
- **THEN** the CLI runs the bundled daemon attached to the terminal and propagates its exit status without creating a background PID record

#### Scenario: Stale PID cannot stop an unrelated process

- **WHEN** PID metadata is stale or reused by another process
- **THEN** stop uses only the control socket, reports stale/unresponsive state if the managed daemon does not answer, and sends no process signal

#### Scenario: Stop preserves enrollment

- **WHEN** the managed daemon confirms graceful shutdown through its control socket
- **THEN** the CLI clears only live process/control metadata and retains daemon credentials, journal, and repository state

### Requirement: CLI backend status reflects protocol readiness

The bundled daemon SHALL expose local status updates to the CLI for `starting`, `connecting`, `connected`, `reconnecting`, `failed`, and `stopped` states. `connected` SHALL be reported only after the existing hello/welcome, reconciliation snapshot, and coordination-readiness gate succeeds. Retryable socket failures SHALL clear connected state before backoff; terminal protocol/authentication failures SHALL report failed state without credentials. Status SHALL show server URL, daemon ID, process state, and connection state without printing credential material. JSON status SHALL be parseable and use stable field names.

#### Scenario: Connected means coordination ready

- **WHEN** the daemon completes the existing authenticated handshake, reconciliation, and readiness gate
- **THEN** `north daemon status` reports `connected`

#### Scenario: Retry clears stale connection state

- **WHEN** a connected daemon loses its socket and enters retryable backoff
- **THEN** status reports `reconnecting` rather than stale `connected`, then returns to `connected` only after a new readiness gate

#### Scenario: Terminal failure is safe

- **WHEN** daemon authentication or protocol validation fails terminally
- **THEN** status reports `failed` and safe failure class without exposing credentials

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
