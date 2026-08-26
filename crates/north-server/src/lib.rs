//! north-server owns business coordination and exposes Axum HTTP/WebSocket
//! transport adapters. WebSocket messages are North JSON frames; application
//! coordination stays outside the handler.

pub mod assessment;
pub mod context;
pub mod transport;

pub use assessment::readiness_assessment_from_wire;

pub use context::{
    assemble_session_start, ConversationMessageSnapshot, ConversationRole, RepositorySnapshot,
    RequirementSnapshot,
};
