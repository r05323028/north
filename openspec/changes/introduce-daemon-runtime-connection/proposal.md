# Introduce daemon registration and connection

## Why

Agent runtimes live on developer machines behind NAT. A daemon-initiated
connection with Multica-like CLI login lets any machine join without inbound
ports or a pairing bureaucracy.

## What Changes

- `north setup --server-url …`: browser completes normal North login, then the
  server issues a dedicated CLI/daemon credential stored locally (never a
  reused verification code).
- Daemon connects outbound (persistent WebSocket over TLS), authenticates
  with the local credential, registers identity + capabilities.
- Heartbeat/liveness tracking (last_seen_at); revocation support.
- Settings > Daemon Status shows connected/offline daemons without internals.

Out of scope: manual pairing codes, multi-user daemon sharing, remote daemon
administration beyond revoke.

## Capabilities

### New Capabilities

- `daemon-runtime`: setup/login flow, credential model, connection lifecycle,
  capability registration, liveness visibility.

### Modified Capabilities

(none)

## Impact

- New crate code in north-daemon (connection client); server gains WS endpoint
  (axum upgrade).
- Affected docs: docs/architecture/daemon.md, server-daemon-protocol.md
  (transport section reference).
- Dependencies on earlier changes: introduce-email-auth-and-owner-bootstrap
  (login), introduce-role-and-permission-model (revocation is admin-gated).
