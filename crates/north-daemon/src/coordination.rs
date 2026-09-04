//! North coordination above the WebSocket transport.
//!
//! The transport supervisor only moves typed frames. This coordinator decides
//! durable idempotency, journal state, replay, and the narrow runtime seam.

use crate::journal::{CommandProcessResult, Journal, JournalError, RuntimeExecutor};
use north_protocol::{
    CommandEnvelope, DaemonFrame, Event, ReconcileSnapshot, ServerFrame, SCHEMA_VERSION,
};
use std::{error::Error, fmt, sync::Arc};

#[derive(Debug)]
pub enum CoordinationError {
    Journal(JournalError),
    RetryableGap { session_id: String, limit: usize },
    TerminalProtocol { code: String, message: String },
}

impl fmt::Display for CoordinationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Journal(error) => error.fmt(f),
            Self::RetryableGap { session_id, limit } => {
                write!(
                    f,
                    "retryable gap boundary for session {session_id} (limit {limit})"
                )
            }
            Self::TerminalProtocol { code, message } => {
                write!(f, "terminal protocol error ({code}): {message}")
            }
        }
    }
}

impl Error for CoordinationError {}

impl From<JournalError> for CoordinationError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeActions {
    pub replay: Vec<DaemonFrame>,
}

#[derive(Debug, Default)]
pub struct RuntimeActions {
    pub frames: Vec<DaemonFrame>,
    pub commands: Vec<CommandEnvelope>,
}

/// Durable coordinator for one daemon identity.
pub struct DaemonCoordinator<E> {
    journal: Journal,
    executor: Arc<E>,
}

