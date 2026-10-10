use axum::{
    extract::{rejection::JsonRejection, Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use north_domain::status::RequirementStatus;
use north_persistence::{
    PersistenceError, RequirementSort, RequirementWorkspacePreferences, RequirementWorkspaceView,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthState, CurrentUser};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug)]
enum PreferenceHttpError {
    BadRequest,
    Internal,
}

impl IntoResponse for PreferenceHttpError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl From<PersistenceError> for PreferenceHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::InvalidUserPreferences => Self::BadRequest,
            _ => Self::Internal,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NullableString {
    Text(String),
    Null(()),
}

impl NullableString {
    fn into_option(self) -> Option<String> {
        match self {
            Self::Text(value) => Some(value),
            Self::Null(()) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    default_view: String,
    list_status: NullableString,
    list_created_by: NullableString,
    list_sort: String,
}

impl UpdateRequest {
    fn into_preferences(self) -> Result<RequirementWorkspacePreferences, PreferenceHttpError> {
        let default_view = match self.default_view.as_str() {
            "board" => RequirementWorkspaceView::Board,
            "list" => RequirementWorkspaceView::List,
            _ => return Err(PreferenceHttpError::BadRequest),
        };
        let list_status = self
            .list_status
            .into_option()
            .map(|status| match status.as_str() {
                "draft" => Ok(RequirementStatus::Draft),
                "discussing" => Ok(RequirementStatus::Discussing),
                "ready" => Ok(RequirementStatus::Ready),
                "accepted" => Ok(RequirementStatus::Accepted),
                "rejected" => Ok(RequirementStatus::Rejected),
                _ => Err(PreferenceHttpError::BadRequest),
            })
            .transpose()?;
        let list_created_by = self
            .list_created_by
            .into_option()
            .map(|value| {
                let value = value.trim();
                if value.is_empty() || value.len() > 256 {
                    return Err(PreferenceHttpError::BadRequest);
                }
                Ok(value.to_owned())
            })
            .transpose()?;
        let list_sort = match self.list_sort.as_str() {
            "updated" => RequirementSort::UpdatedDescending,
            "updated_asc" => RequirementSort::UpdatedAscending,
            _ => return Err(PreferenceHttpError::BadRequest),
        };

        Ok(RequirementWorkspacePreferences {
            default_view,
            list_status,
            list_created_by,
            list_sort,
        })
    }
}

#[derive(Debug, Serialize)]
struct PreferenceResponse {
    default_view: &'static str,
    list_status: Option<&'static str>,
    list_created_by: Option<String>,
    list_sort: &'static str,
}

impl TryFrom<RequirementWorkspacePreferences> for PreferenceResponse {
    type Error = PreferenceHttpError;

    fn try_from(preferences: RequirementWorkspacePreferences) -> Result<Self, Self::Error> {
        let list_sort = match preferences.list_sort {
            RequirementSort::UpdatedDescending => "updated",
            RequirementSort::UpdatedAscending => "updated_asc",
            RequirementSort::Board => return Err(PreferenceHttpError::Internal),
        };
        Ok(Self {
            default_view: preferences.default_view.as_str(),
            list_status: preferences.list_status.map(RequirementStatus::as_str),
            list_created_by: preferences.list_created_by,
            list_sort,
        })
    }
}

pub fn router() -> Router<AuthState> {
    Router::new().route(
        "/auth/me/preferences/requirement-workspace",
        get(get_preferences).put(put_preferences),
    )
}

async fn get_preferences(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<PreferenceResponse>, PreferenceHttpError> {
    let preferences = state
        .store()
        .get_requirement_workspace_preferences(&user.user().id)
        .await?;
    Ok(Json(preferences.try_into()?))
}

async fn put_preferences(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    payload: Result<Json<UpdateRequest>, JsonRejection>,
) -> Result<Json<PreferenceResponse>, PreferenceHttpError> {
    let Json(payload) = payload.map_err(|_| PreferenceHttpError::BadRequest)?;
    let preferences = payload.into_preferences()?;
    state
        .store()
        .save_requirement_workspace_preferences(&user.user().id, &preferences)
        .await?;
    Ok(Json(preferences.try_into()?))
}
