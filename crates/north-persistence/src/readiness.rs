use crate::{
    requirements::{lock_requirement, update_requirement, RequirementError},
    AuthStore,
};
use north_domain::{
    readiness::{
        AcceptedReadinessAssessment, ReadinessAssessment, ReviewPacket, ReviewedRepository, Verdict,
    },
    requirement::MarkReadyError,
    status::RequirementStatus,
};
use sha2::{Digest, Sha256};
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
    pub accepted_state_version: Option<u64>,
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
    SequenceGap {
        expected: u64,
        received: u64,
    },
    StaleAssessment {
        assessment_revision: u64,
        current_revision: u64,
    },
    StaleStateVersion {
        assessment_state_version: u64,
        current_state_version: u64,
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
            Self::SequenceGap { expected, received } => {
                write!(f, "assessment event sequence gap: expected {expected}, received {received}")
            }
            Self::StaleAssessment {
                assessment_revision,
                current_revision,
            } => write!(
                f,
                "assessment revision {assessment_revision} does not match current revision {current_revision}"
            ),
            Self::StaleStateVersion {
                assessment_state_version,
                current_state_version,
            } => write!(
                f,
                "assessment state version {assessment_state_version} does not match current state version {current_state_version}"
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
            RequirementError::InvalidStateVersion => Self::InvalidEvidence,
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
        let event_digest = readiness_event_digest(
            event_id,
            session_id,
            daemon_event_seq,
            requirement_id,
            assessment,
        )?;
        self.record_readiness_assessment_with_event_digest(
            event_id,
            session_id,
            daemon_event_seq,
            requirement_id,
            assessment,
            &event_digest,
        )
        .await
    }

    pub async fn record_readiness_assessment_with_event_digest(
        &self,
        event_id: &str,
        session_id: &str,
        daemon_event_seq: u64,
        requirement_id: &str,
        assessment: &ReadinessAssessment,
        event_digest: &str,
    ) -> Result<ReadinessAssessmentResult, ReadinessError> {
        if event_id.trim().is_empty()
            || session_id.trim().is_empty()
            || event_digest.trim().is_empty()
            || daemon_event_seq == 0
        {
            return Err(ReadinessError::InvalidEvidence);
        }
        if assessment.requirement_revision == 0
            || assessment.repositories_reviewed.iter().any(|repository| {
                repository.repository_id.trim().is_empty()
                    || !complete_commit_sha(&repository.commit_sha)
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
        let session_state = session_delivery_row(&mut transaction, event.session_id).await?;
        if session_state
            .as_ref()
            .and_then(|state| state.requirement_id.as_deref())
            != Some(requirement_id)
        {
            return Err(ReadinessError::SessionRequirementMismatch);
        }
        if let Some(row) = readiness_event_by_id(&mut transaction, event.event_id).await? {
            if row.session_id != event.session_id
                || row.daemon_event_seq != sequence
                || (!row.legacy_identity && row.payload_digest != event_digest)
            {
                return Err(ReadinessError::EventIdentityConflict);
            }
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
        let session_state = session_state.ok_or(ReadinessError::SessionRequirementMismatch)?;
        let mut expected_sequence = session_state
            .event_ack_through_seq
            .checked_add(1)
            .ok_or(ReadinessError::InvalidEvidence)?;
        while session_state.event_ack_sparse.contains(&expected_sequence) {
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or(ReadinessError::InvalidEvidence)?;
        }
        if sequence > expected_sequence {
            return Err(ReadinessError::SequenceGap {
                expected: u64::try_from(expected_sequence)
                    .map_err(|_| ReadinessError::InvalidEvidence)?,
                received: event.daemon_event_seq,
            });
        }
        if sequence <= session_state.event_ack_through_seq {
            return Err(ReadinessError::SequenceConflict);
        }
        if let Some(row) =
            assessment_by_sequence(&mut transaction, event.session_id, sequence).await?
        {
            if row.event_id != event.event_id {
                return Err(ReadinessError::SequenceConflict);
            }
        }
        let generic_event: Option<String> = sqlx::query_scalar(
            "SELECT event_id FROM server_event_dedupe
             WHERE session_id = $1 AND daemon_event_seq = $2",
        )
        .bind(event.session_id)
        .bind(sequence)
        .fetch_optional(&mut *transaction)
        .await?;
        if generic_event.is_some() {
            return Err(ReadinessError::SequenceConflict);
        }

        let row = match lock_requirement(&mut transaction, requirement_id).await {
            Ok(row) => row,
            Err(RequirementError::NotFound) => {
                let record_row = insert_assessment(
                    &mut transaction,
                    AssessmentInsert {
                        event: &event,
                        event_requirement_id: &event_requirement_id,
                        requirement_id: None,
                        assessment,
                        outcome: AssessmentOutcome::Rejected,
                        accepted_state_version: None,
                        rejection_reason: Some("requirement_not_found"),
                    },
                )
                .await?;
                insert_event_dedupe(
                    &mut transaction,
                    &event,
                    sequence,
                    event_digest,
                    AssessmentOutcome::Rejected,
                    Some("requirement_not_found"),
                )
                .await?;
                advance_event_watermark(&mut transaction, event.session_id, sequence).await?;
                transaction.commit().await?;
                return Ok(ReadinessAssessmentResult {
                    record: record_row.into_record()?,
                    duplicate: false,
                });
            }
            Err(error) => return Err(ReadinessError::from(error)),
        };
        let mut requirement = row.to_domain().map_err(ReadinessError::from)?;
        let expected_state_version = requirement.state_version();
        let citations_valid = repository_citations_exist(
            &mut transaction,
            &session_state.repository_ids,
            &assessment.repositories_reviewed,
        )
        .await?;
        let (outcome, rejection_reason) = if !citations_valid {
            (
                AssessmentOutcome::Rejected,
                Some("unknown_repository".to_owned()),
            )
        } else {
            match requirement.mark_ready(assessment) {
                Ok(()) => (AssessmentOutcome::Accepted, None),
                Err(error) => (AssessmentOutcome::Rejected, Some(mark_ready_reason(&error))),
            }
        };
        let accepted_state_version =
            (outcome == AssessmentOutcome::Accepted).then_some(requirement.state_version());
        let record_row = insert_assessment(
            &mut transaction,
            AssessmentInsert {
                event: &event,
                event_requirement_id: &event_requirement_id,
                requirement_id: Some(requirement_id),
                assessment,
                outcome,
                accepted_state_version,
                rejection_reason: rejection_reason.as_deref(),
            },
        )
        .await?;

        if outcome == AssessmentOutcome::Accepted {
            update_requirement(&mut transaction, &requirement, expected_state_version)
                .await
                .map_err(ReadinessError::from)?;
            sqlx::query(
                "INSERT INTO transition_audit
                    (requirement_id, actor_id, transition, from_status, to_status,
                     assessment_id, state_version)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(requirement.id())
            .bind(event_id)
            .bind("mark_ready")
            .bind(persisted_status(RequirementStatus::Discussing))
            .bind(persisted_status(requirement.status()))
            .bind(record_row.id.as_str())
            .bind(
                i64::try_from(requirement.state_version())
                    .map_err(|_| ReadinessError::InvalidEvidence)?,
            )
            .execute(&mut *transaction)
            .await?;
        }
        insert_event_dedupe(
            &mut transaction,
            &event,
            sequence,
            event_digest,
            outcome,
            rejection_reason.as_deref(),
        )
        .await?;
        advance_event_watermark(&mut transaction, event.session_id, sequence).await?;
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
                    repositories_reviewed, outcome, accepted_state_version,
                    rejection_reason,
                    assessed_at_ms, created_at::text AS created_at
             FROM readiness_assessments
             WHERE requirement_id = $1
               AND requirement_revision = $2
               AND accepted_state_version = $3
               AND generation_unknown = FALSE
               AND outcome = 'accepted'
             ORDER BY created_at DESC, id ASC
             LIMIT 1",
        )
        .bind(requirement_id)
        .bind(i64::try_from(requirement.revision()).map_err(|_| ReadinessError::InvalidRevision)?)
        .bind(
            i64::try_from(requirement.state_version())
                .map_err(|_| ReadinessError::InvalidEvidence)?,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ReadinessError::StaleAssessment {
            assessment_revision: 0,
            current_revision: requirement.revision(),
        })?;
        let assessment = assessment.into_record()?;
        let evidence = AcceptedReadinessAssessment {
            id: assessment.id,
            state_version: assessment
                .accepted_state_version
                .ok_or(ReadinessError::InvalidEvidence)?,
            assessment: assessment.assessment,
        };
        let packet =
            ReviewPacket::project(&requirement, &evidence).map_err(|error| match error {
                north_domain::readiness::PacketError::StaleAssessment {
                    assessment_revision,
                    current_revision,
                } => ReadinessError::StaleAssessment {
                    assessment_revision,
                    current_revision,
                },
                north_domain::readiness::PacketError::StaleStateVersion {
                    assessment_state_version,
                    current_state_version,
                } => ReadinessError::StaleStateVersion {
                    assessment_state_version,
                    current_state_version,
                },
                north_domain::readiness::PacketError::InvalidAssessmentIdentity
                | north_domain::readiness::PacketError::NotReady
                | north_domain::readiness::PacketError::InvalidAssessment => {
                    ReadinessError::InvalidEvidence
                }
            })?;
        transaction.commit().await?;
        Ok(packet)
    }
}

#[derive(Debug, FromRow)]
struct ReadinessEventDedupeRow {
    session_id: String,
    daemon_event_seq: i64,
    payload_digest: String,
    legacy_identity: bool,
}

async fn readiness_event_by_id(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: &str,
) -> Result<Option<ReadinessEventDedupeRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, ReadinessEventDedupeRow>(
        "SELECT session_id, daemon_event_seq, payload_digest, legacy_identity
         FROM server_event_dedupe
         WHERE event_id = $1
         FOR UPDATE",
    )
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn insert_event_dedupe(
    transaction: &mut Transaction<'_, Postgres>,
    event: &AssessmentEvent<'_>,
    sequence: i64,
    payload_digest: &str,
    outcome: AssessmentOutcome,
    rejection_reason: Option<&str>,
) -> Result<(), ReadinessError> {
    sqlx::query(
        "INSERT INTO server_event_dedupe
            (event_id, session_id, daemon_event_seq, payload_digest, payload,
             outcome, rejection_reason)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(event.event_id)
    .bind(event.session_id)
    .bind(sequence)
    .bind(payload_digest)
    .bind(payload_digest)
    .bind(outcome.as_str())
    .bind(rejection_reason)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn assessment_by_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: &str,
) -> Result<Option<AssessmentRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, AssessmentRow>(
        "SELECT id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                requirement_revision, verdict, blockers, assumptions,
                repositories_reviewed, outcome, accepted_state_version,
                    rejection_reason,
                assessed_at_ms, created_at::text AS created_at
         FROM readiness_assessments
         WHERE event_id = $1
         FOR UPDATE",
    )
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn session_delivery_row(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<Option<SessionDeliveryRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, SessionDeliveryRow>(
        "SELECT requirement_id, event_ack_through_seq, event_ack_sparse, repository_ids
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

#[derive(Debug, FromRow)]
struct SessionDeliveryRow {
    requirement_id: Option<String>,
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
    repository_ids: Vec<String>,
}

async fn advance_event_watermark(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    sequence: i64,
) -> Result<(), ReadinessError> {
    let mut cursor = sqlx::query_as::<_, EventCursorRow>(
        "SELECT event_ack_through_seq, event_ack_sparse
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    cursor.event_ack_through_seq = cursor.event_ack_through_seq.max(sequence);
    loop {
        let next = cursor.event_ack_through_seq.saturating_add(1);
        let Some(position) = cursor
            .event_ack_sparse
            .iter()
            .position(|value| *value == next)
        else {
            break;
        };
        cursor.event_ack_sparse.remove(position);
        cursor.event_ack_through_seq = next;
    }
    cursor
        .event_ack_sparse
        .retain(|value| *value > cursor.event_ack_through_seq);
    sqlx::query(
        "UPDATE execution_sessions
         SET event_ack_through_seq = $2, event_ack_sparse = $3
         WHERE id = $1",
    )
    .bind(session_id)
    .bind(cursor.event_ack_through_seq)
    .bind(cursor.event_ack_sparse)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct EventCursorRow {
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
}

// Git currently supports SHA-1 and SHA-256 object formats. Accept both
// canonical widths; never require one fixed width for repository evidence.
const GIT_OBJECT_ID_HEX_WIDTHS: &[usize] = &[20 * 2, 32 * 2];

fn complete_commit_sha(value: &str) -> bool {
    GIT_OBJECT_ID_HEX_WIDTHS.contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn repository_citations_exist(
    transaction: &mut Transaction<'_, Postgres>,
    session_repository_ids: &[String],
    repositories: &[ReviewedRepository],
) -> Result<bool, ReadinessError> {
    for repository in repositories {
        if !session_repository_ids
            .iter()
            .any(|repository_id| repository_id == &repository.repository_id)
        {
            return Ok(false);
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM repositories WHERE id = $1)")
                .bind(&repository.repository_id)
                .fetch_one(&mut **transaction)
                .await?;
        if !exists {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn assessment_by_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    daemon_event_seq: i64,
) -> Result<Option<AssessmentRow>, ReadinessError> {
    Ok(sqlx::query_as::<_, AssessmentRow>(
        "SELECT id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                requirement_revision, verdict, blockers, assumptions,
                repositories_reviewed, outcome, accepted_state_version,
                    rejection_reason,
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

struct AssessmentInsert<'a> {
    event: &'a AssessmentEvent<'a>,
    event_requirement_id: &'a str,
    requirement_id: Option<&'a str>,
    assessment: &'a ReadinessAssessment,
    outcome: AssessmentOutcome,
    accepted_state_version: Option<u64>,
    rejection_reason: Option<&'a str>,
}

async fn insert_assessment(
    transaction: &mut Transaction<'_, Postgres>,
    input: AssessmentInsert<'_>,
) -> Result<AssessmentRow, ReadinessError> {
    let AssessmentInsert {
        event,
        event_requirement_id,
        requirement_id,
        assessment,
        outcome,
        accepted_state_version,
        rejection_reason,
    } = input;
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
             repositories_reviewed, outcome, accepted_state_version,
             rejection_reason, assessed_at_ms)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         RETURNING id, event_id, session_id, daemon_event_seq, event_requirement_id, requirement_id,
                   requirement_revision, verdict, blockers, assumptions,
                   repositories_reviewed, outcome, accepted_state_version,
                    rejection_reason,
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
    .bind(
        accepted_state_version
            .map(i64::try_from)
            .transpose()
            .map_err(|_| ReadinessError::InvalidEvidence)?,
    )
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
    accepted_state_version: Option<i64>,
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
                    .filter(|value| complete_commit_sha(value))
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
            accepted_state_version: self
                .accepted_state_version
                .map(u64::try_from)
                .transpose()
                .map_err(|_| ReadinessError::InvalidEvidence)?,
            rejection_reason: self.rejection_reason,
            created_at: self.created_at,
        })
    }
}

fn readiness_event_digest(
    event_id: &str,
    session_id: &str,
    daemon_event_seq: u64,
    requirement_id: &str,
    assessment: &ReadinessAssessment,
) -> Result<String, ReadinessError> {
    let repositories_reviewed = assessment
        .repositories_reviewed
        .iter()
        .map(|repository| {
            serde_json::json!({
                "repository_id": repository.repository_id,
                "commit_sha": repository.commit_sha,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "event_id": event_id,
        "session_id": session_id,
        "daemon_event_seq": daemon_event_seq,
        "requirement_id": requirement_id,
        "assessment": {
            "requirement_revision": assessment.requirement_revision,
            "verdict": persisted_verdict(assessment.verdict),
            "blockers": assessment.blockers,
            "assumptions": assessment.assumptions,
            "repositories_reviewed": repositories_reviewed,
        },
    });
    let bytes = serde_json::to_vec(&value).map_err(|_| ReadinessError::InvalidEvidence)?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
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
    fn complete_commit_sha_accepts_supported_object_widths() {
        assert!(complete_commit_sha(&"a".repeat(40)));
        assert!(complete_commit_sha(&"b".repeat(64)));
        assert!(!complete_commit_sha("abc123"));
        assert!(!complete_commit_sha(&format!("{} ", "a".repeat(40))));
        assert!(!complete_commit_sha(&"g".repeat(40)));
    }

    #[test]
    fn persisted_assessment_rejects_incomplete_repository_sha() {
        let row = AssessmentRow {
            id: "assessment".into(),
            event_id: "event".into(),
            session_id: "session".into(),
            daemon_event_seq: 1,
            event_requirement_id: "requirement".into(),
            requirement_id: Some("requirement".into()),
            requirement_revision: 1,
            verdict: "ready".into(),
            blockers: Vec::new(),
            assumptions: Vec::new(),
            repositories_reviewed: serde_json::json!([{
                "repository_id": "repo",
                "commit_sha": "abc123"
            }]),
            outcome: "accepted".into(),
            accepted_state_version: Some(1),
            rejection_reason: None,
            assessed_at_ms: 1,
            created_at: "now".into(),
        };
        assert!(matches!(
            row.into_record(),
            Err(ReadinessError::InvalidEvidence)
        ));
    }

    #[test]
    fn persisted_assessment_values_are_stable() {
        assert_eq!(AssessmentOutcome::Accepted.as_str(), "accepted");
        assert_eq!(
            persisted_verdict(Verdict::NeedsClarification),
            "needs_clarification"
        );
    }
}
