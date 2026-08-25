## 1. Credential and identity lifecycle

- [ ] 1.1 Migration 0007: daemon registrations/credentials with durable `daemon_id`, secret hash, label, `created_by`, `created_at`, `revoked_at`, and liveness fields
- [ ] 1.2 Setup endpoint pair (request token → approve → one-time secret display) with user ownership and no credential reuse
- [ ] 1.3 CLI scaffold in north-daemon bin: setup + start subcommands; store secret 0600 and durable local daemon identity

## 2. Connection and routing

- [ ] 2.1 Server WS endpoint: credential/protocol auth, register identity/capabilities, heartbeat updates `last_seen_at`
- [x] 2.2 Add the single `tokio-tungstenite` connection supervisor with bounded transport backoff; no daemon-owned business retry budget. Runtime/session code has no separate reconnect loop.
- [ ] 2.3 Session start selects an eligible daemon, persists `session.daemon_id` with the first command, and rejects frames from other identities
- [ ] 2.4 Revocation drops live connections, refuses new ones, and leaves pinned sessions to server retry/failure without migration

## 3. Surface

- [ ] 3.1 Settings > Daemon Status page (live/offline, last seen, capabilities, owner/revocation visibility without runtime internals)

## 4. Validation

- [ ] 4.1 Integration: connect/auth/register/heartbeat/revoke, owner/admin permissions, pinned routing, reconnect, and no-migration paths
- [ ] 4.2 Full Rust gate + `openspec validate --all --strict`
