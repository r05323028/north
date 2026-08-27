//! north-server owns business coordination and exposes Axum HTTP/WebSocket
//! transport adapters. WebSocket messages are North JSON frames; application
//! coordination stays outside the handler.

pub mod assessment;
pub mod auth;
pub mod context;
pub mod daemon;
pub mod roles;
pub mod transport;

pub use assessment::readiness_assessment_from_wire;
pub use auth::{
    auth_middleware, router as auth_router, AuthState, CodeDelivery, CurrentUser, DeliveryError,
    LogCodeDelivery, RequestCodeRequest, VerifyCodeRequest, CODE_REQUEST_MIN_INTERVAL_SECONDS,
};
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

/// Build authenticated HTTP routes only after startup migrations succeed.
pub async fn build_app(
    pool: north_persistence::DatabasePool,
    delivery: std::sync::Arc<dyn CodeDelivery>,
) -> Result<axum::Router, north_persistence::MigrationError> {
    run_migrations(&pool).await?;
    Ok(auth_router(AuthState::new(
        north_persistence::AuthStore::new(pool),
        delivery,
    )))
}

pub use context::{
    assemble_session_start, ConversationMessageSnapshot, ConversationRole, RepositorySnapshot,
    RequirementSnapshot,
};
pub use daemon::{DaemonDispatchError, DaemonHttpError, DaemonResponse, DaemonRuntime};
