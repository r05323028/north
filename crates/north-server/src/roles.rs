use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch},
    Extension, Router,
};
use north_domain::role::{assign_role as domain_assign_role, Role, RoleError};
use north_persistence::{PersistenceError, UserRecord};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthState, CurrentUser};

/// Authorization failure returned by role-aware HTTP handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleHttpError {
    PermissionDenied,
    SelfModification,
    OwnerGrantRequiresOwner,
    NotAuthorized,
    InvalidRole,
    UserNotFound,
    Internal,
}

impl RoleHttpError {
    fn status(self) -> StatusCode {
        match self {
            Self::SelfModification | Self::InvalidRole => StatusCode::BAD_REQUEST,
            Self::PermissionDenied | Self::OwnerGrantRequiresOwner | Self::NotAuthorized => {
                StatusCode::FORBIDDEN
            }
            Self::UserNotFound => StatusCode::NOT_FOUND,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::SelfModification => "self_modification",
            Self::OwnerGrantRequiresOwner => "owner_grant_requires_owner",
            Self::NotAuthorized => "not_authorized",
            Self::InvalidRole => "invalid_role",
            Self::UserNotFound => "user_not_found",
            Self::Internal => "internal_error",
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for RoleHttpError {
    fn into_response(self) -> Response {
        (self.status(), Json(ErrorBody { error: self.code() })).into_response()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub role: String,
}

impl From<UserRecord> for UserResponse {
    fn from(user: UserRecord) -> Self {
        Self {
            id: user.id,
            email: user.email,
            role: role_name(user.role).into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssignRoleRequest {
    pub role: String,
}

/// Require a role allowed to make human review decisions.
pub fn require_review(user: &CurrentUser) -> Result<(), RoleHttpError> {
    user.user()
        .role
        .can_review()
        .then_some(())
        .ok_or(RoleHttpError::PermissionDenied)
}

/// Require an Owner or Admin for instance administration.
pub fn require_admin(user: &CurrentUser) -> Result<(), RoleHttpError> {
    user.user()
        .role
        .can_administer()
        .then_some(())
        .ok_or(RoleHttpError::PermissionDenied)
}

/// Apply API-boundary authorization before persistence changes a target role.
pub fn authorize_role_assignment(
    actor: &CurrentUser,
    target_id: &str,
    new_role: Role,
) -> Result<(), RoleHttpError> {
    require_admin(actor)?;
    domain_assign_role(actor.user().role, actor.user().id == target_id, new_role)?;
    Ok(())
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "Owner",
        Role::Admin => "Admin",
        Role::RequirementManager => "RequirementManager",
        Role::Requester => "Requester",
    }
}

fn parse_role(value: &str) -> Result<Role, RoleHttpError> {
    match value {
        "Owner" => Ok(Role::Owner),
        "Admin" => Ok(Role::Admin),
        "RequirementManager" => Ok(Role::RequirementManager),
        "Requester" => Ok(Role::Requester),
        _ => Err(RoleHttpError::InvalidRole),
    }
}

impl From<RoleError> for RoleHttpError {
    fn from(error: RoleError) -> Self {
        match error {
            RoleError::SelfModification => Self::SelfModification,
            RoleError::OwnerGrantRequiresOwner => Self::OwnerGrantRequiresOwner,
            RoleError::NotAuthorized => Self::NotAuthorized,
        }
    }
}

fn persistence_error(_: PersistenceError) -> RoleHttpError {
    RoleHttpError::Internal
}

/// Protected role-aware HTTP routes. Auth middleware is applied by `auth::router`.
pub fn router() -> Router<AuthState> {
    Router::new()
        .route("/auth/me", get(current_user))
        .route("/users", get(list_users))
        .route("/users/{user_id}/role", patch(assign_user_role))
}

pub async fn current_user(Extension(user): Extension<CurrentUser>) -> Json<UserResponse> {
    Json(user.0.into())
}

pub async fn list_users(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<UserResponse>>, RoleHttpError> {
    require_admin(&user)?;
    let users = state
        .store()
        .list_users()
        .await
        .map_err(persistence_error)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(users))
}

pub async fn assign_user_role(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(target_id): Path<String>,
    Json(payload): Json<AssignRoleRequest>,
) -> Result<Json<UserResponse>, RoleHttpError> {
    require_admin(&actor)?;
    let new_role = parse_role(&payload.role)?;
    authorize_role_assignment(&actor, &target_id, new_role)?;
    let target = state
        .store()
        .user_by_id(&target_id)
        .await
        .map_err(persistence_error)?
        .ok_or(RoleHttpError::UserNotFound)?;
    let updated = state
        .store()
        .update_user_role(&target.id, new_role)
        .await
        .map_err(persistence_error)?
        .ok_or(RoleHttpError::UserNotFound)?;
    Ok(Json(updated.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role) -> CurrentUser {
        CurrentUser(UserRecord {
            id: "user-1".into(),
            email: "user@example.com".into(),
            role,
        })
    }

    #[test]
    fn guards_use_domain_permission_matrices() {
        assert!(require_review(&user(Role::RequirementManager)).is_ok());
        assert!(require_review(&user(Role::Requester)).is_err());
        assert!(require_admin(&user(Role::Admin)).is_ok());
        assert!(require_admin(&user(Role::RequirementManager)).is_err());
    }

    #[test]
    fn assignment_errors_remain_distinct_at_http_boundary() {
        assert_eq!(
            RoleHttpError::from(RoleError::SelfModification).code(),
            "self_modification"
        );
        assert_eq!(
            RoleHttpError::from(RoleError::OwnerGrantRequiresOwner).code(),
            "owner_grant_requires_owner"
        );
        assert_eq!(
            RoleHttpError::from(RoleError::NotAuthorized).code(),
            "not_authorized"
        );
    }

    #[test]
    fn public_role_names_match_persisted_values() {
        assert_eq!(role_name(Role::Owner), "Owner");
        assert_eq!(role_name(Role::RequirementManager), "RequirementManager");
    }
}
