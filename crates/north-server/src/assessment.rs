//! Server-owned readiness assessment ingestion and review-packet projection.
//!
//! Wire DTOs are converted here, then persistence performs the domain gates and
//! one-transaction evidence/state update. ACK construction happens only after
//! that persistence call returns successfully.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use north_domain::readiness::{ReadinessAssessment, ReviewPacket, ReviewedRepository, Verdict};
use north_persistence::{AssessmentOutcome, AuthStore, ReadinessAssessmentResult, ReadinessError};
use north_protocol::{
    Event, EventAck, EventAckStatus, EventEnvelope, FrameError, ReadinessVerdictWire,
    RequirementAssessed, SCHEMA_VERSION,
};
use serde::Serialize;
use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::auth::AuthState;

/// Convert typed protocol evidence into the domain assessment.
/// `north-protocol` remains independent of `north-domain`.
pub fn readiness_assessment_from_wire(
    wire: &RequirementAssessed,
    assessed_at_ms: u64,
) -> Result<ReadinessAssessment, FrameError> {
    wire.validate()?;
    Ok(ReadinessAssessment {
        requirement_revision: wire.requirement_revision,
        verdict: match wire.verdict {
            ReadinessVerdictWire::Ready => Verdict::Ready,
            ReadinessVerdictWire::NeedsClarification => Verdict::NeedsClarification,
        },
        blockers: wire.blockers.clone(),
        assumptions: wire.assumptions.clone(),
        repositories_reviewed: wire
            .repositories_reviewed
            .iter()
            .map(|repository| north_domain::readiness::ReviewedRepository {
                repository_id: repository.repository_id.clone(),
                commit_sha: repository.commit_sha.clone(),
            })
            .collect(),
        assessed_at_ms,
    })
}

#[derive(Debug)]
pub enum AssessmentError {
    InvalidPayload(FrameError),
    Persistence(ReadinessError),
    NotAssessmentEvent,
}

impl fmt::Display for AssessmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPayload(error) => write!(f, "invalid assessment payload: {error}"),
            Self::Persistence(error) => write!(f, "assessment persistence failed: {error}"),
            Self::NotAssessmentEvent => f.write_str("event is not a requirement.assessed event"),
        }
    }
}

impl Error for AssessmentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPayload(error) => Some(error),
            Self::Persistence(error) => Some(error),
            Self::NotAssessmentEvent => None,
        }
    }
}

/// Process one typed assessment and return its canonical event ACK.
/// Persistence commits evidence, dedupe, and any lifecycle transition before
/// this function returns an ACK.
pub async fn process_requirement_assessed(
    store: &AuthStore,
    event_id: &str,
    session_id: &str,
    daemon_event_seq: u64,
    payload: &RequirementAssessed,
) -> Result<EventAck, AssessmentError> {
    let assessment = readiness_assessment_from_wire(payload, now_ms())
        .map_err(AssessmentError::InvalidPayload)?;
    let result = store
        .record_readiness_assessment(
            event_id,
            session_id,
            daemon_event_seq,
            &payload.requirement_id,
            &assessment,
        )
        .await
        .map_err(AssessmentError::Persistence)?;
    Ok(event_ack(
        &result.record.event_id,
        &result.record.session_id,
        result.record.daemon_event_seq,
        &result,
    ))
}

/// Process a daemon envelope. Non-assessment events remain owned by later
/// runtime event handling and are rejected here rather than silently mutated.
pub async fn handle_requirement_assessed(
    store: &AuthStore,
    envelope: &EventEnvelope,
) -> Result<EventAck, AssessmentError> {
    let Event::RequirementAssessed(payload) = &envelope.event else {
        return Err(AssessmentError::NotAssessmentEvent);
    };
    process_requirement_assessed(
        store,
        &envelope.event_id,
        &envelope.session_id,
        envelope.daemon_event_seq,
        payload,
    )
    .await
}

fn event_ack(
    event_id: &str,
    session_id: &str,
    daemon_event_seq: u64,
    result: &ReadinessAssessmentResult,
) -> EventAck {
    let (status, reason) = match result.record.outcome {
        AssessmentOutcome::Accepted => (EventAckStatus::Accepted, None),
        AssessmentOutcome::Rejected => (
            EventAckStatus::Rejected,
            result.record.rejection_reason.clone(),
        ),
    };
    EventAck {
        event_id: event_id.to_owned(),
        session_id: session_id.to_owned(),
        daemon_event_seq,
        schema_version: SCHEMA_VERSION,
        status,
        reason,
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Debug)]
pub enum AssessmentHttpError {
    NotFound,
    Conflict,
    Internal,
}

impl AssessmentHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Internal => "internal_error",
        }
    }
}

