use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use north_persistence::{PersistenceError, RepositoryRecord};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthState, CurrentUser},
    roles::require_admin,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<&'static str>,
}

#[derive(Debug)]
pub enum RepositoryHttpError {
    BadRequest,
    Forbidden,
    NotFound,
    Conflict {
        repository_id: Option<String>,
        action: Option<&'static str>,
    },
    Internal,
}

impl RepositoryHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Forbidden => "permission_denied",
            Self::NotFound => "not_found",
            Self::Conflict { .. } => "repository_conflict",
            Self::Internal => "internal_error",
        }
    }
}

impl IntoResponse for RepositoryHttpError {
    fn into_response(self) -> Response {
        let (repository_id, action) = match &self {
            Self::Conflict {
                repository_id,
                action,
            } => (repository_id.clone(), *action),
            _ => (None, None),
        };
        (
            self.status(),
            Json(ErrorBody {
                error: self.code(),
                repository_id,
                action,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepositoryResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub disabled_at: Option<String>,
    pub enabled: bool,
}

impl From<RepositoryRecord> for RepositoryResponse {
    fn from(repository: RepositoryRecord) -> Self {
        let enabled = repository.enabled();
        Self {
            id: repository.id,
            name: repository.name,
            url: repository.url,
            description: repository.description,
            created_at: repository.created_at,
            updated_at: repository.updated_at,
            enabled,
            disabled_at: repository.disabled_at,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRepositoryRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
}

pub fn router() -> Router<AuthState> {
    Router::new()
        .route(
            "/repositories",
            get(list_repositories).post(create_repository),
        )
        .route(
            "/repositories/{repository_id}",
            get(get_repository).patch(update_repository),
        )
        .route(
            "/repositories/{repository_id}/disable",
            post(disable_repository),
        )
        .route(
            "/repositories/{repository_id}/re-enable",
            post(reenable_repository),
        )
        .route(
            "/repositories/{repository_id}/reenable",
            post(reenable_repository),
        )
}

pub async fn list_repositories(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<RepositoryResponse>>, RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    Ok(Json(
        state
            .store()
            .list_repositories()
            .await
            .map_err(map_persistence_error)?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

pub async fn create_repository(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Json(payload): Json<CreateRepositoryRequest>,
) -> Result<(StatusCode, Json<RepositoryResponse>), RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    let repository = match state
        .store()
        .create_repository(&payload.name, &payload.url, &payload.description)
        .await
    {
        Ok(repository) => repository,
        Err(PersistenceError::RepositoryNameConflict) => {
            let existing = state
                .store()
                .repository_by_normalized_name(&payload.name)
                .await
                .map_err(map_persistence_error)?;
            let (repository_id, action) = match existing {
                Some(repository) if repository.disabled_at.is_some() => {
                    (Some(repository.id), "re_enable")
                }
                _ => (None, "choose_another_name"),
            };
            return Err(RepositoryHttpError::Conflict {
                repository_id,
                action: Some(action),
            });
        }
        Err(error) => return Err(map_persistence_error(error)),
    };
    Ok((StatusCode::CREATED, Json(repository.into())))
}

pub async fn get_repository(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(repository_id): Path<String>,
) -> Result<Json<RepositoryResponse>, RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    let repository = state
        .store()
        .repository_by_id(&repository_id)
        .await
        .map_err(map_persistence_error)?
        .ok_or(RepositoryHttpError::NotFound)?;
    Ok(Json(repository.into()))
}

pub async fn update_repository(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(repository_id): Path<String>,
    Json(payload): Json<UpdateRepositoryRequest>,
) -> Result<Json<RepositoryResponse>, RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    let repository = match state
        .store()
        .update_repository_fields(
            &repository_id,
            payload.name.as_deref(),
            payload.description.as_deref(),
            payload.url.as_deref(),
        )
        .await
    {
        Ok(repository) => repository,
        Err(PersistenceError::RepositoryNameConflict) => {
            return Err(RepositoryHttpError::Conflict {
                repository_id: None,
                action: Some("choose_another_name"),
            });
        }
        Err(PersistenceError::RepositoryUrlImmutable) => {
            return Err(RepositoryHttpError::Conflict {
                repository_id: Some(repository_id),
                action: Some("disable_old_create_new"),
            });
        }
        Err(error) => return Err(map_persistence_error(error)),
    };
    Ok(Json(repository.into()))
}

pub async fn disable_repository(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(repository_id): Path<String>,
) -> Result<Json<RepositoryResponse>, RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    Ok(Json(
        state
            .store()
            .disable_repository(&repository_id)
            .await
            .map_err(map_persistence_error)?
            .into(),
    ))
}

pub async fn reenable_repository(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(repository_id): Path<String>,
) -> Result<Json<RepositoryResponse>, RepositoryHttpError> {
    require_admin(&user).map_err(|_| RepositoryHttpError::Forbidden)?;
    Ok(Json(
        state
            .store()
            .reenable_repository(&repository_id)
            .await
            .map_err(map_persistence_error)?
            .into(),
    ))
}

fn map_persistence_error(error: PersistenceError) -> RepositoryHttpError {
    match error {
        PersistenceError::InvalidRepository(_) => RepositoryHttpError::BadRequest,
        PersistenceError::RepositoryNotFound => RepositoryHttpError::NotFound,
        PersistenceError::RepositoryNameConflict => RepositoryHttpError::Conflict {
            repository_id: None,
            action: Some("re_enable"),
        },
        PersistenceError::RepositoryUrlImmutable => RepositoryHttpError::Conflict {
            repository_id: None,
            action: Some("disable_old_create_new"),
        },
        PersistenceError::Database(_) => RepositoryHttpError::Internal,
        _ => RepositoryHttpError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_domain::role::Role;
    use north_persistence::UserRecord;

    fn user(role: Role) -> CurrentUser {
        CurrentUser(UserRecord {
            id: "user-1".into(),
            email: "user@example.com".into(),
            role,
        })
    }

    #[test]
    fn only_admin_roles_pass_repository_guard() {
        assert!(require_admin(&user(Role::Owner)).is_ok());
        assert!(require_admin(&user(Role::Admin)).is_ok());
        assert!(require_admin(&user(Role::RequirementManager)).is_err());
        assert!(require_admin(&user(Role::Requester)).is_err());
    }

    #[test]
    fn responses_expose_lifecycle_without_credentials() {
        let response = RepositoryResponse::from(RepositoryRecord {
            id: "repo-1".into(),
            name: "North".into(),
            name_normalized: "north".into(),
            url: "https://example.test/north.git".into(),
            description: String::new(),
            created_at: "created".into(),
            updated_at: "updated".into(),
            disabled_at: None,
        });
        let json = serde_json::to_string(&response).expect("serialize repository");
        assert!(json.contains("enabled"));
        assert!(!json.contains("credential"));
        assert!(!json.contains("password"));
    }
}