impl<E: RuntimeExecutor> DaemonCoordinator<E> {
    pub fn new(journal: Journal, executor: E) -> Self {
        Self {
            journal,
            executor: Arc::new(executor),
        }
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn executor(&self) -> Arc<E> {
        self.executor.clone()
    }

    /// Recover commands without running long-lived runtime work before the
    /// transport is active. Received commands are durably marked for dispatch;
    /// dispatch-started commands remain non-resubmittable and become unknown.
    pub fn recover_for_scheduler(&self) -> Result<RuntimeActions, CoordinationError> {
        let recovery = self.journal.recover_for_scheduler(self.executor.as_ref())?;
        Ok(RuntimeActions {
            frames: command_results_to_frames(recovery.results),
            commands: recovery.commands,
        })
    }

    /// Recover local records before application traffic is released.
    pub fn recover(&self) -> Result<Vec<DaemonFrame>, CoordinationError> {
        let results = self.journal.recover(self.executor.as_ref())?;
        Ok(command_results_to_frames(results))
    }

    /// Apply the one connection-level reconciliation snapshot, then replay
    /// every still-eligible event in ascending per-session sequence order.
    pub fn reconcile(
        &self,
        snapshot: ReconcileSnapshot,
    ) -> Result<HandshakeActions, CoordinationError> {
        self.journal.apply_reconciliation(snapshot.clone())?;
        let mut replay = Vec::new();
        for session in snapshot.sessions {
            replay.extend(
                self.journal
                    .replay_events(&session.session_id, &session)?
                    .into_iter()
                    .map(DaemonFrame::Event),
            );
        }
        Ok(HandshakeActions { replay })
    }

    /// Accept one server frame without entering long-running runtime work.
    /// Durable command intake and runtime scheduling are separate operations.
    pub fn accept_server_frame(
        &self,
        frame: ServerFrame,
    ) -> Result<RuntimeActions, CoordinationError> {
        match frame {
            ServerFrame::Command(command) => match self.accept_command(command) {
                Ok(actions) => Ok(actions),
                Err(CoordinationError::Journal(JournalError::IdentityConflict(reason))) => {
                    Ok(RuntimeActions {
                        frames: vec![protocol_error("command_identity_conflict", reason)],
                        commands: Vec::new(),
                    })
                }
                Err(CoordinationError::Journal(JournalError::GapOverflow {
                    session_id,
                    limit,
                })) => Err(CoordinationError::RetryableGap { session_id, limit }),
                Err(error) => Err(error),
            },
            ServerFrame::EventAck(ack) => {
                let session_id = ack.session_id.clone();
                match self.journal.acknowledge_event(ack) {
                    Ok(()) => {
                        self.journal.compact_session(&session_id)?;
                        Ok(RuntimeActions::default())
                    }
                    Err(JournalError::IdentityConflict(reason)) => Ok(RuntimeActions {
                        frames: vec![protocol_error("event_ack_identity_conflict", reason)],
                        commands: Vec::new(),
                    }),
                    Err(error) => Err(CoordinationError::Journal(error)),
                }
            }
            ServerFrame::ProtocolError(error) => Err(CoordinationError::TerminalProtocol {
                code: error.code,
                message: error.message,
            }),
            ServerFrame::Welcome(_) | ServerFrame::Reconcile(_) => Ok(RuntimeActions::default()),
        }
    }

    /// Finalize one worker result in the durable journal and expose the next
    /// same-session command, if command sequencing now permits it.
    pub fn finish_runtime(
        &self,
        session_id: &str,
        command_id: &str,
        outcome: crate::journal::DispatchOutcome,
        runtime_events: Vec<Event>,
    ) -> Result<RuntimeActions, CoordinationError> {
        let emitted = self
            .journal
            .complete_command(command_id, outcome, runtime_events)?;
        let mut actions = RuntimeActions {
            frames: emitted.into_iter().map(DaemonFrame::Event).collect(),
            commands: Vec::new(),
        };
        if let Some(accepted) = self.journal.accept_next_ready_command(session_id)? {
            actions
                .frames
                .extend(command_result_to_frames(accepted.result));
            if let Some(command) = accepted.command {
                actions.commands.push(command);
            }
        }
        Ok(actions)
    }

    /// Process one server frame after handshake readiness. A command ACK is
    /// emitted only after its command record is durable; event ACKs update the
    /// local journal and never cause automatic runtime resubmission.
    pub fn process_server_frame(
        &self,
        frame: ServerFrame,
    ) -> Result<Vec<DaemonFrame>, CoordinationError> {
        match frame {
            ServerFrame::Command(command) => match self.process_command(command) {
                Ok(frames) => Ok(frames),
                Err(CoordinationError::Journal(JournalError::IdentityConflict(reason))) => {
                    Ok(vec![protocol_error("command_identity_conflict", reason)])
                }
                Err(CoordinationError::Journal(JournalError::GapOverflow {
                    session_id,
                    limit,
                })) => Err(CoordinationError::RetryableGap { session_id, limit }),
                Err(error) => Err(error),
            },
            ServerFrame::EventAck(ack) => {
                let session_id = ack.session_id.clone();
                match self.journal.acknowledge_event(ack) {
                    Ok(()) => {
                        self.journal.compact_session(&session_id)?;
                        Ok(Vec::new())
                    }
                    Err(JournalError::IdentityConflict(reason)) => {
                        Ok(vec![protocol_error("event_ack_identity_conflict", reason)])
                    }
                    Err(error) => Err(CoordinationError::Journal(error)),
                }
            }
            ServerFrame::ProtocolError(error) => Err(CoordinationError::TerminalProtocol {
                code: error.code,
                message: error.message,
            }),
            ServerFrame::Welcome(_) | ServerFrame::Reconcile(_) => Ok(Vec::new()),
        }
    }

    pub fn append_event(
        &self,
        event_id: impl Into<String>,
        session_id: impl Into<String>,
        event: Event,
    ) -> Result<DaemonFrame, CoordinationError> {
        Ok(DaemonFrame::Event(
            self.journal
                .append_event(event_id, session_id, event)?
                .envelope,
        ))
    }

    fn accept_command(
        &self,
        command: CommandEnvelope,
    ) -> Result<RuntimeActions, CoordinationError> {
        let accepted = if matches!(&command.command, north_protocol::Command::SessionCancel(_)) {
            self.journal.accept_control_command(command)?
        } else {
            self.journal.accept_command(command)?
        };
        Ok(RuntimeActions {
            frames: command_result_to_frames(accepted.result),
            commands: accepted.command.into_iter().collect(),
        })
    }

    fn process_command(
        &self,
        command: CommandEnvelope,
    ) -> Result<Vec<DaemonFrame>, CoordinationError> {
        let session_id = command.session_id.clone();
        let result = self
            .journal
            .process_command(command, self.executor.as_ref())?;
        let mut frames = command_result_to_frames(result);
        frames.extend(
            self.journal
                .process_ready(&session_id, self.executor.as_ref())?
                .into_iter()
                .flat_map(command_result_to_frames),
        );
        Ok(frames)
    }
}

fn command_results_to_frames(results: Vec<CommandProcessResult>) -> Vec<DaemonFrame> {
    results
        .into_iter()
        .flat_map(command_result_to_frames)
        .collect()
}

fn command_result_to_frames(result: CommandProcessResult) -> Vec<DaemonFrame> {
    let mut frames = vec![DaemonFrame::CommandAck(result.acknowledgement)];
    frames.extend(result.emitted_events.into_iter().map(DaemonFrame::Event));
    frames
}

/// Construct a structurally valid protocol error for an invalid server frame.
pub fn protocol_error(code: impl Into<String>, message: impl Into<String>) -> DaemonFrame {
    DaemonFrame::ProtocolError(north_protocol::ProtocolErrorFrame {
        schema_version: SCHEMA_VERSION,
        code: code.into(),
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{DispatchOutcome, RecoveryOutcome};
    use north_protocol::{MessageSend, ProtocolErrorFrame, SessionResume};
    use sha2::Digest;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct Fake {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl RuntimeExecutor for Fake {
        fn dispatch(
            &self,
            operation_id: &str,
            _command_id: &str,
            _command: &north_protocol::Command,
        ) -> DispatchOutcome {
            self.calls.lock().expect("calls").push(operation_id.into());
            DispatchOutcome::DispatchSucceeded
        }

        fn recover(
            &self,
            _operation_id: &str,
            _command_id: &str,
            _command: &north_protocol::Command,
        ) -> RecoveryOutcome {
            RecoveryOutcome::Unknown
        }
    }

    fn command() -> CommandEnvelope {
        CommandEnvelope {
            command_id: "command-1".into(),
            session_id: "session-1".into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: north_protocol::Command::MessageSend(MessageSend {
                message_id: "message-1".into(),
                content: "hello".into(),
            }),
        }
    }

    #[test]
    fn coordinator_acknowledges_duplicate_without_second_runtime_call() {
        let directory = tempdir().expect("temp directory");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let coordinator = DaemonCoordinator::new(
            Journal::open(directory.path().join("journal.json"), "daemon-1").expect("journal"),
            Fake {
                calls: calls.clone(),
            },
        );
        let first = coordinator
            .process_server_frame(ServerFrame::Command(command()))
            .expect("first command");
        let second = coordinator
            .process_server_frame(ServerFrame::Command(command()))
            .expect("duplicate command");
        assert!(matches!(first.as_slice(), [DaemonFrame::CommandAck(_)]));
        assert!(matches!(second.as_slice(), [DaemonFrame::CommandAck(_)]));
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn command_identity_conflict_emits_protocol_error() {
        let directory = tempdir().expect("temp directory");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let coordinator = DaemonCoordinator::new(
            Journal::open(directory.path().join("journal.json"), "daemon-1").expect("journal"),
            Fake {
                calls: calls.clone(),
            },
        );
        coordinator
            .process_server_frame(ServerFrame::Command(command()))
            .expect("first command");
        let mut conflict = command();
        conflict.command_id = "command-2".into();
        conflict.server_command_seq = 2;
        let responses = coordinator
            .process_server_frame(ServerFrame::Command(conflict))
            .expect("protocol error response");
        assert!(matches!(
            responses.as_slice(),
            [DaemonFrame::ProtocolError(_)]
        ));
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn bounded_gap_returns_retryable_boundary() {
        let directory = tempdir().expect("temp directory");
        let journal = crate::journal::Journal::open_with_config(
            directory.path().join("journal.json"),
            "daemon-1",
            crate::journal::JournalConfig {
                max_gap_buffer_entries_per_session: 1,
            },
        )
        .expect("journal");
        let coordinator = DaemonCoordinator::new(
            journal,
            Fake {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let mut first_gap = command();
        first_gap.command_id = "command-2".into();
        first_gap.server_command_seq = 2;
        assert!(coordinator
            .process_server_frame(ServerFrame::Command(first_gap))
            .expect("buffer gap")
            .iter()
            .all(|frame| matches!(frame, DaemonFrame::CommandAck(_))));
        let mut overflow = command();
        overflow.command_id = "command-3".into();
        overflow.server_command_seq = 3;
        if let north_protocol::Command::MessageSend(message) = &mut overflow.command {
            message.message_id = "message-3".into();
        }
        assert!(matches!(
            coordinator.process_server_frame(ServerFrame::Command(overflow)),
            Err(CoordinationError::RetryableGap { limit: 1, .. })
        ));
    }

    #[test]
    fn protocol_error_is_terminal_to_current_coordinator() {
        let directory = tempdir().expect("temp directory");
        let coordinator = DaemonCoordinator::new(
            Journal::open(directory.path().join("journal.json"), "daemon-1").expect("journal"),
            Fake {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let result =
            coordinator.process_server_frame(ServerFrame::ProtocolError(ProtocolErrorFrame {
                schema_version: SCHEMA_VERSION,
                code: "invalid_frame".into(),
                message: "close".into(),
            }));
        assert!(matches!(
            result,
            Err(CoordinationError::TerminalProtocol { .. })
        ));
    }

    #[test]
    fn recovery_frames_do_not_resubmit_unknown_operation() {
        let directory = tempdir().expect("temp directory");
        let journal =
            Journal::open(directory.path().join("journal.json"), "daemon-1").expect("journal");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let fake = Fake {
            calls: calls.clone(),
        };
        let command = CommandEnvelope {
            command_id: "command-unknown".into(),
            session_id: "session-1".into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: north_protocol::Command::SessionResume(SessionResume {}),
        };
        let digest = {
            let value = serde_json::to_vec(&command).expect("json");
            let digest = sha2::Sha256::digest(value);
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        journal
            .prepare_command_for_test(command, digest)
            .expect("prepare");
        journal
            .mark_dispatch_started_for_test("command-unknown")
            .expect("started");
        let coordinator = DaemonCoordinator::new(journal, fake);
        let frames = coordinator.recover().expect("recover");
        assert!(frames
            .iter()
            .any(|frame| matches!(frame, DaemonFrame::Event(_))));
        assert!(calls.lock().expect("calls").is_empty());
    }
}
