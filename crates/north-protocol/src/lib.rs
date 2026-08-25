//! Wire-level command/event types shared by the server and the daemon.
//!
//! Owned by OpenSpec change `introduce-server-daemon-protocol`. Planned baseline:
//!
//! Server → Daemon commands: session.start, session.cancel, session.resume, message.send
//! Daemon → Server events:  session.started, agent.message, agent.activity,
//!                          requirement.assessed, session.completed, session.failed
//!
//! Every envelope carries stable identifiers (command_id / event_id / session_id);
//! delivery is at-least-once with idempotent processing. See
//! docs/architecture/server-daemon-protocol.md.
