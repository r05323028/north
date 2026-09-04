use axum::{
    extract::{Json, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use north_domain::{requirement::RequirementEdit, status::RequirementStatus};
use north_persistence::{
    RequirementError, RequirementListQuery, RequirementRecord, RequirementSort,
    RequirementTransition,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthState, CurrentUser},
    roles::require_review,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug)]
pub enum RequirementHttpError {
    BadRequest,
    Forbidden,
    NotFound,
    Conflict,
    Internal,
}

impl RequirementHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Forbidden => "permission_denied",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Internal => "internal_error",
        }
    }
}

impl IntoResponse for RequirementHttpError {
    fn into_response(self) -> Response {
        (self.status(), Json(ErrorBody { error: self.code() })).into_response()
    }
}

impl From<RequirementError> for RequirementHttpError {
    fn from(error: RequirementError) -> Self {
        match error {
            RequirementError::NotFound => Self::NotFound,
            RequirementError::Conflict => Self::Conflict,
            RequirementError::InvalidStatus(_)
            | RequirementError::InvalidTransition(_)
            | RequirementError::Edit(_) => Self::BadRequest,
            RequirementError::Database(_)
            | RequirementError::InvalidRevision
            | RequirementError::InvalidStateVersion => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequirementResponse {
    pub id: String,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub status: String,
    pub revision: u64,
    pub state_version: u64,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<RequirementRecord> for RequirementResponse {
    fn from(requirement: RequirementRecord) -> Self {
        Self {
            id: requirement.id,
            title: requirement.title,
            description: requirement.description,
            summary: requirement.summary,
            acceptance_criteria: requirement.acceptance_criteria,
            assumptions: requirement.assumptions,
            open_questions: requirement.open_questions,
            status: requirement.status.as_str().into(),
            revision: requirement.revision,
            state_version: requirement.state_version,
            created_by: requirement.created_by,
            created_at: requirement.created_at,
            updated_at: requirement.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRequirementRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct RequirementEditRequest {
    pub expected_state_version: Option<u64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub assumptions: Option<Vec<String>>,
    pub open_questions: Option<Vec<String>>,
}

impl RequirementEditRequest {
    fn into_domain(self) -> Result<(u64, RequirementEdit), RequirementHttpError> {
        let expected_state_version = self
            .expected_state_version
            .ok_or(RequirementHttpError::BadRequest)?;
        Ok((
            expected_state_version,
            RequirementEdit {
                title: normalize_edit_text(self.title, 500, false)?,
                description: normalize_edit_text(self.description, 10_000, false)?,
                summary: normalize_edit_text(self.summary, 10_000, true)?,
                acceptance_criteria: normalize_edit_list(self.acceptance_criteria, 100, 10_000)?,
                assumptions: normalize_edit_list(self.assumptions, 100, 10_000)?,
                open_questions: normalize_edit_list(self.open_questions, 100, 10_000)?,
            },
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct TransitionRequest {
    pub expected_state_version: Option<u64>,
    pub assessment_id: Option<String>,
    pub feedback: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RequirementQuery {
    #[serde(alias = "q")]
    pub search: Option<String>,
    pub status: Option<String>,
    pub created_by: Option<String>,
    pub sort: Option<String>,
}

/// Protected requirement CRUD and lifecycle routes.
pub fn router() -> Router<AuthState> {
    Router::new()
        .route(
            "/requirements",
            get(list_requirements).post(create_requirement),
        )
        .route(
            "/requirements/{requirement_id}",
            get(get_requirement).patch(edit_requirement),
        )
        .route(
            "/requirements/{requirement_id}/begin-discussion",
            post(begin_discussion),
        )
        .route("/requirements/{requirement_id}/accept", post(accept))
        .route("/requirements/{requirement_id}/reject", post(reject))
        .route(
            "/requirements/{requirement_id}/request-changes",
            post(request_changes),
        )
        .route("/requirements/{requirement_id}/reopen", post(reopen))
        .merge(crate::conversations::router())
        .merge(crate::assessment::router())
}

pub async fn create_requirement(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Json(payload): Json<CreateRequirementRequest>,
) -> Result<(StatusCode, Json<RequirementResponse>), RequirementHttpError> {
    let title = bounded_text(&payload.title, 500).ok_or(RequirementHttpError::BadRequest)?;
    let description =
        bounded_text(&payload.description, 10_000).ok_or(RequirementHttpError::BadRequest)?;
    let requirement = state
        .store()
        .create_requirement(&title, &description, &actor.user().id)
        .await
        .map_err(RequirementHttpError::from)?;
    state.events().requirement_changed(requirement.id.clone());
    Ok((StatusCode::CREATED, Json(requirement.into())))
}

pub async fn list_requirements(
    State(state): State<AuthState>,
    Query(query): Query<RequirementQuery>,
) -> Result<Json<Vec<RequirementResponse>>, RequirementHttpError> {
    let status = query.status.as_deref().map(parse_status).transpose()?;
    let sort = match query.sort.as_deref() {
        None | Some("updated") | Some("updated_desc") | Some("updated_at_desc") => {
            RequirementSort::UpdatedDescending
        }
        Some("updated_asc") | Some("updated_at_asc") => RequirementSort::UpdatedAscending,
        Some(_) => return Err(RequirementHttpError::BadRequest),
    };
    let records = state
        .store()
        .list_requirements(&RequirementListQuery {
            search: query.search,
            status,
            created_by: query.created_by,
            sort,
        })
        .await
        .map_err(RequirementHttpError::from)?;
    Ok(Json(records.into_iter().map(Into::into).collect()))
}

pub async fn get_requirement(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    let requirement = state
        .store()
        .requirement_by_id(&requirement_id)
        .await
        .map_err(RequirementHttpError::from)?
        .ok_or(RequirementHttpError::NotFound)?;
    Ok(Json(requirement.into()))
}

pub async fn edit_requirement(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<RequirementEditRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    let (expected_state_version, edit) = payload.into_domain()?;
    let requirement = state
        .store()
        .edit_requirement_with_actor(
            &requirement_id,
            expected_state_version,
            &actor.user().id,
            &edit,
        )
        .await
        .map_err(RequirementHttpError::from)?;
    if requirement.state_version > expected_state_version {
        state.events().requirement_changed(requirement.id.clone());
    }
    Ok(Json(requirement.into()))
}

async fn transition(
    state: AuthState,
    actor: CurrentUser,
    requirement_id: String,
    payload: TransitionRequest,
    operation: RequirementTransition,
    reviewer_only: bool,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    if reviewer_only {
        require_review(&actor).map_err(|_| RequirementHttpError::Forbidden)?;
    }
    let expected_state_version = payload
        .expected_state_version
        .ok_or(RequirementHttpError::BadRequest)?;
    let assessment_id = payload
        .assessment_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match operation {
        RequirementTransition::Accept
        | RequirementTransition::Reject
        | RequirementTransition::RequestChanges
            if assessment_id.is_none() =>
        {
            return Err(RequirementHttpError::BadRequest)
        }
        RequirementTransition::BeginDiscussion | RequirementTransition::Reopen
            if assessment_id.is_some() =>
        {
            return Err(RequirementHttpError::BadRequest)
        }
        _ => {}
    }
    let feedback = payload
        .feedback
        .map(|feedback| feedback.trim().to_owned())
        .filter(|feedback| !feedback.is_empty());
    if feedback
        .as_deref()
        .is_some_and(|feedback| feedback.len() > 10_000)
        || (operation == RequirementTransition::RequestChanges && feedback.is_none())
        || (operation != RequirementTransition::RequestChanges && feedback.is_some())
    {
        return Err(RequirementHttpError::BadRequest);
    }
    let requirement = state
        .store()
        .transition_requirement_with_feedback(
            &requirement_id,
            expected_state_version,
            &actor.user().id,
            operation,
            feedback.as_deref(),
            assessment_id,
        )
        .await
        .map_err(RequirementHttpError::from)?;
    state.events().requirement_changed(requirement.id.clone());
    Ok(Json(requirement.into()))
}

pub async fn begin_discussion(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<TransitionRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    transition(
        state,
        actor,
        requirement_id,
        payload,
        RequirementTransition::BeginDiscussion,
        false,
    )
    .await
}

pub async fn accept(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<TransitionRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    transition(
        state,
        actor,
        requirement_id,
        payload,
        RequirementTransition::Accept,
        true,
    )
    .await
}

pub async fn reject(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<TransitionRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    transition(
        state,
        actor,
        requirement_id,
        payload,
        RequirementTransition::Reject,
        true,
    )
    .await
}

pub async fn request_changes(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<TransitionRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    transition(
        state,
        actor,
        requirement_id,
        payload,
        RequirementTransition::RequestChanges,
        true,
    )
    .await
}

pub async fn reopen(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<TransitionRequest>,
) -> Result<Json<RequirementResponse>, RequirementHttpError> {
    transition(
        state,
        actor,
        requirement_id,
        payload,
        RequirementTransition::Reopen,
        true,
    )
    .await
}

fn bounded_text(value: &str, max: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max).then(|| value.to_owned())
}

fn normalize_edit_text(
    value: Option<String>,
    max: usize,
    allow_empty: bool,
) -> Result<Option<String>, RequirementHttpError> {
    value
        .map(|value| {
            let value = value.trim();
            if value.len() > max || (!allow_empty && value.is_empty()) {
                Err(RequirementHttpError::BadRequest)
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

fn normalize_edit_list(
    values: Option<Vec<String>>,
    max_items: usize,
    max_item: usize,
) -> Result<Option<Vec<String>>, RequirementHttpError> {
    let Some(values) = values else {
        return Ok(None);
    };
    if values.len() > max_items {
        return Err(RequirementHttpError::BadRequest);
    }
    values
        .into_iter()
        .map(|value| bounded_text(&value, max_item).ok_or(RequirementHttpError::BadRequest))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse_status(value: &str) -> Result<RequirementStatus, RequirementHttpError> {
    match value.to_ascii_lowercase().as_str() {
        "draft" => Ok(RequirementStatus::Draft),
        "discussing" => Ok(RequirementStatus::Discussing),
        "ready" => Ok(RequirementStatus::Ready),
        "accepted" => Ok(RequirementStatus::Accepted),
        "rejected" => Ok(RequirementStatus::Rejected),
        _ => Err(RequirementHttpError::BadRequest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_query_uses_domain_states() {
        assert_eq!(
            parse_status("Discussing").unwrap(),
            RequirementStatus::Discussing
        );
        assert!(parse_status("unknown").is_err());
    }

    #[test]
    fn oversized_structured_edits_are_rejected() {
        assert!(RequirementEditRequest {
            expected_state_version: Some(1),
            title: Some("x".repeat(501)),
            description: None,
            summary: None,
            acceptance_criteria: None,
            assumptions: None,
            open_questions: None,
        }
        .into_domain()
        .is_err());
        assert!(RequirementEditRequest {
            expected_state_version: Some(1),
            title: None,
            description: None,
            summary: None,
            acceptance_criteria: Some(vec!["criterion".into(); 101]),
            assumptions: None,
            open_questions: None,
        }
        .into_domain()
        .is_err());
    }

    #[test]
    fn edit_values_are_trimmed_before_domain_comparison() {
        let Ok((_, edit)) = (RequirementEditRequest {
            expected_state_version: Some(1),
            title: Some(" Title ".into()),
            description: Some(" Description ".into()),
            summary: Some(" Summary ".into()),
            acceptance_criteria: Some(vec![" criterion ".into()]),
            assumptions: Some(vec![" assumption ".into()]),
            open_questions: Some(vec![" question ".into()]),
        })
        .into_domain() else {
            panic!("valid edit values");
        };
        assert_eq!(edit.title.as_deref(), Some("Title"));
        assert_eq!(edit.description.as_deref(), Some("Description"));
        assert_eq!(edit.summary.as_deref(), Some("Summary"));
        assert_eq!(edit.acceptance_criteria, Some(vec!["criterion".into()]));
        assert_eq!(edit.assumptions, Some(vec!["assumption".into()]));
        assert_eq!(edit.open_questions, Some(vec!["question".into()]));
    }

    #[test]
    fn optional_fields_can_be_cleared() {
        let Ok((expected, edit)) = (RequirementEditRequest {
            expected_state_version: Some(7),
            title: None,
            description: None,
            summary: Some("  ".into()),
            acceptance_criteria: Some(Vec::new()),
            assumptions: Some(Vec::new()),
            open_questions: Some(Vec::new()),
        })
        .into_domain() else {
            panic!("valid optional edit values");
        };
        assert_eq!(expected, 7);
        assert_eq!(edit.summary.as_deref(), Some(""));
        assert_eq!(edit.acceptance_criteria, Some(Vec::new()));
        assert_eq!(edit.assumptions, Some(Vec::new()));
        assert_eq!(edit.open_questions, Some(Vec::new()));
        assert!(RequirementEditRequest {
            expected_state_version: Some(7),
            title: Some(" ".into()),
            description: None,
            summary: None,
            acceptance_criteria: None,
            assumptions: None,
            open_questions: None,
        }
        .into_domain()
        .is_err());
    }

    #[test]
    fn missing_expected_state_version_is_rejected() {
        assert!(RequirementEditRequest {
            expected_state_version: None,
            title: Some("new".into()),
            description: None,
            summary: None,
            acceptance_criteria: None,
            assumptions: None,
            open_questions: None,
        }
        .into_domain()
        .is_err());
    }
}
