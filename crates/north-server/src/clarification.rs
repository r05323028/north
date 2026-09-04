use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use north_domain::status::RequirementStatus;
use north_persistence::{
    AuthStore, ClarificationActivity, ClarificationError, ClarificationPhase, ClarificationRun,
    ClarificationStartInput, ClarificationStartResult, ClarificationStatus, MessageKind,
    ReadinessView, RepositoryRecord, RequirementError, RequirementRecord,
};
use north_protocol::{
    Command, CommandEnvelope, MessageSend, ServerFrame, SessionCancel, SessionStart, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    auth::{AuthState, CurrentUser},
    context::{
        assemble_session_start, select_conversation_excerpt, ConversationMessageSnapshot,
        ConversationRole, RepositorySnapshot, RequirementSnapshot,
    },
};

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub message_id: String,
    pub expected_state_version: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ActivityQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ClarificationRunResponse {
    pub run_id: String,
    pub requirement_id: String,
    pub start_message_id: String,
    pub phase: String,
    pub status: String,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
}

impl From<&ClarificationRun> for ClarificationRunResponse {
    fn from(run: &ClarificationRun) -> Self {
        Self {
            run_id: run.run_id.clone(),
            requirement_id: run.requirement_id.clone(),
            start_message_id: run.start_message_id.clone(),
            phase: run.phase.as_str().into(),
            status: run.status.as_str().into(),
            cancel_requested: run.cancel_requested,
            created_at: run.created_at.clone(),
            updated_at: run.updated_at.clone(),
            last_activity_at: run.last_activity_at.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SessionReadResponse {
    pub session: Option<ClarificationRunResponse>,
}

#[derive(Debug, Serialize)]
pub struct ClarificationMutationResponse {
    pub session: ClarificationRunResponse,
}

#[derive(Debug, Serialize)]
pub struct ReadinessResponse {
    pub assessment: Option<ReadinessAssessmentResponse>,
}

#[derive(Debug, Serialize)]
pub struct ReadinessAssessmentResponse {
    pub id: String,
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub requirement_revision: u64,
    pub verdict: String,
    pub blockers: Vec<String>,
    pub assumptions: Vec<String>,
    pub repositories_reviewed: Value,
    pub outcome: String,
    pub rejection_reason: Option<String>,
    pub assessed_at_ms: i64,
    pub accepted_state_version: Option<u64>,
    pub created_at: String,
    pub current: bool,
}

#[derive(Debug, Serialize)]
pub struct ActivityResponse {
    pub activities: Vec<ActivityItemResponse>,
    pub next_offset: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ActivityItemResponse {
    pub id: i64,
    pub event_id: String,
    pub session_id: String,
    pub activity: String,
    pub created_at: String,
}

impl From<ClarificationActivity> for ActivityItemResponse {
    fn from(activity: ClarificationActivity) -> Self {
        Self {
            id: activity.id,
            event_id: activity.event_id,
            session_id: activity.session_id,
            activity: activity.activity,
            created_at: activity.created_at,
        }
    }
}

#[derive(Debug)]
pub enum ClarificationHttpError {
    BadRequest,
    NotFound,
    Conflict,
    Internal,
    Unavailable {
        requirement: Box<crate::requirements::RequirementResponse>,
        run: Box<ClarificationRun>,
    },
}

impl ClarificationHttpError {
    fn response(self) -> (StatusCode, Value) {
        match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST, json!({"error":"bad_request"})),
            Self::NotFound => (StatusCode::NOT_FOUND, json!({"error":"not_found"})),
            Self::Conflict => (StatusCode::CONFLICT, json!({"error":"conflict"})),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"internal_error"}),
            ),
            Self::Unavailable { requirement, run } => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({
                    "error": "clarification_unavailable",
                    "requirement": requirement,
                    "session": ClarificationRunResponse::from(run.as_ref()),
                }),
            ),
        }
    }
}

impl IntoResponse for ClarificationHttpError {
    fn into_response(self) -> Response {
        let (status, body) = self.response();
        (status, Json(body)).into_response()
    }
}

impl From<ClarificationError> for ClarificationHttpError {
    fn from(error: ClarificationError) -> Self {
        match error {
            ClarificationError::RequirementNotFound
            | ClarificationError::MessageNotFound
            | ClarificationError::RunNotFound => Self::NotFound,
            ClarificationError::InvalidMessage | ClarificationError::InvalidContext => {
                Self::BadRequest
            }
            ClarificationError::StateVersionConflict
            | ClarificationError::ExistingRunDifferentStart
            | ClarificationError::RunNotEligible
            | ClarificationError::CommandConflict => Self::Conflict,
            ClarificationError::InvalidSessionState | ClarificationError::Database(_) => {
                Self::Internal
            }
        }
    }
}

