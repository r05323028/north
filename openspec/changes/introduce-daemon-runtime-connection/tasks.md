## 1. Credential and identity lifecycle

- [x] 1.1 Migration 0007: daemon registrations/credentials with durable `daemon_id`, secret hash, label, `created_by`, `created_at`, `revoked_at`, and liveness fields
- [x] 1.2 Setup endpoint pair (request token → approve → one-time secret display) with user ownership and no credential reuse
- [x] 1.3 CLI scaffold in north-daemon bin: setup + start subcommands; store secret 0600 and durable local daemon identity

## 2. Connection and routing

- [x] 2.1 Server WS endpoint: credential/protocol auth, register identity/capabilities, heartbeat updates `last_seen_at`
- [x] 2.2 Add the single `tokio-tungstenite` connection supervisor with bounded transport backoff; no daemon-owned business retry budget. Runtime/session code has no separate reconnect loop.
- [x] 2.5 Add explicit Connecting/AwaitingWelcome/Authenticated/Reconciling/ReconciliationReceived/Active gating, typed handshake-result delivery, coordination readiness, handshake timeouts, terminal protocol failure classification, and transport backoff reset.
- [x] 2.3 Session start selects an eligible daemon, persists `session.daemon_id` with the first command, and rejects frames from other identities
- [x] 2.4 Revocation drops live connections, refuses new ones, and leaves pinned sessions to server retry/failure without migration

## 3. Surface

- [x] 3.1 Settings > Daemon Status page (live/offline, last seen, capabilities, owner/revocation visibility without runtime internals)

## 4. Validation

- [x] 4.1 Integration: connect/auth/register/heartbeat/revoke, owner/admin permissions, pinned routing, reconnect, and no-migration paths
- [x] 4.2 Full Rust gate + `openspec validate --all --strict`

## 5. Verification remediation

- [x] 5.1 Reconciliation is applied by daemon coordination before readiness; post-handshake frames are delivered to coordination instead of discarded
- [x] 5.2 Revalidate authenticated daemon connection on every inbound application frame so revocation cuts access immediately
- [x] 5.3 Allow credential owners to view their own daemon status while retaining all-daemon visibility for Admin/Owner
- [x] 5.4 Derive daemon Live/Offline status from a bounded `last_seen_at` liveness window, not only transport close state
- [x] 5.5 Extend integration/unit coverage for owner access, owner revocation, offline pinned-session ownership, and reconciliation application
- [x] 5.6 Remove unused server welcome/reconciliation timeout knobs; daemon supervisor remains owner of those stage deadlines
- [x] 5.7 Apply the same heartbeat liveness window when selecting eligible daemons for new sessions
- [x] 5.8 Execute DB-backed daemon lifecycle coverage in a dedicated CI PostgreSQL job while keeping no-database local workspace runs deterministic
