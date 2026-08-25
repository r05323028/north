## 1. Credential lifecycle

- [ ] 1.1 Migration 0007: daemon_credentials (secret hash, label, created_by, revoked_at)
- [ ] 1.2 Setup endpoint pair (request token → approve → one-time secret display)
- [ ] 1.3 CLI scaffold in north-daemon bin: setup + start subcommands; store secret 0600

## 2. Connection

- [ ] 2.1 Server WS endpoint: credential auth, register identity/capabilities, heartbeat updates last_seen_at
- [ ] 2.2 Reconnect loop with bounded backoff (constants configurable)
- [ ] 2.3 Revocation drops live connections and refuses new ones

## 3. Surface

- [ ] 3.1 Settings > Daemon Status page (live/offline, last seen, capabilities)

## 4. Validation

- [ ] 4.1 Integration: connect/auth/register/heartbeat/revoke happy+unhappy paths
- [ ] 4.2 Full Rust gate + openspec validate --strict
