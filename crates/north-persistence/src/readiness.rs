use crate::{
    requirements::{lock_requirement, update_requirement, RequirementError},
    AuthStore,
};
use north_domain::{
    readiness::{ReadinessAssessment, ReviewPacket, ReviewedRepository, Verdict},
    requirement::MarkReadyError,
    status::RequirementStatus,
};
use sqlx::{FromRow, Postgres, Transaction};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssessmentOutcome {
    Accepted,
    Rejected,
}

impl AssessmentOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, ReadinessError> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            value => Err(ReadinessError::InvalidOutcome(value.to_owned())),
        }
    }
}

/// Immutable evidence row plus its server-authoritative result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessAssessmentRecord {
    pub id: String,
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub event_requirement_id: String,
    pub requirement_id: Option<String>,
    pub assessment: ReadinessAssessment,
    pub outcome: AssessmentOutcome,
    pub rejection_reason: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessAssessmentResult {
    pub record: ReadinessAssessmentRecord,
    pub duplicate: bool,
}

struct AssessmentEvent<'a> {
    event_id: &'a str,
    session_id: &'a str,
    daemon_event_seq: u64,
}

#[derive(Debug)]
pub enum ReadinessError {
    Database(sqlx::Error),
    RequirementNotFound,
    InvalidStatus(String),
    InvalidRevision,
    InvalidOutcome(String),
    InvalidEvidence,
    SequenceConflict,
    EventIdentityConflict,
    SessionRequirementMismatch,
    StaleAssessment {
        assessment_revision: u64,
        current_revision: u64,
    },
    NotReady,
}

impl fmt::Display for ReadinessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::RequirementNotFound => f.write_str("requirement not found"),
            Self::InvalidStatus(status) => write!(f, "invalid requirement status: {status}"),
            Self::InvalidRevision => f.write_str("invalid requirement revision"),
            Self::InvalidOutcome(outcome) => write!(f, "invalid assessment outcome: {outcome}"),
            Self::InvalidEvidence => f.write_str("invalid readiness evidence"),
            Self::SequenceConflict => f.write_str("assessment event sequence conflicts with another event"),
            Self::EventIdentityConflict => {
                f.write_str("assessment event identity conflicts with a committed event")
            }
            Self::SessionRequirementMismatch => {
                f.write_str("assessment session is not bound to its requirement")
            }
            Self::StaleAssessment {
                assessment_revision,
                current_revision,
            } => write!(
                f,
                "assessment revision {assessment_revision} does not match current revision {current_revision}"
            ),
            Self::NotReady => f.write_str("requirement is not ready for review"),
        }
    }
}

impl Error for ReadinessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ReadinessError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<RequirementError> for ReadinessError {
    fn from(error: RequirementError) -> Self {
        match error {
            RequirementError::Database(error) => Self::Database(error),
            RequirementError::NotFound => Self::RequirementNotFound,
            RequirementError::InvalidStatus(status) => Self::InvalidStatus(status),
            RequirementError::InvalidRevision => Self::InvalidRevision,
            RequirementError::Conflict
            | RequirementError::InvalidTransition(_)
            | RequirementError::Edit(_) => Self::InvalidEvidence,
        }
    }
}

