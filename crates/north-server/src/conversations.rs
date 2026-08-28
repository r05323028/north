use axum::{
    extract::{Json, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Extension, Router,
};
use north_persistence::{ConversationError, ConversationPage, MessageRecord};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthState, CurrentUser};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug)]
pub enum ConversationHttpError {
    BadRequest,
    NotFound,
    Internal,
}

impl ConversationHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::Internal => "internal_error",
        }
    }
}

impl IntoResponse for ConversationHttpError {
    fn into_response(self) -> Response {
        (self.status(), Json(ErrorBody { error: self.code() })).into_response()
    }
}

impl From<ConversationError> for ConversationHttpError {
    fn from(error: ConversationError) -> Self {
        match error {
            ConversationError::RequirementNotFound => Self::NotFound,
            ConversationError::InvalidKind(_)
            | ConversationError::InvalidMessage
            | ConversationError::InvalidPage => Self::BadRequest,
            ConversationError::Database(_) => Self::Internal,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PostMessageRequest {
    pub body: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ConversationQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MessageResponse {
    pub id: String,
    pub conversation_id: String,
    pub author_user_id: Option<String>,
    pub kind: String,
    pub body: String,
    pub created_at: String,
}

impl From<MessageRecord> for MessageResponse {
    fn from(message: MessageRecord) -> Self {
        Self {
            id: message.id,
            conversation_id: message.conversation_id,
            author_user_id: message.author_user_id,
            kind: message.kind.as_str().into(),
            body: message.body,
            created_at: message.created_at,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ConversationResponse {
    pub id: String,
    pub requirement_id: String,
    pub created_at: String,
    pub messages: Vec<MessageResponse>,
    pub next_offset: Option<u64>,
}

impl From<ConversationPage> for ConversationResponse {
    fn from(page: ConversationPage) -> Self {
        Self {
            id: page.conversation.id,
            requirement_id: page.conversation.requirement_id,
            created_at: page.conversation.created_at,
            messages: page.messages.into_iter().map(Into::into).collect(),
            next_offset: page.next_offset,
        }
    }
}

pub async fn post_message(
    State(state): State<AuthState>,
    Extension(actor): Extension<CurrentUser>,
    Path(requirement_id): Path<String>,
    Json(payload): Json<PostMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), ConversationHttpError> {
    let body = payload.body.trim();
    if body.is_empty() || body.len() > 100_000 {
        return Err(ConversationHttpError::BadRequest);
    }
    let message = state
        .store()
        .post_requester_message(&requirement_id, &actor.user().id, body)
        .await
        .map_err(ConversationHttpError::from)?;
    Ok((StatusCode::CREATED, Json(message.into())))
}

pub async fn get_conversation(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
    Query(query): Query<ConversationQuery>,
) -> Result<Json<ConversationResponse>, ConversationHttpError> {
    let page = state
        .store()
        .conversation_page(
            &requirement_id,
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(ConversationHttpError::from)?;
    Ok(Json(page.into()))
}

pub fn router() -> Router<AuthState> {
    Router::new()
        .route(
            "/requirements/{requirement_id}/conversation",
            get(get_conversation),
        )
        .route(
            "/requirements/{requirement_id}/conversation/messages",
            post(post_message),
        )
        .route(
            "/requirements/{requirement_id}/messages",
            get(get_conversation).post(post_message),
        )
        .route(
            "/requirements/{requirement_id}/conversation/structured",
            patch(crate::requirements::edit_requirement),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requester_api_cannot_select_telemetry_kind() {
        let response = MessageResponse {
            id: "message-1".into(),
            conversation_id: "conversation-1".into(),
            author_user_id: Some("user-1".into()),
            kind: north_persistence::MessageKind::Requester.as_str().into(),
            body: "clarify scope".into(),
            created_at: "now".into(),
        };
        assert_eq!(response.kind, "requester");
    }
}
