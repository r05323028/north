//! north-server owns business coordination and exposes Axum HTTP/WebSocket
//! transport adapters. WebSocket messages are North JSON frames; application
//! coordination stays outside the handler.

use std::{error::Error, fmt};

pub mod assessment;
pub mod auth;
pub mod context;
pub mod conversations;
pub mod daemon;
pub mod repositories;
pub mod requirements;
pub mod roles;
pub mod transport;

/// Convert wire readiness evidence through the server-owned domain boundary.
pub fn readiness_assessment_from_wire(
    wire: &north_protocol::RequirementAssessed,
    assessed_at_ms: u64,
) -> Result<north_domain::readiness::ReadinessAssessment, north_protocol::FrameError> {
    assessment::readiness_assessment_from_wire(wire, assessed_at_ms)
}
pub use auth::{
    auth_middleware, router as auth_router, AuthState, CodeDelivery, CurrentUser, DeliveryError,
    LogCodeDelivery, RequestCodeRequest, VerifyCodeRequest, CODE_REQUEST_MIN_INTERVAL_SECONDS,
};
pub use conversations::{ConversationHttpError, ConversationResponse, MessageResponse};
pub use repositories::{RepositoryHttpError, RepositoryResponse};
pub use requirements::{RequirementHttpError, RequirementResponse};
pub use roles::{
    assign_user_role, authorize_role_assignment, current_user, require_admin, require_review,
    RoleHttpError,
};

/// Run schema migrations as part of server startup.
pub async fn run_migrations(
    pool: &north_persistence::DatabasePool,
) -> Result<(), north_persistence::MigrationError> {
    north_persistence::run_migrations(pool).await
}

#[derive(Debug)]
pub enum BuildAppError {
    Migration(north_persistence::MigrationError),
    Startup(north_persistence::PersistenceError),
}

impl fmt::Display for BuildAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Migration(error) => write!(f, "run migrations: {error}"),
            Self::Startup(error) => write!(f, "initialize server state: {error}"),
        }
    }
}

impl Error for BuildAppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Migration(error) => Some(error),
            Self::Startup(error) => Some(error),
        }
    }
}

/// Build authenticated HTTP routes only after migrations and lease reset succeed.
pub async fn build_app(
    pool: north_persistence::DatabasePool,
    delivery: std::sync::Arc<dyn CodeDelivery>,
) -> Result<axum::Router, BuildAppError> {
    run_migrations(&pool)
        .await
        .map_err(BuildAppError::Migration)?;
    let store = north_persistence::AuthStore::new(pool);
    store
        .invalidate_daemon_connections()
        .await
        .map_err(BuildAppError::Startup)?;
    Ok(auth_router(AuthState::new(store, delivery)))
}

pub use context::{
    assemble_session_start, ConversationMessageSnapshot, ConversationRole, RepositorySnapshot,
    RequirementSnapshot,
};
pub use daemon::{
    CommandRequest, DaemonDispatchError, DaemonHttpError, DaemonResponse, DaemonRuntime,
    SetupApprovalResponse,
};
