//! Wire-level frame catalog shared by the server and the daemon.
//!
//! Direction is part of a message's identity. Three disjoint groups:
//!
//! 1. Connection/control frames (connection-scoped, either direction):
//!    - daemon → server: hello/registration (identity + capabilities)
//!    - daemon → server: heartbeat (liveness)
//!    - server → daemon: acknowledgement of durably processed event ids
//!    - server → daemon: resume/reconciliation state
//!
//! 2. Server commands (server → daemon ONLY):
//!    `session.start`, `session.cancel`, `session.resume`, `message.send`
//!
//! 3. Daemon events (daemon → server ONLY):
//!    `session.started`, `agent.message`, `agent.activity`,
//!    `requirement.assessed`, `session.completed`, `session.failed`
//!
//! `session.resume` exists ONLY as a server command; reconnect reconciliation
//! happens through control frames, never by duplicating a command as an event.
//!
//! Envelope invariants: daemon initiates the connection; every envelope carries
//! stable identifiers (`command_id` / `event_id` / `session_id`); delivery is
//! at-least-once with idempotent processing; the server ACKs processed event
//! ids and only then may the daemon trim them from its replay buffer.
//!
//! Owned by OpenSpec change `introduce-server-daemon-protocol`; see
//! docs/architecture/server-daemon-protocol.md.
