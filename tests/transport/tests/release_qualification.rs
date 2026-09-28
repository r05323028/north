use std::{env, time::Duration};

use north_daemon::transport::{
    ConnectionConfig, ConnectionError, ConnectionEvent, ConnectionSupervisor,
};
use north_protocol::{
    AgentActivity, AgentMessage, Command, CommandAck, DaemonFrame, Event, Event as ProtocolEvent,
    EventAckStatus, EventEnvelope, Hello, ReadinessVerdictWire, RequirementAssessed, ServerFrame,
    SessionCompleted, SessionStarted, SCHEMA_VERSION,
};
use tokio::sync::mpsc;

fn event(session_id: &str, event_id: &str, sequence: u64, payload: ProtocolEvent) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.into(),
        session_id: session_id.into(),
        daemon_event_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        event: payload,
    }
}

async fn next_frame(receiver: &mut mpsc::Receiver<ConnectionEvent>) -> ServerFrame {
    loop {
        match tokio::time::timeout(Duration::from_secs(30), receiver.recv())
            .await
            .expect("release WSS frame timeout")
            .expect("release WSS supervisor closed")
        {
            ConnectionEvent::Reconnecting => {}
            ConnectionEvent::HandshakeComplete { ready, .. } => {
                ready.send(()).expect("signal daemon coordination ready");
            }
            ConnectionEvent::Frame(frame) => return frame,
        }
    }
}

async fn send_event_and_expect_ack(
    outbound: &mpsc::Sender<DaemonFrame>,
    inbound: &mut mpsc::Receiver<ConnectionEvent>,
    event: EventEnvelope,
) {
    let event_id = event.event_id.clone();
    outbound
        .send(DaemonFrame::Event(event))
        .await
        .expect("send release event");
    match next_frame(inbound).await {
        ServerFrame::EventAck(ack) => {
            assert_eq!(ack.event_id, event_id);
            assert_eq!(ack.status, EventAckStatus::Accepted);
        }
        other => panic!("expected event ACK for {event_id}, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires live North WSS proxy, daemon credentials, and queued requirement"]
async fn release_daemon_runtime_qualifies_trusted_wss_protocol() {
    let server_url = env::var("NORTH_RELEASE_WSS_URL")
        .expect("NORTH_RELEASE_WSS_URL is required for release WSS qualification");
    assert!(
        server_url.starts_with("wss://"),
        "release URL must use wss://"
    );
    let daemon_id = env::var("NORTH_RELEASE_DAEMON_ID")
        .expect("NORTH_RELEASE_DAEMON_ID is required for release WSS qualification");
    let credential = env::var("NORTH_RELEASE_DAEMON_CREDENTIAL")
        .expect("NORTH_RELEASE_DAEMON_CREDENTIAL is required for release WSS qualification");
    let requirement_id = env::var("NORTH_RELEASE_REQUIREMENT_ID")
        .expect("NORTH_RELEASE_REQUIREMENT_ID is required for release WSS qualification");

    let (probe, _) = tokio_tungstenite::connect_async(server_url.as_str())
        .await
        .unwrap_or_else(|error| panic!("native-root WSS upgrade failed: {error}"));
    eprintln!("native-root WSS upgrade passed");
    drop(probe);

    let config = ConnectionConfig::new(
        server_url,
        Hello::new(daemon_id, credential, vec!["agent".into()]),
    );
    let supervisor = ConnectionSupervisor::new(config);
    let (outbound, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let (inbound_sender, mut inbound) = mpsc::channel(32);
    let task = tokio::spawn(async move { supervisor.run(outbound_receiver, inbound_sender).await });

    let command = next_frame(&mut inbound).await;
    let ServerFrame::Command(command) = command else {
        panic!("expected queued session.start command");
    };
    assert!(command.server_command_seq > 0);
    let session_id = command.session_id.clone();
    let Command::SessionStart(start) = &command.command else {
        panic!("expected session.start command");
    };
    assert_eq!(start.requirement.id, requirement_id);
    assert!(!start.conversation.excerpt.is_empty());
    assert!(start.repositories.is_empty() || !start.repositories[0].repository_id.is_empty());

    outbound
        .send(DaemonFrame::CommandAck(CommandAck {
            command_id: command.command_id.clone(),
            session_id: session_id.clone(),
            server_command_seq: command.server_command_seq,
            schema_version: SCHEMA_VERSION,
        }))
        .await
        .expect("send canonical command ACK");

    send_event_and_expect_ack(
        &outbound,
        &mut inbound,
        event(
            &session_id,
            "release-session-started",
            1,
            Event::SessionStarted(SessionStarted {
                runtime_id: "release-runtime".into(),
            }),
        ),
    )
    .await;
    send_event_and_expect_ack(
        &outbound,
        &mut inbound,
        event(
            &session_id,
            "release-agent-message",
            2,
            Event::AgentMessage(AgentMessage {
                message_id: "release-agent-message".into(),
                content: "Qualification clarification complete.".into(),
            }),
        ),
    )
    .await;
    send_event_and_expect_ack(
        &outbound,
        &mut inbound,
        event(
            &session_id,
            "release-agent-activity",
            3,
            Event::AgentActivity(AgentActivity {
                activity: "qualification-complete".into(),
            }),
        ),
    )
    .await;
    let assessment = event(
        &session_id,
        "release-assessment",
        4,
        Event::RequirementAssessed(RequirementAssessed {
            requirement_id: requirement_id.clone(),
            requirement_revision: start.requirement.revision,
            verdict: ReadinessVerdictWire::Ready,
            blockers: vec![],
            assumptions: vec![],
            repositories_reviewed: vec![],
        }),
    );
    send_event_and_expect_ack(&outbound, &mut inbound, assessment.clone()).await;
    send_event_and_expect_ack(&outbound, &mut inbound, assessment).await;
    send_event_and_expect_ack(
        &outbound,
        &mut inbound,
        event(
            &session_id,
            "release-session-completed",
            5,
            Event::SessionCompleted(SessionCompleted {
                summary: "Qualification complete.".into(),
            }),
        ),
    )
    .await;

    // Exact duplicate above is idempotent; changed payload for its stale identity is terminal.
    outbound
        .send(DaemonFrame::Event(event(
            &session_id,
            "release-assessment",
            4,
            Event::RequirementAssessed(RequirementAssessed {
                requirement_id: requirement_id.clone(),
                requirement_revision: start.requirement.revision,
                verdict: ReadinessVerdictWire::Ready,
                blockers: vec![],
                assumptions: vec!["conflicting duplicate payload".into()],
                repositories_reviewed: vec![],
            }),
        )))
        .await
        .expect("send conflicting duplicate assessment");
    let error = task
        .await
        .expect("join release WSS supervisor")
        .expect_err("conflicting duplicate event must terminate the supervisor");
    match error {
        ConnectionError::TerminalProtocol { code, .. } => {
            assert_eq!(code, "event_identity_conflict");
        }
        other => panic!("expected terminal identity conflict, got {other:?}"),
    }
}
