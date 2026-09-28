use crate::AuthStore;
use north_domain::{
    requirement::{EditError, PersistedRequirement, Requirement, RequirementEdit, RestoreError},
    status::{InvalidTransition, RequirementStatus},
};
use sea_orm::{DatabaseTransaction, DbErr, FromQueryResult, TransactionTrait};
use std::{error::Error, fmt};

/// Persisted requirement projection returned to server handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementRecord {
    pub id: String,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub status: RequirementStatus,
    pub revision: u64,
    pub state_version: u64,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequirementSort {
    UpdatedAscending,
    #[default]
    UpdatedDescending,
    Board,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementListQuery {
    pub search: Option<String>,
    pub status: Option<RequirementStatus>,
    pub created_by: Option<String>,
    pub sort: RequirementSort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementTransition {
    BeginDiscussion,
    Accept,
    Reject,
    RequestChanges,
    Reopen,
}

impl RequirementTransition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeginDiscussion => "begin_discussion",
            Self::Accept => "accept",
            Self::Reject => "reject",
            Self::RequestChanges => "request_changes",
            Self::Reopen => "reopen",
        }
    }

    fn apply(self, requirement: &mut Requirement) -> Result<(), RequirementError> {
        match self {
            Self::BeginDiscussion => requirement.begin_discussion(),
            Self::Accept => requirement.accept(),
            Self::Reject => requirement.reject(),
            Self::RequestChanges => requirement.request_changes(),
            Self::Reopen => requirement.reopen(),
        }
        .map_err(Into::into)
    }
}

#[derive(Debug)]
pub enum RequirementError {
    Database(DbErr),
    NotFound,
    Conflict,
    InvalidStatus(String),
    InvalidRevision,
    InvalidStateVersion,
    InvalidTransition(InvalidTransition),
    Edit(EditError),
}

impl fmt::Display for RequirementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::NotFound => f.write_str("requirement not found"),
            Self::Conflict => f.write_str("requirement state version conflict"),
            Self::InvalidStatus(status) => write!(f, "invalid requirement status: {status}"),
            Self::InvalidRevision => f.write_str("invalid requirement revision"),
            Self::InvalidStateVersion => f.write_str("invalid requirement state version"),
            Self::InvalidTransition(error) => {
                write!(
                    f,
                    "invalid requirement transition: {:?} -> {:?}",
                    error.from, error.to
                )
            }
            Self::Edit(error) => write!(f, "requirement edit refused: {error:?}"),
        }
    }
}

impl Error for RequirementError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DbErr> for RequirementError {
    fn from(error: DbErr) -> Self {
        Self::Database(error)
    }
}

impl From<RestoreError> for RequirementError {
    fn from(error: RestoreError) -> Self {
        match error {
            RestoreError::InvalidRevision => Self::InvalidRevision,
            RestoreError::InvalidStateVersion => Self::InvalidStateVersion,
        }
    }
}

impl From<InvalidTransition> for RequirementError {
    fn from(error: InvalidTransition) -> Self {
        Self::InvalidTransition(error)
    }
}

impl From<EditError> for RequirementError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

