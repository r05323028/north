use north_daemon::{DaemonCoordinator, DispatchOutcome, Journal, RecoveryOutcome, RuntimeExecutor};
use north_protocol::{
    Command, CommandEnvelope, DaemonFrame, Event, EventAck, EventAckStatus, MessageSend,
    ServerFrame, SessionReconcileState, SCHEMA_VERSION,
};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Clone)]
struct FakeExecutor {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RuntimeExecutor for FakeExecutor {
    fn dispatch(
        &self,
        operation_id: &str,
        _command_id: &str,
        _command: &Command,
    ) -> DispatchOutcome {
        self.calls.lock().expect("calls").push(operation_id.into());
        DispatchOutcome::DispatchSucceeded
    }

    fn recover(
        &self,
        _operation_id: &str,
        _command_id: &str,
        _command: &Command,
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
        command: Command::MessageSend(MessageSend {
            message_id: "message-1".into(),
            content: "one durable message".into(),
        }),
    }
}

#[test]
fn command_restart_and_compaction_remain_idempotent() {
    let directory = tempdir().expect("temporary journal");
    let path = directory.path().join("daemon.json");
    let calls = Arc::new(Mutex::new(Vec::new()));
    {
        let coordinator = DaemonCoordinator::new(
            Journal::open(&path, "daemon-1").expect("open journal"),
            FakeExecutor {
                calls: calls.clone(),
            },
        );
        coordinator
            .process_server_frame(ServerFrame::Command(command()))
            .expect("dispatch command");
        coordinator
            .journal()
            .compact_session("session-1")
            .expect("compact command");
    }
    let coordinator = DaemonCoordinator::new(
        Journal::open(&path, "daemon-1").expect("reopen journal"),
        FakeExecutor {
            calls: calls.clone(),
        },
    );
    let responses = coordinator
        .process_server_frame(ServerFrame::Command(command()))
        .expect("late duplicate ACK");
    assert!(matches!(responses.as_slice(), [DaemonFrame::CommandAck(_)]));
    assert_eq!(calls.lock().expect("calls").as_slice(), ["command-1"]);
}

#[test]
fn event_restart_replays_original_then_reconciliation_suppresses_it() {
    let directory = tempdir().expect("temporary journal");
    let path = directory.path().join("daemon.json");
    let event = {
        let journal = Journal::open(&path, "daemon-1").expect("open journal");
        journal
            .append_event(
                "event-1",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "thinking".into(),
                }),
            )
            .expect("append event")
            .envelope
    };
    let journal = Journal::open(&path, "daemon-1").expect("reopen journal");
    let replay = journal
        .replay_events(
            "session-1",
            &SessionReconcileState {
                session_id: "session-1".into(),
                command_ack_through_seq: 0,
                event_ack_through_seq: 0,
                event_ack_sparse: Vec::new(),
            },
        )
        .expect("replay event");
    assert_eq!(replay, vec![event.clone()]);
    journal
        .acknowledge_event(EventAck {
            event_id: event.event_id,
            session_id: event.session_id,
            daemon_event_seq: event.daemon_event_seq,
            schema_version: SCHEMA_VERSION,
            status: EventAckStatus::Rejected,
            reason: Some("downstream not ready".into()),
        })
        .expect("event ACK");
    assert!(journal
        .replay_events(
            "session-1",
            &SessionReconcileState {
                session_id: "session-1".into(),
                command_ack_through_seq: 0,
                event_ack_through_seq: 1,
                event_ack_sparse: Vec::new(),
            },
        )
        .expect("suppressed replay")
        .is_empty());
}