impl AuthStore {
    /// Record one assessment and, when all domain gates pass, promote Ready.
    /// Dedupe, row locking, evidence, audit, and state update share one commit.
    pub async fn record_readiness_assessment(
        &self,
        event_id: &str,
        session_id: &str,
        daemon_event_seq: u64,
        requirement_id: &str,
        assessment: &ReadinessAssessment,
    ) -> Result<ReadinessAssessmentResult, ReadinessError> {
        if event_id.trim().is_empty() || session_id.trim().is_empty() || daemon_event_seq == 0 {
            return Err(ReadinessError::InvalidEvidence);
        }
        if assessment.requirement_revision == 0
            || assessment.repositories_reviewed.iter().any(|repository| {
                repository.repository_id.trim().is_empty()
                    || repository.commit_sha.trim().is_empty()
            })
            || assessment
                .blockers
                .iter()
                .any(|value| value.trim().is_empty())
            || assessment
                .assumptions
                .iter()
                .any(|value| value.trim().is_empty())
        {
            return Err(ReadinessError::InvalidEvidence);
        }
        let event_requirement_id = requirement_id.to_owned();
        let event = AssessmentEvent {
            event_id,
            session_id,
            daemon_event_seq,
        };
        let mut transaction = self.pool.begin().await?;
        // ponytail: one advisory lock per event id; replace with a keyed ledger lock only if throughput matters.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(event.event_id)
            .execute(&mut *transaction)
            .await?;
        let sequence =
            i64::try_from(event.daemon_event_seq).map_err(|_| ReadinessError::InvalidEvidence)?;
        let sequence_lock_key = format!("{}:{sequence}", event.session_id);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(sequence_lock_key)
            .execute(&mut *transaction)
            .await?;
        let bound_requirement_id =
            session_requirement_row(&mut transaction, event.session_id).await?;
        if bound_requirement_id.as_deref() != Some(requirement_id) {
            return Err(ReadinessError::SessionRequirementMismatch);
        }
        if let Some(row) = assessment_by_event(&mut transaction, event.event_id).await? {
            let record = row.into_record()?;
            if record.session_id != event.session_id
                || record.daemon_event_seq != event.daemon_event_seq
                || record.event_requirement_id != requirement_id
                || !same_assessment_content(&record.assessment, assessment)
            {
                return Err(ReadinessError::EventIdentityConflict);
            }
            transaction.commit().await?;
            return Ok(ReadinessAssessmentResult {
                record,
                duplicate: true,
            });
        }
        if let Some(row) =
            assessment_by_sequence(&mut transaction, event.session_id, sequence).await?
        {
            if row.event_id != event.event_id {
                return Err(ReadinessError::SequenceConflict);
            }
        }

        let row = match lock_requirement(&mut transaction, requirement_id).await {
            Ok(row) => row,
            Err(RequirementError::NotFound) => {
                let record_row = insert_assessment(
                    &mut transaction,
                    &event,
                    &event_requirement_id,
                    None,
                    assessment,
                    AssessmentOutcome::Rejected,
                    Some("requirement_not_found"),
                )
                .await?;
                transaction.commit().await?;
                return Ok(ReadinessAssessmentResult {
                    record: record_row.into_record()?,
                    duplicate: false,
                });
            }
            Err(error) => return Err(ReadinessError::from(error)),
        };
        let mut requirement = row.to_domain().map_err(ReadinessError::from)?;
        let current_revision = requirement.revision();
        let (outcome, rejection_reason) = match requirement.mark_ready(assessment) {
            Ok(()) => (AssessmentOutcome::Accepted, None),
            Err(error) => (AssessmentOutcome::Rejected, Some(mark_ready_reason(&error))),
        };
        let record_row = insert_assessment(
            &mut transaction,
            &event,
            &event_requirement_id,
            Some(requirement_id),
            assessment,
            outcome,
            rejection_reason.as_deref(),
        )
        .await?;

        if outcome == AssessmentOutcome::Accepted {
            update_requirement(&mut transaction, &requirement, current_revision)
                .await
                .map_err(ReadinessError::from)?;
            sqlx::query(
                "INSERT INTO transition_audit
                    (requirement_id, actor_id, transition, from_status, to_status)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(requirement.id())
            .bind(event_id)
            .bind("mark_ready")
            .bind(persisted_status(RequirementStatus::Discussing))
            .bind(persisted_status(requirement.status()))
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(ReadinessAssessmentResult {
            record: record_row.into_record()?,
            duplicate: false,
        })
    }

    /// Build a review packet from the current Requirement and matching evidence.
    pub async fn review_packet(
        &self,
        requirement_id: &str,
    ) -> Result<ReviewPacket, ReadinessError> {
        let mut transaction = self.pool.begin().await?;
        let row = lock_requirement(&mut transaction, requirement_id)
            .await
            .map_err(ReadinessError::from)?;
        let requirement = row.to_domain().map_err(ReadinessError::from)?;
        if requirement.status() != RequirementStatus::Ready {
            return Err(ReadinessError::NotReady);
        }
        let assessment = sqlx::query_as::<_, AssessmentRow>(
            "SELECT id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                    requirement_revision, verdict, blockers, assumptions,
                    repositories_reviewed, outcome, rejection_reason,
                    assessed_at_ms, created_at::text AS created_at
             FROM readiness_assessments
             WHERE requirement_id = $1
               AND requirement_revision = $2
               AND outcome = 'accepted'
             ORDER BY created_at DESC, id ASC
             LIMIT 1",
        )
        .bind(requirement_id)
        .bind(i64::try_from(requirement.revision()).map_err(|_| ReadinessError::InvalidRevision)?)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ReadinessError::StaleAssessment {
            assessment_revision: 0,
            current_revision: requirement.revision(),
        })?;
        let assessment = assessment.into_record()?.assessment;
        let packet =
            ReviewPacket::project(&requirement, &assessment).map_err(|error| match error {
                north_domain::readiness::PacketError::StaleAssessment {
                    assessment_revision,
                    current_revision,
                } => ReadinessError::StaleAssessment {
                    assessment_revision,
                    current_revision,
                },
            })?;
        transaction.commit().await?;
        Ok(packet)
    }
}

async fn assessment_by_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: &str,
) -> Result<Option<AssessmentRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, AssessmentRow>(
        "SELECT id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                requirement_revision, verdict, blockers, assumptions,
                repositories_reviewed, outcome, rejection_reason,
                assessed_at_ms, created_at::text AS created_at
         FROM readiness_assessments
         WHERE event_id = $1
         FOR UPDATE",
    )
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn session_requirement_row(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<Option<String>, ReadinessError> {
    let row = sqlx::query_as::<_, SessionRequirementRow>(
        "SELECT requirement_id
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.and_then(|row| row.requirement_id))
}

#[derive(Debug, FromRow)]
struct SessionRequirementRow {
    requirement_id: Option<String>,
}

async fn assessment_by_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    daemon_event_seq: i64,
) -> Result<Option<AssessmentRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, AssessmentRow>(
        "SELECT id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                requirement_revision, verdict, blockers, assumptions,
                repositories_reviewed, outcome, rejection_reason,
                assessed_at_ms, created_at::text AS created_at
         FROM readiness_assessments
         WHERE session_id = $1 AND daemon_event_seq = $2
         FOR UPDATE",
    )
    .bind(session_id)
    .bind(daemon_event_seq)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn insert_assessment(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AssessmentEvent<'_>,
    event_requirement_id: &str,
    requirement_id: Option<&str>,
    assessment: &ReadinessAssessment,
    outcome: AssessmentOutcome,
    rejection_reason: Option<&str>,
) -> Result<AssessmentRow, ReadinessError> {
    let assessed_at_ms =
        i64::try_from(assessment.assessed_at_ms).map_err(|_| ReadinessError::InvalidEvidence)?;
    let requirement_revision = i64::try_from(assessment.requirement_revision)
        .map_err(|_| ReadinessError::InvalidRevision)?;
    let repositories_reviewed = serde_json::Value::Array(
        assessment
            .repositories_reviewed
            .iter()
            .map(|repository| {
                serde_json::json!({
                    "repository_id": repository.repository_id,
                    "commit_sha": repository.commit_sha,
                })
            })
            .collect(),
    );
    Ok(sqlx::query_as::<_, AssessmentRow>(
        "INSERT INTO readiness_assessments
            (id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
             requirement_revision, verdict, blockers, assumptions,
             repositories_reviewed, outcome, rejection_reason, assessed_at_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         RETURNING id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                   requirement_revision, verdict, blockers, assumptions,
                   repositories_reviewed, outcome, rejection_reason,
                   assessed_at_ms, created_at::text AS created_at",
    )
    .bind(crate::random_hex(16))
    .bind(event.event_id)
    .bind(event.session_id)
    .bind(i64::try_from(event.daemon_event_seq).map_err(|_| ReadinessError::InvalidEvidence)?)
    .bind(event_requirement_id)
    .bind(requirement_id)
    .bind(requirement_revision)
    .bind(persisted_verdict(assessment.verdict))
    .bind(&assessment.blockers)
    .bind(&assessment.assumptions)
    .bind(repositories_reviewed)
    .bind(outcome.as_str())
    .bind(rejection_reason)
    .bind(assessed_at_ms)
    .fetch_one(&mut **transaction)
    .await?)
}

#[derive(Debug, FromRow)]
struct AssessmentRow {
    id: String,
    event_id: String,
    session_id: String,
    daemon_event_seq: i64,
    event_requirement_id: String,
    requirement_id: Option<String>,
    requirement_revision: i64,
    verdict: String,
    blockers: Vec<String>,
    assumptions: Vec<String>,
    repositories_reviewed: serde_json::Value,
    outcome: String,
    rejection_reason: Option<String>,
    assessed_at_ms: i64,
    created_at: String,
}

impl AssessmentRow {
    fn into_record(self) -> Result<ReadinessAssessmentRecord, ReadinessError> {
        let repositories_reviewed = self
            .repositories_reviewed
            .as_array()
            .ok_or(ReadinessError::InvalidEvidence)?
            .iter()
            .map(|value| {
                let repository_id = value
                    .get("repository_id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or(ReadinessError::InvalidEvidence)?
                    .to_owned();
                let commit_sha = value
                    .get("commit_sha")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or(ReadinessError::InvalidEvidence)?
                    .to_owned();
                Ok(ReviewedRepository {
                    repository_id,
                    commit_sha,
                })
            })
            .collect::<Result<Vec<_>, ReadinessError>>()?;
        Ok(ReadinessAssessmentRecord {
            id: self.id,
            event_id: self.event_id,
            session_id: self.session_id,
            daemon_event_seq: u64::try_from(self.daemon_event_seq)
                .map_err(|_| ReadinessError::InvalidEvidence)?,
            event_requirement_id: self.event_requirement_id,
            requirement_id: self.requirement_id,
            assessment: ReadinessAssessment {
                requirement_revision: u64::try_from(self.requirement_revision)
                    .map_err(|_| ReadinessError::InvalidRevision)?,
                verdict: match self.verdict.as_str() {
                    "ready" => Verdict::Ready,
                    "needs_clarification" => Verdict::NeedsClarification,
                    _ => return Err(ReadinessError::InvalidEvidence),
                },
                blockers: self.blockers,
                assumptions: self.assumptions,
                repositories_reviewed,
                assessed_at_ms: u64::try_from(self.assessed_at_ms)
                    .map_err(|_| ReadinessError::InvalidEvidence)?,
            },
            outcome: AssessmentOutcome::from_persisted(&self.outcome)?,
            rejection_reason: self.rejection_reason,
            created_at: self.created_at,
        })
    }
}

fn same_assessment_content(existing: &ReadinessAssessment, incoming: &ReadinessAssessment) -> bool {
    existing.requirement_revision == incoming.requirement_revision
        && existing.verdict == incoming.verdict
        && existing.blockers == incoming.blockers
        && existing.assumptions == incoming.assumptions
        && existing.repositories_reviewed == incoming.repositories_reviewed
}

fn persisted_verdict(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Ready => "ready",
        Verdict::NeedsClarification => "needs_clarification",
    }
}

fn persisted_status(status: RequirementStatus) -> &'static str {
    match status {
        RequirementStatus::Draft => "Draft",
        RequirementStatus::Discussing => "Discussing",
        RequirementStatus::Ready => "Ready",
        RequirementStatus::Accepted => "Accepted",
        RequirementStatus::Rejected => "Rejected",
    }
}

fn mark_ready_reason(error: &MarkReadyError) -> String {
    match error {
        MarkReadyError::StaleAssessment {
            assessment_revision,
            current_revision,
        } => format!("stale_assessment:{assessment_revision}!={current_revision}"),
        MarkReadyError::VerdictNotReady => "verdict_not_ready".into(),
        MarkReadyError::BlockersPresent => "blockers_present".into(),
        MarkReadyError::MissingAcceptanceCriteria => "missing_acceptance_criteria".into(),
        MarkReadyError::Transition(error) => {
            format!("invalid_transition:{:?}->{:?}", error.from, error.to)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_assessment_values_are_stable() {
        assert_eq!(AssessmentOutcome::Accepted.as_str(), "accepted");
        assert_eq!(
            persisted_verdict(Verdict::NeedsClarification),
            "needs_clarification"
        );
    }
}