impl IntoResponse for AssessmentHttpError {
    fn into_response(self) -> Response {
        (self.status(), Json(ErrorBody { error: self.code() })).into_response()
    }
}

impl From<ReadinessError> for AssessmentHttpError {
    fn from(error: ReadinessError) -> Self {
        match error {
            ReadinessError::RequirementNotFound => Self::NotFound,
            ReadinessError::StaleAssessment { .. }
            | ReadinessError::StaleStateVersion { .. }
            | ReadinessError::NotReady => Self::Conflict,
            ReadinessError::InvalidStatus(_)
            | ReadinessError::InvalidOutcome(_)
            | ReadinessError::InvalidEvidence
            | ReadinessError::SequenceConflict
            | ReadinessError::EventIdentityConflict
            | ReadinessError::SessionRequirementMismatch => Self::Internal,
            ReadinessError::Database(_) | ReadinessError::InvalidRevision => Self::Internal,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ReviewPacketResponse {
    pub assessment_id: String,
    pub requirement_revision: u64,
    pub requirement_state_version: u64,
    pub goal: String,
    pub scope: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub blockers: Vec<String>,
    pub assessment_assumptions: Vec<String>,
    pub repositories_reviewed: Vec<ReviewedRepositoryResponse>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ReviewedRepositoryResponse {
    pub repository_id: String,
    pub commit_sha: String,
}

impl From<ReviewPacket> for ReviewPacketResponse {
    fn from(packet: ReviewPacket) -> Self {
        Self {
            assessment_id: packet.assessment_id,
            requirement_revision: packet.requirement_revision,
            requirement_state_version: packet.requirement_state_version,
            goal: packet.goal,
            scope: packet.scope,
            summary: packet.summary,
            acceptance_criteria: packet.acceptance_criteria,
            assumptions: packet.assumptions,
            open_questions: packet.open_questions,
            blockers: packet.blockers,
            assessment_assumptions: packet.assessment_assumptions,
            repositories_reviewed: packet
                .repositories_reviewed
                .into_iter()
                .map(ReviewedRepositoryResponse::from)
                .collect(),
        }
    }
}

impl From<ReviewedRepository> for ReviewedRepositoryResponse {
    fn from(repository: ReviewedRepository) -> Self {
        Self {
            repository_id: repository.repository_id,
            commit_sha: repository.commit_sha,
        }
    }
}

pub async fn review_packet(
    State(state): State<AuthState>,
    Path(requirement_id): Path<String>,
) -> Result<Json<ReviewPacketResponse>, AssessmentHttpError> {
    let packet = state
        .store()
        .review_packet(&requirement_id)
        .await
        .map_err(AssessmentHttpError::from)?;
    Ok(Json(packet.into()))
}

pub fn router() -> Router<AuthState> {
    Router::new().route(
        "/requirements/{requirement_id}/review-packet",
        get(review_packet),
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_status_matches_assessment_outcome() {
        let record = ReadinessAssessmentResult {
            record: north_persistence::ReadinessAssessmentRecord {
                id: "assessment-1".into(),
                event_id: "event-1".into(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                event_requirement_id: "requirement-1".into(),
                requirement_id: Some("requirement-1".into()),
                assessment: ReadinessAssessment {
                    requirement_revision: 1,
                    verdict: Verdict::NeedsClarification,
                    blockers: vec!["scope".into()],
                    assumptions: Vec::new(),
                    repositories_reviewed: Vec::new(),
                    assessed_at_ms: 1,
                },
                outcome: AssessmentOutcome::Rejected,
                accepted_state_version: Some(1),
                rejection_reason: Some("blockers_present".into()),
                created_at: "now".into(),
            },
            duplicate: false,
        };
        let ack = event_ack("event-1", "session-1", 1, &record);
        assert_eq!(ack.status, EventAckStatus::Rejected);
        assert_eq!(ack.reason.as_deref(), Some("blockers_present"));
    }

    #[test]
    fn converts_typed_wire_evidence_without_protocol_domain_dependency() -> Result<(), FrameError> {
        let assessment = readiness_assessment_from_wire(
            &RequirementAssessed {
                requirement_id: "requirement-1".into(),
                requirement_revision: 2,
                verdict: ReadinessVerdictWire::Ready,
                blockers: Vec::new(),
                assumptions: vec!["One account".into()],
                repositories_reviewed: vec![north_protocol::ReviewedRepositoryWire {
                    repository_id: "north".into(),
                    commit_sha: "abc123".into(),
                }],
            },
            42,
        )?;

        assert_eq!(assessment.requirement_revision, 2);
        assert_eq!(assessment.verdict, Verdict::Ready);
        assert_eq!(assessment.repositories_reviewed[0].commit_sha, "abc123");
        assert_eq!(assessment.assessed_at_ms, 42);
        Ok(())
    }
}