impl From<RequirementError> for ClarificationHttpError {
    fn from(error: RequirementError) -> Self {
        match error {
            RequirementError::NotFound => Self::NotFound,
            RequirementError::Conflict => Self::Conflict,
            RequirementError::InvalidStatus(_)
            | RequirementError::InvalidTransition(_)
            | RequirementError::Edit(_)
            | RequirementError::InvalidRevision
            | RequirementError::InvalidStateVersion => Self::Internal,
            RequirementError::Database(_) => Self::Internal,
        }
    }
}

/// Authenticated clarification reads and explicit intent mutations.
pub fn router() -> Router<AuthState> {
    Router::new()
        .route(
            "/requirements/{requirement_id}/clarification/start",
            post(start_clarification),
        )
        .route(
            "/requirements/{requirement_id}/clarification/runs/{run_id}/messages/{message_id}/dispatch",
            post(dispatch_message),
        )
        .route(
            "/requirements/{requirement_id}/clarification/runs/{run_id}/cancel",
            post(cancel_clarification),
        )
        .route(
            "/requirements/{requirement_id}/session",
            get(get_session),
        )
        .route(
            "/requirements/{requirement_id}/readiness",
            get(get_readiness),
        )
        .route(
            "/requirements/{requirement_id}/activity",
            get(get_activity),
        )
}

async fn start_clarification(
    State(state): State<AuthState>,
    Extension(_actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<StartRequest>,
) -> Result<Response, ClarificationHttpError> {
    let expected_state_version = payload
        .expected_state_version
        .ok_or(ClarificationHttpError::BadRequest)?;
    let requirement = load_requirement(state.store(), &requirement_id).await?;
    let message = state
        .store()
        .message_for_requirement(&requirement_id, &payload.message_id)
        .await
        .map_err(|_| ClarificationHttpError::Internal)?
        .ok_or(ClarificationHttpError::NotFound)?;
    if message.kind != MessageKind::Requester {
        return Err(ClarificationHttpError::BadRequest);
    }
    let messages = state
        .store()
        .conversation_messages(&requirement_id)
        .await
        .map_err(|_| ClarificationHttpError::Internal)?;
    let conversation = messages
        .into_iter()
        .map(|message| ConversationMessageSnapshot {
            message_id: message.id,
            role: match message.kind {
                MessageKind::Requester => ConversationRole::Requester,
                MessageKind::Agent => ConversationRole::Agent,
                MessageKind::System => ConversationRole::System,
            },
            content: message.body,
        })
        .collect::<Vec<_>>();
    let excerpt = select_conversation_excerpt(&conversation, &payload.message_id)
        .map_err(|_| ClarificationHttpError::BadRequest)?;
    let repositories = state
        .store()
        .active_repositories()
        .await
        .map_err(|_| ClarificationHttpError::Internal)?
        .into_iter()
        .map(repository_snapshot)
        .collect::<Vec<_>>();
    let start_context = assemble_session_start(
        RequirementSnapshot {
            id: requirement.id.clone(),
            revision: requirement.revision,
            title: requirement.title.clone(),
            description: requirement.description.clone(),
            summary: requirement.summary.clone(),
            acceptance_criteria: requirement.acceptance_criteria.clone(),
            assumptions: requirement.assumptions.clone(),
            open_questions: requirement.open_questions.clone(),
        },
        excerpt,
        repositories,
    )
    .map_err(|_| ClarificationHttpError::BadRequest)?;
    let context =
        serde_json::to_value(start_context).map_err(|_| ClarificationHttpError::Internal)?;
    let capability = "agent".to_owned();
    let repository_ids = context_repository_ids(&context);
    let result = state
        .store()
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &requirement_id,
                start_message_id: &payload.message_id,
                expected_state_version,
                context: &context,
                context_requirement_revision: requirement.revision,
                repository_ids: &repository_ids,
                required_capabilities: std::slice::from_ref(&capability),
            },
            build_start_payload,
        )
        .await
        .map_err(ClarificationHttpError::from)?;
    finish_start(&state, requirement, result).await
}

