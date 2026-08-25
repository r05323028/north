# Design

## Context

Multica-style CLI login; daemon dials out; server must know which machines exist.

## Decisions

- `north setup --server-url <url>` prints/opens browser auth URL carrying a
  short-lived request token; on approval the server returns a daemon credential
  (random secret) shown once, stored by CLI in user config with 0600.
- Credentials table separate from users/sessions: daemon_credential(id, label,
  secret_hash, created_by, created_at, revoked_at). Verification codes are
  never reused as daemon credentials.
- Connection endpoint upgrades to WebSocket only after credential auth;
  registers daemon identity (hostname, agent runtime versions) + capabilities;
  updates last_seen_at on heartbeat.
- Server-side liveness view powers Settings > Daemon Status; offline daemons
  show last seen, never business impact.

## Open Questions

- Exact heartbeat interval / offline threshold — config constants.
