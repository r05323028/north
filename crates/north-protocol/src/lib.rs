//! North 0.1 wire types and JSON codec.
//!
//! `north-protocol` is deliberately transport/framework independent. It owns
//! the application wire contract; Axum and tokio-tungstenite stop at their
//! transport adapters. North 0.1 serializes these values as JSON text messages.
//!
//! Direction is part of a message's identity. Server commands are sent only by
//! the server, daemon events only by the daemon, and connection/control frames
//! are explicit enum variants rather than transport-specific messages.

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub const PROTOCOL_VERSION: &str = "0.1";
pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug)]
pub enum FrameError {
    Json(serde_json::Error),
    Validation(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "North JSON codec error: {error}"),
            Self::Validation(reason) => write!(f, "invalid North frame: {reason}"),
        }
    }
}

impl Error for FrameError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

impl From<serde_json::Error> for FrameError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

fn non_empty(field: &str, value: &str) -> Result<(), FrameError> {
    if value.trim().is_empty() {
        return Err(FrameError::Validation(format!("{field} must not be empty")));
    }
    Ok(())
}

fn schema(version: u16) -> Result<(), FrameError> {
    if version != SCHEMA_VERSION {
        return Err(FrameError::Validation(format!(
            "unsupported schema_version {version}; expected {SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

fn protocol(version: &str) -> Result<(), FrameError> {
    if version != PROTOCOL_VERSION {
        return Err(FrameError::Validation(format!(
            "unsupported protocol_version {version:?}; expected {PROTOCOL_VERSION:?}"
        )));
    }
    Ok(())
}

fn sequence(field: &str, value: u64) -> Result<(), FrameError> {
    if value == 0 {
        return Err(FrameError::Validation(format!("{field} must start at 1")));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: String,
    pub schema_version: u16,
    pub daemon_id: String,
    /// User-owned daemon credential. Adapters must never log this value.
    pub credential: String,
    pub capabilities: Vec<String>,
}

impl fmt::Debug for Hello {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hello")
            .field("protocol_version", &self.protocol_version)
            .field("schema_version", &self.schema_version)
            .field("daemon_id", &self.daemon_id)
            .field("credential", &"<redacted>")
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl Hello {
    pub fn new(
        daemon_id: impl Into<String>,
        credential: impl Into<String>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.into(),
            schema_version: SCHEMA_VERSION,
            daemon_id: daemon_id.into(),
            credential: credential.into(),
            capabilities,
        }
    }

    fn validate(&self) -> Result<(), FrameError> {
        protocol(&self.protocol_version)?;
        schema(self.schema_version)?;
        non_empty("daemon_id", &self.daemon_id)?;
        non_empty("credential", &self.credential)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol_version: String,
    pub schema_version: u16,
    pub daemon_id: String,
    pub server_time: String,
}

impl Welcome {
    fn validate(&self) -> Result<(), FrameError> {
        protocol(&self.protocol_version)?;
        schema(self.schema_version)?;
        non_empty("daemon_id", &self.daemon_id)?;
        non_empty("server_time", &self.server_time)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub schema_version: u16,
    pub daemon_id: String,
    pub sent_at: String,
    /// Application liveness/state summary, not a Requirement lifecycle state.
    pub application_state: String,
}

impl Heartbeat {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("daemon_id", &self.daemon_id)?;
        non_empty("sent_at", &self.sent_at)?;
        non_empty("application_state", &self.application_state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStart {
    pub requirement_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCancel {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResume {
    pub last_event_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSend {
    pub message_id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Command {
    #[serde(rename = "session.start")]
    SessionStart(SessionStart),
    #[serde(rename = "session.cancel")]
    SessionCancel(SessionCancel),
    #[serde(rename = "session.resume")]
    SessionResume(SessionResume),
    #[serde(rename = "message.send")]
    MessageSend(MessageSend),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub command_id: String,
    pub session_id: String,
    pub server_command_seq: u64,
    pub sent_at: String,
    pub schema_version: u16,
    pub command: Command,
}

impl CommandEnvelope {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("command_id", &self.command_id)?;
        non_empty("session_id", &self.session_id)?;
        non_empty("sent_at", &self.sent_at)?;
        sequence("server_command_seq", self.server_command_seq)?;
        match &self.command {
            Command::SessionStart(payload) => non_empty("requirement_id", &payload.requirement_id),
            Command::SessionCancel(payload) => non_empty("reason", &payload.reason),
            Command::SessionResume(_) => Ok(()),
            Command::MessageSend(payload) => {
                non_empty("message_id", &payload.message_id)?;
                non_empty("content", &payload.content)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStarted {
    pub runtime_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub message_id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActivity {
    pub activity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementAssessed {
    pub requirement_id: String,
    pub requirement_revision: u64,
    pub assessment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCompleted {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFailed {
    pub recoverable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    #[serde(rename = "session.started")]
    SessionStarted(SessionStarted),
    #[serde(rename = "agent.message")]
    AgentMessage(AgentMessage),
    #[serde(rename = "agent.activity")]
    AgentActivity(AgentActivity),
    #[serde(rename = "requirement.assessed")]
    RequirementAssessed(RequirementAssessed),
    #[serde(rename = "session.completed")]
    SessionCompleted(SessionCompleted),
    #[serde(rename = "session.failed")]
    SessionFailed(SessionFailed),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub sent_at: String,
    pub schema_version: u16,
    pub event: Event,
}

impl EventEnvelope {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("event_id", &self.event_id)?;
        non_empty("session_id", &self.session_id)?;
        non_empty("sent_at", &self.sent_at)?;
        sequence("daemon_event_seq", self.daemon_event_seq)?;
        match &self.event {
            Event::SessionStarted(payload) => non_empty("runtime_id", &payload.runtime_id),
            Event::AgentMessage(payload) => {
                non_empty("message_id", &payload.message_id)?;
                non_empty("content", &payload.content)
            }
            Event::AgentActivity(payload) => non_empty("activity", &payload.activity),
            Event::RequirementAssessed(payload) => {
                non_empty("requirement_id", &payload.requirement_id)?;
                sequence("requirement_revision", payload.requirement_revision)?;
                non_empty("assessment", &payload.assessment)
            }
            Event::SessionCompleted(payload) => non_empty("summary", &payload.summary),
            Event::SessionFailed(payload) => non_empty("reason", &payload.reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAck {
    pub command_id: String,
    pub session_id: String,
    pub server_command_seq: u64,
    pub schema_version: u16,
}

impl CommandAck {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("command_id", &self.command_id)?;
        non_empty("session_id", &self.session_id)?;
        sequence("server_command_seq", self.server_command_seq)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventAckStatus {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "rejected")]
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventAck {
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub schema_version: u16,
    pub status: EventAckStatus,
    pub reason: Option<String>,
}

impl EventAck {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("event_id", &self.event_id)?;
        non_empty("session_id", &self.session_id)?;
        sequence("daemon_event_seq", self.daemon_event_seq)?;
        if matches!(self.status, EventAckStatus::Rejected)
            && self.reason.as_deref().is_none_or(str::is_empty)
        {
            return Err(FrameError::Validation(
                "rejected event ACK must include reason".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileState {
    pub schema_version: u16,
    pub session_id: String,
    pub command_ack_through_seq: u64,
    pub event_ack_through_seq: u64,
    pub event_ack_sparse: Vec<u64>,
}

impl ReconcileState {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("session_id", &self.session_id)?;
        if self
            .event_ack_sparse
            .iter()
            .any(|sequence| *sequence <= self.event_ack_through_seq)
        {
            return Err(FrameError::Validation(
                "sparse acknowledgements must be above event_ack_through_seq".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolErrorFrame {
    pub schema_version: u16,
    pub code: String,
    pub message: String,
    pub fatal: bool,
}

impl ProtocolErrorFrame {
    fn validate(&self) -> Result<(), FrameError> {
        schema(self.schema_version)?;
        non_empty("code", &self.code)?;
        non_empty("message", &self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "frame", content = "payload")]
pub enum ServerFrame {
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "command")]
    Command(CommandEnvelope),
    #[serde(rename = "event_ack")]
    EventAck(EventAck),
    #[serde(rename = "reconcile")]
    Reconcile(ReconcileState),
    #[serde(rename = "protocol_error")]
    ProtocolError(ProtocolErrorFrame),
}

impl ServerFrame {
    pub fn validate(&self) -> Result<(), FrameError> {
        match self {
            Self::Welcome(frame) => frame.validate(),
            Self::Command(frame) => frame.validate(),
            Self::EventAck(frame) => frame.validate(),
            Self::Reconcile(frame) => frame.validate(),
            Self::ProtocolError(frame) => frame.validate(),
        }
    }

    pub fn to_json(&self) -> Result<String, FrameError> {
        self.validate()?;
        serde_json::to_string(self).map_err(FrameError::from)
    }

    pub fn from_json(text: &str) -> Result<Self, FrameError> {
        let frame: ServerFrame = serde_json::from_str(text)?;
        frame.validate()?;
        Ok(frame)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "frame", content = "payload")]
pub enum DaemonFrame {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "heartbeat")]
    Heartbeat(Heartbeat),
    #[serde(rename = "event")]
    Event(EventEnvelope),
    #[serde(rename = "command_ack")]
    CommandAck(CommandAck),
}

impl DaemonFrame {
    pub fn validate(&self) -> Result<(), FrameError> {
        match self {
            Self::Hello(frame) => frame.validate(),
            Self::Heartbeat(frame) => frame.validate(),
            Self::Event(frame) => frame.validate(),
            Self::CommandAck(frame) => frame.validate(),
        }
    }

    pub fn to_json(&self) -> Result<String, FrameError> {
        self.validate()?;
        serde_json::to_string(self).map_err(FrameError::from)
    }

    pub fn from_json(text: &str) -> Result<Self, FrameError> {
        let frame: DaemonFrame = serde_json::from_str(text)?;
        frame.validate()?;
        Ok(frame)
    }
}

pub fn encode_server_frame(frame: &ServerFrame) -> Result<String, FrameError> {
    frame.to_json()
}

pub fn decode_server_frame(text: &str) -> Result<ServerFrame, FrameError> {
    ServerFrame::from_json(text)
}

pub fn encode_daemon_frame(frame: &DaemonFrame) -> Result<String, FrameError> {
    frame.to_json()
}

pub fn decode_daemon_frame(text: &str) -> Result<DaemonFrame, FrameError> {
    DaemonFrame::from_json(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Hello {
        Hello::new("daemon-1", "credential", vec!["agent".into()])
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
                content: "clarify request".into(),
            }),
        }
    }

    fn event() -> EventEnvelope {
        EventEnvelope {
            event_id: "event-1".into(),
            session_id: "session-1".into(),
            daemon_event_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            event: Event::AgentActivity(AgentActivity {
                activity: "thinking".into(),
            }),
        }
    }

    #[test]
    fn every_frame_family_round_trips_as_json() {
        let server_frames = [
            ServerFrame::Welcome(Welcome {
                protocol_version: PROTOCOL_VERSION.into(),
                schema_version: SCHEMA_VERSION,
                daemon_id: "daemon-1".into(),
                server_time: "2026-01-01T00:00:00Z".into(),
            }),
            ServerFrame::Command(command()),
            ServerFrame::EventAck(EventAck {
                event_id: "event-1".into(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Accepted,
                reason: None,
            }),
            ServerFrame::Reconcile(ReconcileState {
                schema_version: SCHEMA_VERSION,
                session_id: "session-1".into(),
                command_ack_through_seq: 1,
                event_ack_through_seq: 1,
                event_ack_sparse: vec![3],
            }),
            ServerFrame::ProtocolError(ProtocolErrorFrame {
                schema_version: SCHEMA_VERSION,
                code: "unsupported_version".into(),
                message: "peer is too old".into(),
                fatal: true,
            }),
        ];
        for frame in server_frames {
            let json = frame.to_json().expect("valid server frame");
            assert_eq!(ServerFrame::from_json(&json).expect("round trip"), frame);
        }

        let daemon_frames = [
            DaemonFrame::Hello(hello()),
            DaemonFrame::Heartbeat(Heartbeat {
                schema_version: SCHEMA_VERSION,
                daemon_id: "daemon-1".into(),
                sent_at: "2026-01-01T00:00:00Z".into(),
                application_state: "connected".into(),
            }),
            DaemonFrame::Event(event()),
            DaemonFrame::CommandAck(CommandAck {
                command_id: "command-1".into(),
                session_id: "session-1".into(),
                server_command_seq: 1,
                schema_version: SCHEMA_VERSION,
            }),
        ];
        for frame in daemon_frames {
            let json = frame.to_json().expect("valid daemon frame");
            assert_eq!(DaemonFrame::from_json(&json).expect("round trip"), frame);
        }
    }

    #[test]
    fn invalid_version_and_unknown_frame_fail_before_dispatch() {
        let mut frame = hello();
        frame.schema_version = SCHEMA_VERSION + 1;
        assert!(DaemonFrame::Hello(frame).to_json().is_err());
        assert!(ServerFrame::from_json(r#"{"frame":"unknown","payload":{}}"#).is_err());
    }
}
