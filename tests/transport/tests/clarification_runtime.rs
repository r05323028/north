use north_daemon::{
    runtime::PiClarificationAdapter, DaemonCoordinator, Journal, RepositoryInspector,
};
use north_protocol::{
    Command, CommandEnvelope, DaemonFrame, Event, MessageSend, ServerFrame, SessionCancel,
    SCHEMA_VERSION,
};
use north_server::context::{
    assemble_session_start, ConversationMessageSnapshot, ConversationRole, RepositorySnapshot,
    RequirementSnapshot,
};
use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::tempdir;

#[test]
fn server_context_reaches_pi_adapter_through_daemon_coordinator(
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempdir()?;
    let inspector = RepositoryInspector::new(
        directory.path().join("cache"),
        directory.path().join("workspaces"),
    )?;
    let fake_pi = directory.path().join("fake-pi");
    fs::write(
        &fake_pi,
        r##"#!/bin/sh
last=""
for argument do last="$argument"; done
printf '%s' "$last" > "$0.prompt"
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"{\"message\":\"Need scope\",\"verdict\":\"needs_clarification\",\"blockers\":[\"scope\"],\"assumptions\":[\"context\"]}"}}'
"##,
    )?;
    let mut permissions = fs::metadata(&fake_pi)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&fake_pi, permissions)?;

    let start = assemble_session_start(
        RequirementSnapshot {
            id: "requirement-1".into(),
            revision: 1,
            title: "Title".into(),
            description: "Description".into(),
            summary: "Summary".into(),
            acceptance_criteria: vec!["Criterion".into()],
            assumptions: vec!["Assumption".into()],
            open_questions: vec!["Question".into()],
        },
        vec![ConversationMessageSnapshot {
            message_id: "message-1".into(),
            role: ConversationRole::Requester,
            content: "Clarify".into(),
        }],
        vec![RepositorySnapshot {
            repository_id: "disabled".into(),
            name: "Disabled".into(),
            url: "https://example.test/disabled.git".into(),
            description: "Must not cross the disabled boundary".into(),
            enabled: false,
        }],
    )?;
    assert!(start.repositories.is_empty());
    assert_eq!(start.conversation.excerpt[0].message_id, "message-1");

    let prompt_log = fake_pi.with_extension("prompt");
    let adapter = PiClarificationAdapter::new(inspector, directory.path().join("sessions"))?
        .with_agent_command(fake_pi);
    let coordinator = DaemonCoordinator::new(
        Journal::open(directory.path().join("journal.json"), "daemon-1")?,
        adapter,
    );
    let frames = coordinator.process_server_frame(ServerFrame::Command(CommandEnvelope {
        command_id: "command-1".into(),
        session_id: "session-1".into(),
        server_command_seq: 1,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        command: Command::SessionStart(start),
    }))?;

    assert!(matches!(frames.first(), Some(DaemonFrame::CommandAck(_))));
    let events = frames
        .into_iter()
        .filter_map(|frame| match frame {
            DaemonFrame::Event(event) => Some(event.event),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(events[0], Event::SessionStarted(_)));
    assert!(matches!(events[1], Event::AgentActivity(_)));
    assert!(matches!(events[2], Event::AgentMessage(_)));
    assert!(matches!(events[3], Event::RequirementAssessed(_)));
    assert_eq!(events.len(), 4);

    let message_frames =
        coordinator.process_server_frame(ServerFrame::Command(CommandEnvelope {
            command_id: "command-2".into(),
            session_id: "session-1".into(),
            server_command_seq: 2,
            sent_at: "2026-01-01T00:00:01Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::MessageSend(MessageSend {
                message_id: "message-2".into(),
                content: "Add scope".into(),
            }),
        }))?;
    let message_events = message_frames
        .into_iter()
        .filter_map(|frame| match frame {
            DaemonFrame::Event(event) => Some(event.event),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(matches!(message_events[0], Event::AgentActivity(_)));
    assert!(matches!(message_events[1], Event::AgentMessage(_)));
    assert!(matches!(message_events[2], Event::RequirementAssessed(_)));
    assert_eq!(message_events.len(), 3);

    let cancel_frames =
        coordinator.process_server_frame(ServerFrame::Command(CommandEnvelope {
            command_id: "command-3".into(),
            session_id: "session-1".into(),
            server_command_seq: 3,
            sent_at: "2026-01-01T00:00:02Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::SessionCancel(SessionCancel {
                reason: "requester_cancelled".into(),
            }),
        }))?;
    assert!(cancel_frames.iter().any(|frame| matches!(
        frame,
        DaemonFrame::Event(event) if matches!(&event.event, Event::SessionCompleted(_))
    )));
    let prompt = fs::read_to_string(prompt_log)?;
    assert!(prompt.contains("requirement-1"));
    assert!(prompt.contains("message-1"));
    assert!(prompt.contains("Clarify"));
    Ok(())
}