async fn finish_start(
    state: &AuthState,
    before: RequirementRecord,
    result: ClarificationStartResult,
) -> Result<Response, ClarificationHttpError> {
    if let Some(command_id) = result.command_id.as_deref() {
        dispatch_command_if_needed(state, command_id).await?;
    }
    let requirement = load_requirement(state.store(), &before.id).await?;
    state.events().session_changed(requirement.id.clone());
    if requirement.state_version != before.state_version {
        state.events().requirement_changed(requirement.id.clone());
    }
    if result.run.status == ClarificationStatus::Unavailable
        && result.run.phase == ClarificationPhase::AwaitingAssignment
    {
        return Err(ClarificationHttpError::Unavailable {
            requirement: Box::new(requirement.into()),
            run: Box::new(result.run),
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(ClarificationMutationResponse {
            session: (&result.run).into(),
        }),
    )
        .into_response())
}

async fn dispatch_message(
    State(state): State<AuthState>,
    Extension(_actor): Extension<CurrentUser>,
    Path((requirement_id, run_id, message_id)): Path<(String, String, String)>,
) -> Result<Response, ClarificationHttpError> {
    let result = state
        .store()
        .dispatch_clarification_message(
            &requirement_id,
            &run_id,
            &message_id,
            build_message_payload,
        )
        .await
        .map_err(ClarificationHttpError::from)?;
    dispatch_command_if_needed(&state, &result.command_id).await?;
    state.events().conversation_changed(requirement_id.clone());
    state.events().session_changed(requirement_id);
    Ok((
        StatusCode::ACCEPTED,
        Json(ClarificationMutationResponse {
            session: (&result.run).into(),
        }),
    )
        .into_response())
}

async fn cancel_clarification(
    State(state): State<AuthState>,
    Extension(_actor): Extension<CurrentUser>,
    Path((requirement_id, run_id)): Path<(String, String)>,
) -> Result<Response, ClarificationHttpError> {
    let result = state
        .store()
        .cancel_clarification(&requirement_id, &run_id, build_cancel_payload)
        .await
        .map_err(ClarificationHttpError::from)?;
    if !result.command_id.is_empty() {
        dispatch_command_if_needed(&state, &result.command_id).await?;
    }
    state.events().session_changed(requirement_id);
    Ok((
        StatusCode::ACCEPTED,
        Json(ClarificationMutationResponse {
            session: (&result.run).into(),
        }),
    )
        .into_response())
}

async fn get_session(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
) -> Result<Json<SessionReadResponse>, ClarificationHttpError> {
    load_requirement(state.store(), &requirement_id).await?;
    let session = state
        .store()
        .latest_clarification_run(&requirement_id)
        .await
        .map_err(ClarificationHttpError::from)?
        .as_ref()
        .map(ClarificationRunResponse::from);
    Ok(Json(SessionReadResponse { session }))
}

async fn get_readiness(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
) -> Result<Json<ReadinessResponse>, ClarificationHttpError> {
    let requirement = load_requirement(state.store(), &requirement_id).await?;
    let assessment = state
        .store()
        .latest_readiness(&requirement_id)
        .await
        .map_err(ClarificationHttpError::from)?
        .map(|assessment| readiness_response(assessment, &requirement))
        .transpose()?;
    Ok(Json(ReadinessResponse { assessment }))
}

async fn get_activity(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<ActivityResponse>, ClarificationHttpError> {
    load_requirement(state.store(), &requirement_id).await?;
    let (activities, next_offset) = state
        .store()
        .clarification_activities(
            &requirement_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(ClarificationHttpError::from)?;
    Ok(Json(ActivityResponse {
        activities: activities.into_iter().map(Into::into).collect(),
        next_offset,
    }))
}

fn readiness_response(
    assessment: ReadinessView,
    requirement: &RequirementRecord,
) -> Result<ReadinessAssessmentResponse, ClarificationHttpError> {
    let current = assessment.outcome == "accepted"
        && requirement.status == RequirementStatus::Ready
        && assessment.requirement_revision == requirement.revision
        && assessment.accepted_state_version == Some(requirement.state_version);
    Ok(ReadinessAssessmentResponse {
        id: assessment.id,
        event_id: assessment.event_id,
        session_id: assessment.session_id,
        daemon_event_seq: assessment.daemon_event_seq,
        requirement_revision: assessment.requirement_revision,
        verdict: assessment.verdict,
        blockers: assessment.blockers,
        assumptions: assessment.assumptions,
        repositories_reviewed: assessment.repositories_reviewed,
        outcome: assessment.outcome,
        rejection_reason: assessment.rejection_reason,
        assessed_at_ms: assessment.assessed_at_ms,
        accepted_state_version: assessment.accepted_state_version,
        created_at: assessment.created_at,
        current,
    })
}

async fn load_requirement(
    store: &AuthStore,
    requirement_id: &str,
) -> Result<RequirementRecord, ClarificationHttpError> {
    store
        .requirement_by_id(requirement_id)
        .await
        .map_err(ClarificationHttpError::from)?
        .ok_or(ClarificationHttpError::NotFound)
}

fn repository_snapshot(repository: RepositoryRecord) -> RepositorySnapshot {
    RepositorySnapshot {
        repository_id: repository.id,
        name: repository.name,
        url: repository.url,
        description: repository.description,
        enabled: repository.disabled_at.is_none(),
    }
}

fn context_repository_ids(context: &Value) -> Vec<String> {
    context["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|repository| repository["repository_id"].as_str().map(str::to_owned))
        .collect()
}

fn build_start_payload(
    _daemon_id: &str,
    run_id: &str,
    command_id: &str,
    sequence: u64,
    context: &Value,
) -> Result<String, north_persistence::PersistenceError> {
    let start: SessionStart = serde_json::from_value(context.clone())
        .map_err(|_| north_persistence::PersistenceError::InvalidCommandPayload)?;
    ServerFrame::Command(CommandEnvelope {
        command_id: command_id.to_owned(),
        session_id: run_id.to_owned(),
        server_command_seq: sequence,
        sent_at: server_time(),
        schema_version: SCHEMA_VERSION,
        command: Command::SessionStart(start),
    })
    .to_json()
    .map_err(|_| north_persistence::PersistenceError::InvalidCommandPayload)
}

fn build_message_payload(
    _daemon_id: &str,
    run_id: &str,
    command_id: &str,
    sequence: u64,
    message_id: &str,
    content: &str,
) -> Result<String, north_persistence::PersistenceError> {
    ServerFrame::Command(CommandEnvelope {
        command_id: command_id.to_owned(),
        session_id: run_id.to_owned(),
        server_command_seq: sequence,
        sent_at: server_time(),
        schema_version: SCHEMA_VERSION,
        command: Command::MessageSend(MessageSend {
            message_id: message_id.to_owned(),
            content: content.to_owned(),
        }),
    })
    .to_json()
    .map_err(|_| north_persistence::PersistenceError::InvalidCommandPayload)
}

fn build_cancel_payload(
    _daemon_id: &str,
    run_id: &str,
    command_id: &str,
    sequence: u64,
) -> Result<String, north_persistence::PersistenceError> {
    ServerFrame::Command(CommandEnvelope {
        command_id: command_id.to_owned(),
        session_id: run_id.to_owned(),
        server_command_seq: sequence,
        sent_at: server_time(),
        schema_version: SCHEMA_VERSION,
        command: Command::SessionCancel(SessionCancel {
            reason: "requester_cancelled".into(),
        }),
    })
    .to_json()
    .map_err(|_| north_persistence::PersistenceError::InvalidCommandPayload)
}

async fn dispatch_command_if_needed(
    state: &AuthState,
    command_id: &str,
) -> Result<(), ClarificationHttpError> {
    let Some(command) = state
        .store()
        .command_by_id(command_id)
        .await
        .map_err(|_| ClarificationHttpError::Internal)?
    else {
        return Err(ClarificationHttpError::Internal);
    };
    if command.compacted {
        return Ok(());
    }
    match state
        .daemon_runtime()
        .dispatch_pinned_command(&command)
        .await
    {
        Ok(()) | Err(crate::daemon::DaemonDispatchError::DaemonUnavailable) => Ok(()),
        Err(crate::daemon::DaemonDispatchError::SessionNotFound)
        | Err(crate::daemon::DaemonDispatchError::InvalidCommand)
        | Err(crate::daemon::DaemonDispatchError::Internal) => {
            Err(ClarificationHttpError::Internal)
        }
    }
}

fn server_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_run_projection_excludes_daemon_details() {
        let run = ClarificationRunResponse {
            run_id: "run".into(),
            requirement_id: "requirement".into(),
            start_message_id: "message".into(),
            phase: "active".into(),
            status: "starting".into(),
            cancel_requested: false,
            created_at: "now".into(),
            updated_at: "now".into(),
            last_activity_at: "now".into(),
        };
        let json = serde_json::to_value(run).expect("projection JSON");
        assert!(json.get("daemon_id").is_none());
        assert!(json.get("run_id").is_some());
    }
}