impl AuthStore {
    pub async fn create_requirement(
        &self,
        title: &str,
        description: &str,
        created_by: &str,
    ) -> Result<RequirementRecord, RequirementError> {
        let requirement = Requirement::new(crate::random_hex(16), title, description, created_by);
        let mut transaction = self.pool.begin().await?;
        lock_board_order(&mut transaction).await?;
        let row = crate::query::query_as::<RequirementRow>(
            "INSERT INTO requirements
                (id, title, description, summary, acceptance_criteria,
                 assumptions, open_questions, status, revision, state_version,
                 created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id, title, description, summary, acceptance_criteria,
                       assumptions, open_questions, status, revision, state_version,
                       created_by, created_at::text AS created_at,
                       updated_at::text AS updated_at",
        )
        .bind(requirement.id())
        .bind(requirement.title())
        .bind(requirement.description())
        .bind(requirement.summary())
        .bind(requirement.acceptance_criteria().to_vec())
        .bind(requirement.assumptions().to_vec())
        .bind(requirement.open_questions().to_vec())
        .bind(persisted_status(requirement.status()))
        .bind(i64::try_from(requirement.revision()).map_err(|_| RequirementError::InvalidRevision)?)
        .bind(
            i64::try_from(requirement.state_version())
                .map_err(|_| RequirementError::InvalidStateVersion)?,
        )
        .bind(requirement.created_by())
        .fetch_one(&transaction)
        .await?;
        crate::query::query(
            "INSERT INTO conversations (id, requirement_id)
             VALUES ($1, $2)",
        )
        .bind(crate::random_hex(16))
        .bind(requirement.id())
        .execute(&transaction)
        .await?;
        append_board_position(
            &mut transaction,
            requirement.id(),
            persisted_status(requirement.status()),
        )
        .await?;
        transaction.commit().await?;
        row.into_record()
    }

    pub async fn requirement_by_id(
        &self,
        requirement_id: &str,
    ) -> Result<Option<RequirementRecord>, RequirementError> {
        let row = crate::query::query_as::<RequirementRow>(
            "SELECT id, title, description, summary, acceptance_criteria,
                    assumptions, open_questions, status, revision, state_version,
                    created_by, created_at::text AS created_at,
                    updated_at::text AS updated_at
             FROM requirements
             WHERE id = $1",
        )
        .bind(requirement_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(RequirementRow::into_record).transpose()
    }

    pub async fn list_requirements(
        &self,
        query: &RequirementListQuery,
    ) -> Result<Vec<RequirementRecord>, RequirementError> {
        let ascending = matches!(query.sort, RequirementSort::UpdatedAscending);
        let board = matches!(query.sort, RequirementSort::Board);
        let status = query.status.map(persisted_status);
        let rows = crate::query::query_as::<RequirementRow>(
            "SELECT id, title, description, summary, acceptance_criteria,
                    assumptions, open_questions, status, revision, state_version,
                    created_by, created_at::text AS created_at,
                    updated_at::text AS updated_at
             FROM requirements
             LEFT JOIN requirement_board_positions AS board_position
               ON board_position.requirement_id = requirements.id
             WHERE ($1::text IS NULL OR title ILIKE '%' || $1 || '%'
                    OR description ILIKE '%' || $1 || '%'
                    OR summary ILIKE '%' || $1 || '%'
                    OR array_to_string(acceptance_criteria, ' ') ILIKE '%' || $1 || '%'
                    OR array_to_string(assumptions, ' ') ILIKE '%' || $1 || '%'
                    OR array_to_string(open_questions, ' ') ILIKE '%' || $1 || '%')
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR created_by = $3)
             ORDER BY CASE WHEN $5 THEN CASE requirements.status
                           WHEN 'Draft' THEN 0
                           WHEN 'Discussing' THEN 1
                           WHEN 'Ready' THEN 2
                           WHEN 'Accepted' THEN 3
                           WHEN 'Rejected' THEN 4
                           ELSE 5 END END ASC,
                      CASE WHEN $5 THEN board_position.rank END ASC NULLS LAST,
                      CASE WHEN $5 THEN requirements.created_at END ASC,
                      CASE WHEN $5 THEN requirements.id END ASC,
                      CASE WHEN NOT $5 AND $4 THEN requirements.updated_at END ASC,
                      CASE WHEN NOT $5 AND NOT $4 THEN requirements.updated_at END DESC,
                      requirements.id ASC",
        )
        .bind(query.search.as_deref())
        .bind(status)
        .bind(query.created_by.as_deref())
        .bind(ascending)
        .bind(board)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(RequirementRow::into_record).collect()
    }

    pub async fn reorder_requirement(
        &self,
        requirement_id: &str,
        expected_state_version: u64,
        before_id: Option<&str>,
        after_id: Option<&str>,
    ) -> Result<bool, RequirementError> {
        let expected_state_version = i64::try_from(expected_state_version)
            .map_err(|_| RequirementError::InvalidStateVersion)?;
        let mut transaction = self.pool.begin().await?;
        lock_board_order(&mut transaction).await?;
        let requirement = lock_requirement(&mut transaction, requirement_id).await?;
        if requirement.state_version != expected_state_version {
            return Err(RequirementError::Conflict);
        }
        if before_id == Some(requirement_id)
            || after_id == Some(requirement_id)
            || (before_id.is_some() && before_id == after_id)
        {
            return Err(RequirementError::Conflict);
        }
        let status = requirement.status.clone();
        ensure_board_column_positions(&mut transaction, &status).await?;
        let mut positions = board_positions_in_column(&mut transaction, &status).await?;
        let current_order: Vec<String> = positions
            .iter()
            .map(|position| position.requirement_id.clone())
            .collect();
        let remaining: Vec<String> = current_order
            .iter()
            .filter(|id| id.as_str() != requirement_id)
            .cloned()
            .collect();
        let before_index = before_id
            .map(|id| {
                remaining
                    .iter()
                    .position(|candidate| candidate.as_str() == id)
                    .ok_or(RequirementError::Conflict)
            })
            .transpose()?;
        let after_index = after_id
            .map(|id| {
                remaining
                    .iter()
                    .position(|candidate| candidate.as_str() == id)
                    .ok_or(RequirementError::Conflict)
            })
            .transpose()?;
        let insertion_index = match (before_index, after_index) {
            (None, None) if remaining.is_empty() => 0,
            (None, Some(0)) => 0,
            (Some(before), None) if before + 1 == remaining.len() => remaining.len(),
            (Some(before), Some(after)) if before + 1 == after => after,
            _ => return Err(RequirementError::Conflict),
        };
        let mut final_order = remaining;
        final_order.insert(insertion_index, requirement_id.to_owned());
        if final_order == current_order {
            transaction.commit().await?;
            return Ok(false);
        }

        let mut rank = candidate_rank_for_order(&positions, &final_order, insertion_index)?;
        if rank.is_none() {
            rebalance_board_column(&mut transaction, &status).await?;
            positions = board_positions_in_column(&mut transaction, &status).await?;
            rank = candidate_rank_for_order(&positions, &final_order, insertion_index)?;
        }
        let rank = rank.ok_or(RequirementError::Conflict)?;
        crate::query::query(
            "UPDATE requirement_board_positions SET rank = $2 WHERE requirement_id = $1",
        )
        .bind(requirement_id)
        .bind(rank)
        .execute(&transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn transition_requirement(
        &self,
        requirement_id: &str,
        expected_state_version: u64,
        actor_id: &str,
        transition: RequirementTransition,
    ) -> Result<RequirementRecord, RequirementError> {
        self.transition_requirement_with_feedback(
            requirement_id,
            expected_state_version,
            actor_id,
            transition,
            None,
            None,
        )
        .await
    }

    pub async fn transition_requirement_with_feedback(
        &self,
        requirement_id: &str,
        expected_state_version: u64,
        actor_id: &str,
        transition: RequirementTransition,
        feedback: Option<&str>,
        assessment_id: Option<&str>,
    ) -> Result<RequirementRecord, RequirementError> {
        let mut transaction = self.pool.begin().await?;
        lock_board_order(&mut transaction).await?;
        let row = lock_requirement(&mut transaction, requirement_id).await?;
        let mut requirement = row.to_domain()?;
        if requirement.state_version() != expected_state_version {
            return Err(RequirementError::Conflict);
        }
        match transition {
            RequirementTransition::Accept
            | RequirementTransition::Reject
            | RequirementTransition::RequestChanges => {
                let assessment_id = assessment_id
                    .filter(|value| !value.trim().is_empty())
                    .ok_or(RequirementError::Conflict)?;
                current_review_assessment(
                    &mut transaction,
                    requirement.id(),
                    requirement.revision(),
                    requirement.state_version(),
                    assessment_id,
                )
                .await?;
            }
            RequirementTransition::BeginDiscussion | RequirementTransition::Reopen => {
                if assessment_id.is_some() {
                    return Err(RequirementError::Conflict);
                }
            }
        }
        let from_status = requirement.status();
        transition.apply(&mut requirement)?;
        let updated =
            update_requirement(&mut transaction, &requirement, expected_state_version).await?;
        crate::query::query(
            "INSERT INTO transition_audit
                (requirement_id, actor_id, transition, from_status, to_status,
                 feedback, assessment_id, state_version)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(requirement.id())
        .bind(actor_id)
        .bind(transition.as_str())
        .bind(persisted_status(from_status))
        .bind(persisted_status(requirement.status()))
        .bind(feedback)
        .bind(assessment_id)
        .bind(
            i64::try_from(requirement.state_version())
                .map_err(|_| RequirementError::InvalidStateVersion)?,
        )
        .execute(&transaction)
        .await?;
        transaction.commit().await?;
        updated.into_record()
    }

    pub async fn edit_requirement(
        &self,
        requirement_id: &str,
        expected_state_version: u64,
        edit: &RequirementEdit,
    ) -> Result<RequirementRecord, RequirementError> {
        self.edit_requirement_with_actor(requirement_id, expected_state_version, "system", edit)
            .await
    }

    pub async fn edit_requirement_with_actor(
        &self,
        requirement_id: &str,
        expected_state_version: u64,
        actor_id: &str,
        edit: &RequirementEdit,
    ) -> Result<RequirementRecord, RequirementError> {
        let mut transaction = self.pool.begin().await?;
        lock_board_order(&mut transaction).await?;
        let row = lock_requirement(&mut transaction, requirement_id).await?;
        let original = row.clone().into_record()?;
        let mut requirement = row.to_domain()?;
        if requirement.state_version() != expected_state_version {
            return Err(RequirementError::Conflict);
        }
        let was_ready = requirement.status() == RequirementStatus::Ready;
        requirement.apply_edit(edit)?;
        if requirement.state_version() == expected_state_version {
            transaction.commit().await?;
            return Ok(original);
        }
        let updated =
            update_requirement(&mut transaction, &requirement, expected_state_version).await?;
        if was_ready {
            crate::query::query(
                "INSERT INTO transition_audit
                    (requirement_id, actor_id, transition, from_status, to_status,
                     feedback, assessment_id, state_version)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(requirement.id())
            .bind(actor_id)
            .bind("edit_demotes_ready")
            .bind(persisted_status(RequirementStatus::Ready))
            .bind(persisted_status(requirement.status()))
            .bind(Option::<&str>::None)
            .bind(Option::<&str>::None)
            .bind(
                i64::try_from(requirement.state_version())
                    .map_err(|_| RequirementError::InvalidStateVersion)?,
            )
            .execute(&transaction)
            .await?;
        }
        transaction.commit().await?;
        updated.into_record()
    }
}

const BOARD_RANK_GAP: i64 = 1024;
// ponytail: one global order lock; shard by status if write throughput matters.
const BOARD_ORDER_ADVISORY_LOCK_KEY: i64 = 0x4e4f525448424f41;

#[derive(Debug, FromQueryResult)]
struct BoardPositionRow {
    requirement_id: String,
    rank: i64,
}

pub(crate) async fn lock_board_order(
    transaction: &mut DatabaseTransaction,
) -> Result<(), RequirementError> {
    crate::query::query("SELECT pg_advisory_xact_lock($1)")
        .bind(BOARD_ORDER_ADVISORY_LOCK_KEY)
        .execute(&*transaction)
        .await?;
    Ok(())
}

async fn board_positions_in_column(
    transaction: &mut DatabaseTransaction,
    status: &str,
) -> Result<Vec<BoardPositionRow>, RequirementError> {
    Ok(crate::query::query_as::<BoardPositionRow>(
        "SELECT position.requirement_id, position.rank
         FROM requirement_board_positions AS position
         INNER JOIN requirements ON requirements.id = position.requirement_id
         WHERE requirements.status = $1
         ORDER BY position.rank ASC, requirements.created_at ASC, requirements.id ASC",
    )
    .bind(status)
    .fetch_all(&*transaction)
    .await?)
}

async fn rebalance_board_column(
    transaction: &mut DatabaseTransaction,
    status: &str,
) -> Result<(), RequirementError> {
    crate::query::query(
        "WITH ordered AS (
             SELECT position.requirement_id,
                    ROW_NUMBER() OVER (
                        ORDER BY position.rank ASC, requirements.created_at ASC, requirements.id ASC
                    ) * $2 AS new_rank
             FROM requirement_board_positions AS position
             INNER JOIN requirements ON requirements.id = position.requirement_id
             WHERE requirements.status = $1
         )
         UPDATE requirement_board_positions AS position
         SET rank = ordered.new_rank
         FROM ordered
         WHERE position.requirement_id = ordered.requirement_id",
    )
    .bind(status)
    .bind(BOARD_RANK_GAP)
    .execute(&*transaction)
    .await?;
    Ok(())
}

async fn ensure_board_column_positions(
    transaction: &mut DatabaseTransaction,
    status: &str,
) -> Result<(), RequirementError> {
    let missing: i64 = crate::query::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM requirements
         LEFT JOIN requirement_board_positions AS position
           ON position.requirement_id = requirements.id
         WHERE requirements.status = $1 AND position.requirement_id IS NULL",
    )
    .bind(status)
    .fetch_one(&*transaction)
    .await?;
    if missing == 0 {
        return Ok(());
    }

    rebalance_board_column(transaction, status).await?;
    crate::query::query(
        "WITH base AS (
             SELECT COALESCE(MAX(position.rank), 0) AS rank
             FROM requirement_board_positions AS position
             INNER JOIN requirements ON requirements.id = position.requirement_id
             WHERE requirements.status = $1
         ), missing AS (
             SELECT requirements.id,
                    ROW_NUMBER() OVER (ORDER BY requirements.created_at ASC, requirements.id ASC) AS ordinal
             FROM requirements
             LEFT JOIN requirement_board_positions AS position
               ON position.requirement_id = requirements.id
             WHERE requirements.status = $1 AND position.requirement_id IS NULL
         )
         INSERT INTO requirement_board_positions (requirement_id, rank)
         SELECT missing.id, base.rank + missing.ordinal * $2
         FROM missing CROSS JOIN base",
    )
    .bind(status)
    .bind(BOARD_RANK_GAP)
    .execute(&*transaction)
    .await?;
    Ok(())
}

async fn append_board_position(
    transaction: &mut DatabaseTransaction,
    requirement_id: &str,
    status: &str,
) -> Result<(), RequirementError> {
    let mut max_rank: Option<i64> = crate::query::query_scalar(
        "SELECT MAX(position.rank)
         FROM requirement_board_positions AS position
         INNER JOIN requirements ON requirements.id = position.requirement_id
         WHERE requirements.status = $1 AND requirements.id <> $2",
    )
    .bind(status)
    .bind(requirement_id)
    .fetch_one(&*transaction)
    .await?;
    if max_rank.is_some_and(|rank| rank.checked_add(BOARD_RANK_GAP).is_none()) {
        rebalance_board_column(transaction, status).await?;
        max_rank = crate::query::query_scalar(
            "SELECT MAX(position.rank)
             FROM requirement_board_positions AS position
             INNER JOIN requirements ON requirements.id = position.requirement_id
             WHERE requirements.status = $1 AND requirements.id <> $2",
        )
        .bind(status)
        .bind(requirement_id)
        .fetch_one(&*transaction)
        .await?;
    }
    let rank = max_rank
        .unwrap_or(0)
        .checked_add(BOARD_RANK_GAP)
        .ok_or(RequirementError::Conflict)?;
    crate::query::query(
        "INSERT INTO requirement_board_positions (requirement_id, rank)
         VALUES ($1, $2)
         ON CONFLICT (requirement_id) DO UPDATE SET rank = EXCLUDED.rank",
    )
    .bind(requirement_id)
    .bind(rank)
    .execute(&*transaction)
    .await?;
    Ok(())
}

fn candidate_board_rank(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (None, None) => Some(BOARD_RANK_GAP),
        (None, Some(right)) => right.checked_sub(BOARD_RANK_GAP),
        (Some(left), None) => left.checked_add(BOARD_RANK_GAP),
        (Some(left), Some(right)) => {
            let gap = i128::from(right) - i128::from(left);
            (gap > 1).then(|| (i128::from(left) + gap / 2) as i64)
        }
    }
}

fn candidate_rank_for_order(
    positions: &[BoardPositionRow],
    final_order: &[String],
    insertion_index: usize,
) -> Result<Option<i64>, RequirementError> {
    let rank_for = |id: Option<&str>| -> Result<Option<i64>, RequirementError> {
        id.map(|id| {
            positions
                .iter()
                .find(|position| position.requirement_id == id)
                .map(|position| position.rank)
                .ok_or(RequirementError::Conflict)
        })
        .transpose()
    };
    let left_id = insertion_index
        .checked_sub(1)
        .map(|index| final_order[index].as_str());
    let right_id = final_order.get(insertion_index + 1).map(String::as_str);
    Ok(candidate_board_rank(
        rank_for(left_id)?,
        rank_for(right_id)?,
    ))
}

pub(crate) async fn lock_requirement(
    transaction: &mut DatabaseTransaction,
    requirement_id: &str,
) -> Result<RequirementRow, RequirementError> {
    crate::query::query_as::<RequirementRow>(
        "SELECT id, title, description, summary, acceptance_criteria,
                assumptions, open_questions, status, revision, state_version,
                created_by, created_at::text AS created_at,
                updated_at::text AS updated_at
         FROM requirements
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(requirement_id)
    .fetch_optional(&*transaction)
    .await?
    .ok_or(RequirementError::NotFound)
}

async fn current_review_assessment(
    transaction: &mut DatabaseTransaction,
    requirement_id: &str,
    requirement_revision: u64,
    requirement_state_version: u64,
    assessment_id: &str,
) -> Result<(), RequirementError> {
    let current_id = crate::query::query_scalar::<String>(
        "SELECT id
         FROM readiness_assessments
         WHERE id = $4
           AND requirement_id = $1
           AND requirement_revision = $2
           AND accepted_state_version = $3
           AND generation_unknown = FALSE
           AND outcome = 'accepted'",
    )
    .bind(requirement_id)
    .bind(i64::try_from(requirement_revision).map_err(|_| RequirementError::InvalidRevision)?)
    .bind(
        i64::try_from(requirement_state_version)
            .map_err(|_| RequirementError::InvalidStateVersion)?,
    )
    .bind(assessment_id)
    .fetch_optional(&*transaction)
    .await?;
    if current_id.as_deref() == Some(assessment_id) {
        Ok(())
    } else {
        Err(RequirementError::Conflict)
    }
}

pub(crate) async fn update_requirement(
    transaction: &mut DatabaseTransaction,
    requirement: &Requirement,
    expected_state_version: u64,
) -> Result<RequirementRow, RequirementError> {
    let previous_status: String =
        crate::query::query_scalar("SELECT status FROM requirements WHERE id = $1")
            .bind(requirement.id())
            .fetch_one(&*transaction)
            .await?;
    let row = crate::query::query_as::<RequirementRow>(
        "UPDATE requirements
         SET title = $2,
             description = $3,
             summary = $4,
             acceptance_criteria = $5,
             assumptions = $6,
             open_questions = $7,
             status = $8,
             revision = $9,
             state_version = $10,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND state_version = $11
         RETURNING id, title, description, summary, acceptance_criteria,
                   assumptions, open_questions, status, revision, state_version,
                   created_by, created_at::text AS created_at,
                   updated_at::text AS updated_at",
    )
    .bind(requirement.id())
    .bind(requirement.title())
    .bind(requirement.description())
    .bind(requirement.summary())
    .bind(requirement.acceptance_criteria().to_vec())
    .bind(requirement.assumptions().to_vec())
    .bind(requirement.open_questions().to_vec())
    .bind(persisted_status(requirement.status()))
    .bind(i64::try_from(requirement.revision()).map_err(|_| RequirementError::InvalidRevision)?)
    .bind(
        i64::try_from(requirement.state_version())
            .map_err(|_| RequirementError::InvalidStateVersion)?,
    )
    .bind(i64::try_from(expected_state_version).map_err(|_| RequirementError::InvalidStateVersion)?)
    .fetch_optional(&*transaction)
    .await?
    .ok_or(RequirementError::Conflict)?;
    if previous_status != persisted_status(requirement.status()) {
        append_board_position(
            transaction,
            requirement.id(),
            persisted_status(requirement.status()),
        )
        .await?;
    }
    Ok(row)
}

#[derive(Debug, Clone, FromQueryResult)]
pub(crate) struct RequirementRow {
    id: String,
    title: String,
    description: String,
    summary: String,
    acceptance_criteria: Vec<String>,
    assumptions: Vec<String>,
    open_questions: Vec<String>,
    status: String,
    revision: i64,
    state_version: i64,
    created_by: String,
    created_at: String,
    updated_at: String,
}

impl RequirementRow {
    pub(crate) fn status(&self) -> Result<RequirementStatus, RequirementError> {
        match self.status.as_str() {
            "Draft" => Ok(RequirementStatus::Draft),
            "Discussing" => Ok(RequirementStatus::Discussing),
            "Ready" => Ok(RequirementStatus::Ready),
            "Accepted" => Ok(RequirementStatus::Accepted),
            "Rejected" => Ok(RequirementStatus::Rejected),
            status => Err(RequirementError::InvalidStatus(status.to_owned())),
        }
    }

    pub(crate) fn to_domain(&self) -> Result<Requirement, RequirementError> {
        Ok(Requirement::from_persisted(PersistedRequirement {
            id: self.id.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            summary: self.summary.clone(),
            acceptance_criteria: self.acceptance_criteria.clone(),
            assumptions: self.assumptions.clone(),
            open_questions: self.open_questions.clone(),
            status: self.status()?,
            revision: u64::try_from(self.revision)
                .map_err(|_| RequirementError::InvalidRevision)?,
            state_version: u64::try_from(self.state_version)
                .map_err(|_| RequirementError::InvalidStateVersion)?,
            created_by: self.created_by.clone(),
        })?)
    }

    pub(crate) fn into_record(self) -> Result<RequirementRecord, RequirementError> {
        self.to_domain()?;
        let status = self.status()?;
        let revision =
            u64::try_from(self.revision).map_err(|_| RequirementError::InvalidRevision)?;
        let state_version =
            u64::try_from(self.state_version).map_err(|_| RequirementError::InvalidStateVersion)?;
        Ok(RequirementRecord {
            id: self.id,
            title: self.title,
            description: self.description,
            summary: self.summary,
            acceptance_criteria: self.acceptance_criteria,
            assumptions: self.assumptions,
            open_questions: self.open_questions,
            status,
            revision,
            state_version,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_labels_are_stable() {
        assert_eq!(
            RequirementTransition::BeginDiscussion.as_str(),
            "begin_discussion"
        );
        assert_eq!(
            RequirementTransition::RequestChanges.as_str(),
            "request_changes"
        );
    }

    #[test]
    fn persisted_status_matches_database_values() {
        assert_eq!(persisted_status(RequirementStatus::Draft), "Draft");
        assert_eq!(persisted_status(RequirementStatus::Ready), "Ready");
    }
}
