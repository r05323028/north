use crate::{
    requirements::{lock_requirement, update_requirement},
    AuthStore, PersistenceError,
};
use north_domain::status::RequirementStatus;
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction};
use std::{error::Error, fmt};

pub const MAX_CONTEXT_MESSAGES: usize = 50;
pub const MAX_CONTEXT_BYTES: usize = 32 * 1024;
const MAX_ACTIVITY_CHARS: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClarificationPhase {
    AwaitingAssignment,
    Active,
    Terminal,
}

impl ClarificationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingAssignment => "awaiting_assignment",
            Self::Active => "active",
            Self::Terminal => "terminal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClarificationStatus {
    Starting,
    Running,
    Completed,
    Unavailable,
}

impl ClarificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarificationRun {
    pub run_id: String,
    pub requirement_id: String,
    pub start_message_id: String,
    pub phase: ClarificationPhase,
    pub status: ClarificationStatus,
    pub cancel_requested: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub(crate) daemon_id: Option<String>,
    pub(crate) start_command_id: Option<String>,
    pub(crate) cancel_command_id: Option<String>,
}

impl ClarificationRun {
    pub fn assigned(&self) -> bool {
        self.daemon_id.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct ClarificationStartInput<'a> {
    pub requirement_id: &'a str,
    pub start_message_id: &'a str,
    pub expected_state_version: u64,
    pub context: &'a Value,
    pub context_requirement_revision: u64,
    pub repository_ids: &'a [String],
    pub required_capabilities: &'a [String],
}

#[derive(Debug, Clone)]
pub struct ClarificationStartResult {
    pub run: ClarificationRun,
    pub command_id: Option<String>,
    pub reused: bool,
}

#[derive(Debug, Clone)]
pub struct ClarificationCommandResult {
    pub run: ClarificationRun,
    pub command_id: String,
}

#[derive(Debug)]
pub enum ClarificationError {
    Database(sqlx::Error),
    RequirementNotFound,
    MessageNotFound,
    InvalidMessage,
    StateVersionConflict,
    ExistingRunDifferentStart,
    RunNotFound,
    RunNotEligible,
    InvalidContext,
    InvalidSessionState,
    CommandConflict,
}

impl fmt::Display for ClarificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::RequirementNotFound => f.write_str("requirement not found"),
            Self::MessageNotFound => f.write_str("message not found"),
            Self::InvalidMessage => f.write_str("message is not eligible for clarification"),
            Self::StateVersionConflict => f.write_str("requirement state version conflict"),
            Self::ExistingRunDifferentStart => f.write_str("another clarification run is active"),
            Self::RunNotFound => f.write_str("clarification run not found"),
            Self::RunNotEligible => f.write_str("clarification run is not eligible"),
            Self::InvalidContext => f.write_str("invalid clarification context"),
            Self::InvalidSessionState => f.write_str("invalid clarification session state"),
            Self::CommandConflict => f.write_str("clarification command identity conflict"),
        }
    }
}

impl Error for ClarificationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ClarificationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

fn valid_start_context(
    context: &Value,
    requirement_id: &str,
    start_message_id: &str,
    requirement_revision: u64,
    repository_ids: &[String],
    required_capabilities: &[String],
) -> bool {
    let Some(requirement) = context.get("requirement").and_then(Value::as_object) else {
        return false;
    };
    if requirement.get("id").and_then(Value::as_str) != Some(requirement_id)
        || requirement.get("revision").and_then(Value::as_u64) != Some(requirement_revision)
    {
        return false;
    }
    let Some(excerpt) = context
        .get("conversation")
        .and_then(|conversation| conversation.get("excerpt"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    if !excerpt
        .iter()
        .any(|message| message.get("message_id").and_then(Value::as_str) == Some(start_message_id))
    {
        return false;
    }
    let Some(context_repository_ids) = context
        .get("repositories")
        .and_then(Value::as_array)
        .and_then(|repositories| {
            repositories
                .iter()
                .map(|repository| {
                    repository
                        .get("repository_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()
        })
    else {
        return false;
    };
    context_repository_ids.as_slice() == repository_ids
        && !repository_ids.windows(2).any(|pair| pair[0] == pair[1])
        && !required_capabilities.is_empty()
        && required_capabilities
            .iter()
            .all(|capability| !capability.trim().is_empty())
}

// Keep conversion local to this module without exposing persistence internals.
fn requirement_error(error: crate::requirements::RequirementError) -> ClarificationError {
    match error {
        crate::requirements::RequirementError::Database(error) => {
            ClarificationError::Database(error)
        }
        crate::requirements::RequirementError::NotFound => ClarificationError::RequirementNotFound,
        crate::requirements::RequirementError::Conflict => ClarificationError::StateVersionConflict,
        _ => ClarificationError::InvalidContext,
    }
}

#[derive(Debug, FromRow)]
struct RunRow {
    id: String,
    requirement_id: String,
    start_message_id: String,
    daemon_id: Option<String>,
    state: String,
    daemon_connected: bool,
    cancel_requested: bool,
    created_at: String,
    updated_at: String,
    last_activity_at: String,
    start_command_id: Option<String>,
    cancel_command_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct MessageBodyRow {
    body: String,
}

impl RunRow {
    fn into_run(self) -> Result<ClarificationRun, ClarificationError> {
        let phase = match (self.state.as_str(), self.daemon_id.is_some()) {
            ("Completed" | "Failed", _) => ClarificationPhase::Terminal,
            ("Idle", false) => ClarificationPhase::AwaitingAssignment,
            ("Idle" | "Running", true) => ClarificationPhase::Active,
            _ => return Err(ClarificationError::InvalidSessionState),
        };
        let status = match self.state.as_str() {
            "Idle" if self.daemon_id.is_some() && self.daemon_connected => {
                ClarificationStatus::Starting
            }
            "Idle" => ClarificationStatus::Unavailable,
            "Running" if self.daemon_connected => ClarificationStatus::Running,
            "Running" => ClarificationStatus::Unavailable,
            "Completed" if self.daemon_id.is_some() => ClarificationStatus::Completed,
            "Completed" => ClarificationStatus::Unavailable,
            "Failed" | "Retrying" => ClarificationStatus::Unavailable,
            _ => return Err(ClarificationError::InvalidSessionState),
        };
        Ok(ClarificationRun {
            run_id: self.id,
            requirement_id: self.requirement_id,
            start_message_id: self.start_message_id,
            phase,
            status,
            cancel_requested: self.cancel_requested,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_activity_at: self.last_activity_at,
            daemon_id: self.daemon_id,
            start_command_id: self.start_command_id,
            cancel_command_id: self.cancel_command_id,
        })
    }
}

impl AuthStore {
    /// Create or reuse one sequential clarification run. Assignment and the
    /// initial command are committed together; no daemon is also a valid run.
    pub async fn start_clarification<F>(
        &self,
        input: ClarificationStartInput<'_>,
        build_payload: F,
    ) -> Result<ClarificationStartResult, ClarificationError>
    where
        F: FnOnce(&str, &str, &str, u64, &Value) -> Result<String, PersistenceError> + Send,
    {
        let ClarificationStartInput {
            requirement_id,
            start_message_id,
            expected_state_version,
            context,
            context_requirement_revision,
            repository_ids,
            required_capabilities,
        } = input;
        if requirement_id.trim().is_empty()
            || start_message_id.trim().is_empty()
            || expected_state_version == 0
            || context_requirement_revision == 0
            || !valid_start_context(
                context,
                requirement_id,
                start_message_id,
                context_requirement_revision,
                repository_ids,
                required_capabilities,
            )
        {
            return Err(ClarificationError::InvalidContext);
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(format!("clarification-slot:{requirement_id}"))
            .execute(&mut *transaction)
            .await?;
        let row = lock_requirement(&mut transaction, requirement_id)
            .await
            .map_err(requirement_error)?;
        let requirement = row.to_domain().map_err(requirement_error)?;
        let message_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM messages
                 JOIN conversations ON conversations.id = messages.conversation_id
                 WHERE conversations.requirement_id = $1
                   AND messages.id = $2
                   AND messages.kind = 'requester'
             )",
        )
        .bind(requirement_id)
        .bind(start_message_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !message_exists {
            return Err(ClarificationError::MessageNotFound);
        }

        let occupant = sqlx::query_as::<_, RunRow>(
            "SELECT id, requirement_id, start_message_id, daemon_id, state,
                    COALESCE((
                        SELECT connected_at IS NOT NULL
                            AND revoked_at IS NULL
                            AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                        FROM daemon_registrations
                        WHERE daemon_id = execution_sessions.daemon_id
                    ), FALSE) AS daemon_connected,
                    cancel_requested, created_at::text AS created_at,
                    updated_at::text AS updated_at,
                    last_activity_at::text AS last_activity_at,
                    start_command_id, cancel_command_id
             FROM execution_sessions
             WHERE requirement_id = $1
               AND start_message_id IS NOT NULL
               AND state NOT IN ('Completed', 'Failed')
             ORDER BY created_at DESC, id DESC
             LIMIT 1
             FOR UPDATE",
        )
        .bind(requirement_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let mut payload_builder = Some(build_payload);
        if let Some(occupant) = occupant {
            if occupant.start_message_id != start_message_id {
                return Err(ClarificationError::ExistingRunDifferentStart);
            }
            let run = occupant.into_run()?;
            let command_id = if run.daemon_id.is_none() && !run.cancel_requested {
                let Some(daemon_id) =
                    choose_eligible_daemon(&mut transaction, required_capabilities).await?
                else {
                    transaction.commit().await?;
                    return Ok(ClarificationStartResult {
                        command_id: run.start_command_id.clone(),
                        run,
                        reused: true,
                    });
                };
                let command_id = crate::random_hex(16);
                let sequence = next_command_sequence(&mut transaction, &run.run_id).await?;
                let start_context: Value = sqlx::query_scalar(
                    "SELECT start_context FROM execution_sessions WHERE id = $1",
                )
                .bind(&run.run_id)
                .fetch_one(&mut *transaction)
                .await?;
                let payload = payload_builder
                    .take()
                    .ok_or(ClarificationError::InvalidContext)?(
                    &daemon_id,
                    &run.run_id,
                    &command_id,
                    sequence,
                    &start_context,
                )
                .map_err(|_| ClarificationError::InvalidContext)?;
                insert_command(
                    &mut transaction,
                    &command_id,
                    &run.run_id,
                    &daemon_id,
                    sequence,
                    &payload,
                )
                .await?;
                sqlx::query(
                    "UPDATE execution_sessions
                     SET daemon_id = $2, start_command_id = $3,
                         updated_at = CURRENT_TIMESTAMP,
                         last_activity_at = CURRENT_TIMESTAMP
                     WHERE id = $1",
                )
                .bind(&run.run_id)
                .bind(&daemon_id)
                .bind(&command_id)
                .execute(&mut *transaction)
                .await?;
                command_id
            } else {
                run.start_command_id.clone().unwrap_or_default()
            };
            let row = run_row(&mut transaction, &run.run_id).await?;
            transaction.commit().await?;
            return Ok(ClarificationStartResult {
                run: row.into_run()?,
                command_id: (!command_id.is_empty()).then_some(command_id),
                reused: true,
            });
        }

        let terminal_same_start: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM execution_sessions
                 WHERE requirement_id = $1
                   AND start_message_id = $2
                   AND state IN ('Completed', 'Failed')
             )",
        )
        .bind(requirement_id)
        .bind(start_message_id)
        .fetch_one(&mut *transaction)
        .await?;
        if terminal_same_start {
            return Err(ClarificationError::RunNotEligible);
        }

        if requirement.state_version() != expected_state_version {
            return Err(ClarificationError::StateVersionConflict);
        }
        if requirement.revision() != context_requirement_revision {
            return Err(ClarificationError::StateVersionConflict);
        }

        let mut transitioned = requirement.clone();
        if transitioned.status() == RequirementStatus::Draft {
            transitioned
                .begin_discussion()
                .map_err(|_| ClarificationError::StateVersionConflict)?;
            update_requirement(&mut transaction, &transitioned, expected_state_version)
                .await
                .map_err(requirement_error)?;
            sqlx::query(
                "INSERT INTO transition_audit
                    (requirement_id, actor_id, transition, from_status, to_status, state_version)
                 VALUES ($1, $2, 'begin_discussion', 'Draft', 'Discussing', $3)",
            )
            .bind(requirement_id)
            .bind("clarification")
            .bind(
                i64::try_from(transitioned.state_version())
                    .map_err(|_| ClarificationError::InvalidSessionState)?,
            )
            .execute(&mut *transaction)
            .await?;
        }

        for repository_id in repository_ids {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM repositories
                     WHERE id = $1 AND disabled_at IS NULL
                 )",
            )
            .bind(repository_id)
            .fetch_one(&mut *transaction)
            .await?;
            if !exists {
                return Err(ClarificationError::InvalidContext);
            }
        }
        let run_id = crate::random_hex(16);
        let daemon_id = choose_eligible_daemon(&mut transaction, required_capabilities).await?;
        let (start_command_id, command) = if let Some(daemon_id) = daemon_id.as_deref() {
            let command_id = crate::random_hex(16);
            let payload = payload_builder
                .take()
                .ok_or(ClarificationError::InvalidContext)?(
                daemon_id,
                &run_id,
                &command_id,
                1,
                context,
            )
            .map_err(|_| ClarificationError::InvalidContext)?;
            (Some(command_id), Some((daemon_id.to_owned(), payload)))
        } else {
            (None, None)
        };
        sqlx::query(
            "INSERT INTO execution_sessions
                (id, daemon_id, requirement_id, state, start_message_id,
                 start_context, start_command_id, repository_ids,
                 repository_context_initialized)
             VALUES ($1, $2, $3, 'Idle', $4, $5, $6, $7, TRUE)",
        )
        .bind(&run_id)
        .bind(daemon_id.as_deref())
        .bind(requirement_id)
        .bind(start_message_id)
        .bind(context)
        .bind(start_command_id.as_deref())
        .bind(repository_ids)
        .execute(&mut *transaction)
        .await?;
        if let Some((daemon_id, payload)) = command {
            insert_command(
                &mut transaction,
                start_command_id
                    .as_deref()
                    .ok_or(ClarificationError::InvalidSessionState)?,
                &run_id,
                &daemon_id,
                1,
                &payload,
            )
            .await?;
        }
        let run = run_row(&mut transaction, &run_id).await?.into_run()?;
        transaction.commit().await?;
        Ok(ClarificationStartResult {
            command_id: start_command_id,
            run,
            reused: false,
        })
    }

    pub async fn latest_clarification_run(
        &self,
        requirement_id: &str,
    ) -> Result<Option<ClarificationRun>, ClarificationError> {
        let row = sqlx::query_as::<_, RunRow>(
            "SELECT id, requirement_id, start_message_id, daemon_id, state,
                    COALESCE((
                        SELECT connected_at IS NOT NULL
                            AND revoked_at IS NULL
                            AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                        FROM daemon_registrations
                        WHERE daemon_id = execution_sessions.daemon_id
                    ), FALSE) AS daemon_connected,
                    cancel_requested, created_at::text AS created_at,
                    updated_at::text AS updated_at,
                    last_activity_at::text AS last_activity_at,
                    start_command_id, cancel_command_id
             FROM execution_sessions
             WHERE requirement_id = $1 AND start_message_id IS NOT NULL
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(requirement_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(RunRow::into_run).transpose()
    }

    pub async fn clarification_run(
        &self,
        requirement_id: &str,
        run_id: &str,
    ) -> Result<ClarificationRun, ClarificationError> {
        run_row_for_requirement(&self.pool, requirement_id, run_id)
            .await?
            .ok_or(ClarificationError::RunNotFound)?
            .into_run()
    }

    pub async fn dispatch_clarification_message<F>(
        &self,
        requirement_id: &str,
        run_id: &str,
        message_id: &str,
        build_payload: F,
    ) -> Result<ClarificationCommandResult, ClarificationError>
    where
        F: FnOnce(&str, &str, &str, u64, &str, &str) -> Result<String, PersistenceError> + Send,
    {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, RunRow>(
            "SELECT id, requirement_id, start_message_id, daemon_id, state,
                    COALESCE((
                        SELECT connected_at IS NOT NULL
                            AND revoked_at IS NULL
                            AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                        FROM daemon_registrations
                        WHERE daemon_id = execution_sessions.daemon_id
                    ), FALSE) AS daemon_connected,
                    cancel_requested, created_at::text AS created_at,
                    updated_at::text AS updated_at,
                    last_activity_at::text AS last_activity_at,
                    start_command_id, cancel_command_id
             FROM execution_sessions
             WHERE id = $1 AND requirement_id = $2 AND start_message_id IS NOT NULL
             FOR UPDATE",
        )
        .bind(run_id)
        .bind(requirement_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ClarificationError::RunNotFound)?;
        let run = row.into_run()?;
        if !matches!(run.phase, ClarificationPhase::Active)
            || run.cancel_requested
            || run.daemon_id.is_none()
        {
            return Err(ClarificationError::RunNotEligible);
        }
        if run.start_message_id == message_id {
            return Err(ClarificationError::InvalidMessage);
        }
        let message = sqlx::query_as::<_, MessageBodyRow>(
            "SELECT messages.body
             FROM messages
             JOIN conversations ON conversations.id = messages.conversation_id
             WHERE messages.id = $1
               AND conversations.requirement_id = $2
               AND messages.kind = 'requester'",
        )
        .bind(message_id)
        .bind(requirement_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ClarificationError::MessageNotFound)?;
        let content_digest = crate::payload_digest(&message.body);
        let existing: Option<(String, String)> = sqlx::query_as(
            "SELECT command_id, content_digest
             FROM server_message_command_map
             WHERE session_id = $1 AND message_id = $2
             FOR UPDATE",
        )
        .bind(run_id)
        .bind(message_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let command_id = if let Some((command_id, existing_digest)) = existing {
            if existing_digest != content_digest {
                return Err(ClarificationError::CommandConflict);
            }
            command_id
        } else {
            let command_id = crate::random_hex(16);
            let sequence = next_command_sequence(&mut transaction, run_id).await?;
            let daemon_id = run
                .daemon_id
                .as_deref()
                .ok_or(ClarificationError::RunNotEligible)?;
            let payload = build_payload(
                daemon_id,
                run_id,
                &command_id,
                sequence,
                message_id,
                &message.body,
            )
            .map_err(|_| ClarificationError::InvalidContext)?;
            sqlx::query(
                "INSERT INTO server_message_command_map
                    (session_id, message_id, command_id, content_digest)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(run_id)
            .bind(message_id)
            .bind(&command_id)
            .bind(&content_digest)
            .execute(&mut *transaction)
            .await?;
            insert_command(
                &mut transaction,
                &command_id,
                run_id,
                daemon_id,
                sequence,
                &payload,
            )
            .await?;
            command_id
        };
        let run = run_row(&mut transaction, run_id).await?.into_run()?;
        transaction.commit().await?;
        Ok(ClarificationCommandResult { run, command_id })
    }

    pub async fn cancel_clarification<F>(
        &self,
        requirement_id: &str,
        run_id: &str,
        build_payload: F,
    ) -> Result<ClarificationCommandResult, ClarificationError>
    where
        F: FnOnce(&str, &str, &str, u64) -> Result<String, PersistenceError> + Send,
    {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(format!("clarification-slot:{requirement_id}"))
            .execute(&mut *transaction)
            .await?;
        let row = sqlx::query_as::<_, RunRow>(
            "SELECT id, requirement_id, start_message_id, daemon_id, state,
                    COALESCE((
                        SELECT connected_at IS NOT NULL
                            AND revoked_at IS NULL
                            AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                        FROM daemon_registrations
                        WHERE daemon_id = execution_sessions.daemon_id
                    ), FALSE) AS daemon_connected,
                    cancel_requested, created_at::text AS created_at,
                    updated_at::text AS updated_at,
                    last_activity_at::text AS last_activity_at,
                    start_command_id, cancel_command_id
             FROM execution_sessions
             WHERE id = $1 AND requirement_id = $2 AND start_message_id IS NOT NULL
             FOR UPDATE",
        )
        .bind(run_id)
        .bind(requirement_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ClarificationError::RunNotFound)?;
        let current = row.into_run()?;
        if matches!(current.phase, ClarificationPhase::Terminal) {
            if !current.cancel_requested {
                return Err(ClarificationError::RunNotEligible);
            }
            transaction.commit().await?;
            return Ok(ClarificationCommandResult {
                run: current,
                command_id: String::new(),
            });
        }
        sqlx::query(
            "UPDATE execution_sessions
             SET cancel_requested = TRUE,
                 state = CASE WHEN daemon_id IS NULL THEN 'Completed' ELSE state END,
                 updated_at = CURRENT_TIMESTAMP,
                 last_activity_at = CURRENT_TIMESTAMP
             WHERE id = $1",
        )
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        let current = run_row(&mut transaction, run_id).await?.into_run()?;
        let Some(daemon_id) = current.daemon_id.as_deref() else {
            transaction.commit().await?;
            return Ok(ClarificationCommandResult {
                run: current,
                command_id: String::new(),
            });
        };
        let command_id = if let Some(command_id) = current.cancel_command_id.clone() {
            command_id
        } else {
            let command_id = crate::random_hex(16);
            let sequence = next_command_sequence(&mut transaction, run_id).await?;
            let payload = build_payload(daemon_id, run_id, &command_id, sequence)
                .map_err(|_| ClarificationError::InvalidContext)?;
            insert_command(
                &mut transaction,
                &command_id,
                run_id,
                daemon_id,
                sequence,
                &payload,
            )
            .await?;
            sqlx::query(
                "UPDATE execution_sessions
                 SET cancel_command_id = $2,
                     updated_at = CURRENT_TIMESTAMP,
                     last_activity_at = CURRENT_TIMESTAMP
                 WHERE id = $1",
            )
            .bind(run_id)
            .bind(&command_id)
            .execute(&mut *transaction)
            .await?;
            command_id
        };
        let run = run_row(&mut transaction, run_id).await?.into_run()?;
        transaction.commit().await?;
        Ok(ClarificationCommandResult { run, command_id })
    }

    pub async fn clarification_activities(
        &self,
        requirement_id: &str,
        offset: u64,
        limit: u64,
    ) -> Result<(Vec<ClarificationActivity>, Option<u64>), ClarificationError> {
        if limit == 0 || limit > 100 || offset > i64::MAX as u64 {
            return Err(ClarificationError::InvalidContext);
        }
        let rows = sqlx::query_as::<_, ActivityRow>(
            "SELECT activities.id, activities.event_id, activities.session_id,
                    activities.activity, activities.created_at::text AS created_at
             FROM clarification_activities AS activities
             JOIN execution_sessions AS sessions ON sessions.id = activities.session_id
             WHERE sessions.requirement_id = $1
             ORDER BY activities.created_at ASC, activities.id ASC
             OFFSET $2 LIMIT $3",
        )
        .bind(requirement_id)
        .bind(i64::try_from(offset).map_err(|_| ClarificationError::InvalidContext)?)
        .bind(i64::try_from(limit).map_err(|_| ClarificationError::InvalidContext)? + 1)
        .fetch_all(&self.pool)
        .await?;
        let next_offset = (rows.len() > limit as usize).then_some(offset + limit);
        Ok((
            rows.into_iter()
                .take(limit as usize)
                .map(Into::into)
                .collect(),
            next_offset,
        ))
    }

    pub async fn latest_readiness(
        &self,
        requirement_id: &str,
    ) -> Result<Option<ReadinessView>, ClarificationError> {
        let row = sqlx::query_as::<_, ReadinessRow>(
            "SELECT id, event_id, session_id, daemon_event_seq,
                    event_requirement_id, requirement_id, requirement_revision,
                    verdict, blockers, assumptions, repositories_reviewed,
                    outcome, rejection_reason, assessed_at_ms,
                    accepted_state_version, created_at::text AS created_at
             FROM readiness_assessments
             WHERE requirement_id = $1
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(requirement_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(TryInto::try_into).transpose()
    }

    pub async fn project_clarification_event(
        &self,
        event_id: &str,
        session_id: &str,
        daemon_event_seq: u64,
        payload_digest: &str,
        payload: &str,
        event: ClarificationEvent,
    ) -> Result<crate::EventReceipt, ClarificationEventError> {
        if event_id.trim().is_empty()
            || session_id.trim().is_empty()
            || daemon_event_seq == 0
            || payload_digest.trim().is_empty()
            || payload.trim().is_empty()
        {
            return Err(ClarificationEventError::Integrity);
        }
        if let ClarificationEvent::Activity { activity } = &event {
            if activity.trim().is_empty() || activity.chars().count() > MAX_ACTIVITY_CHARS {
                return Err(ClarificationEventError::Projection);
            }
        }
        let payload_value = serde_json::from_str::<Value>(payload)
            .map_err(|_| ClarificationEventError::Integrity)?;
        if crate::canonical_payload_digest(&payload_value) != payload_digest {
            return Err(ClarificationEventError::Integrity);
        }
        let sequence =
            i64::try_from(daemon_event_seq).map_err(|_| ClarificationEventError::Integrity)?;
        let mut transaction = self.pool.begin().await?;
        let session = sqlx::query_as::<_, ProjectionSessionRow>(
            "SELECT event_ack_through_seq, event_ack_sparse,
                    start_message_id, state, daemon_id
             FROM execution_sessions WHERE id = $1 FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ClarificationEventError::NotClarificationSession)?;
        if session.start_message_id.is_none() {
            return Err(ClarificationEventError::NotClarificationSession);
        }
        if let Some(existing) = sqlx::query_as::<_, EventReceiptRow>(
            "SELECT event_id, session_id, daemon_event_seq, payload_digest,
                    outcome, rejection_reason
             FROM server_event_dedupe WHERE event_id = $1 FOR UPDATE",
        )
        .bind(event_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            if existing.session_id != session_id
                || existing.daemon_event_seq != sequence
                || existing.payload_digest != payload_digest
            {
                return Err(ClarificationEventError::Integrity);
            }
            transaction.commit().await?;
            return existing.into_receipt(true);
        }
        let mut expected = session
            .event_ack_through_seq
            .checked_add(1)
            .ok_or(ClarificationEventError::Integrity)?;
        while session.event_ack_sparse.contains(&expected) {
            expected = expected
                .checked_add(1)
                .ok_or(ClarificationEventError::Integrity)?;
        }
        if sequence > expected {
            return Err(ClarificationEventError::Gap {
                expected: u64::try_from(expected).unwrap_or(u64::MAX),
                received: daemon_event_seq,
            });
        }
        if sequence <= session.event_ack_through_seq {
            return Err(ClarificationEventError::Integrity);
        }
        if let Some(existing) = sqlx::query_scalar::<_, String>(
            "SELECT event_id FROM server_event_dedupe
             WHERE session_id = $1 AND daemon_event_seq = $2",
        )
        .bind(session_id)
        .bind(sequence)
        .fetch_optional(&mut *transaction)
        .await?
        {
            return Err(ClarificationEventError::SequenceConflict(existing));
        }
        let rejection_reason = if matches!(session.state.as_str(), "Completed" | "Failed") {
            Some("session_terminal")
        } else if session.daemon_id.is_none() {
            Some("session_unassigned")
        } else if matches!(&event, ClarificationEvent::SessionStarted { .. })
            && session.state != "Idle"
        {
            Some("session_already_started")
        } else {
            None
        };
        let outcome = if rejection_reason.is_some() {
            "rejected"
        } else {
            "accepted"
        };
        if outcome == "rejected" {
            insert_generic_event_dedupe(
                &mut transaction,
                EventDedupe {
                    event_id,
                    session_id,
                    sequence,
                    payload_digest,
                    payload,
                    outcome,
                    rejection_reason,
                },
            )
            .await?;
            advance_event_watermark(&mut transaction, session_id, sequence).await?;
            transaction.commit().await?;
            return Ok(crate::EventReceipt {
                event_id: event_id.to_owned(),
                session_id: session_id.to_owned(),
                daemon_event_seq,
                outcome: crate::EventReceiptOutcome::Rejected,
                rejection_reason: rejection_reason.map(str::to_owned),
                duplicate: false,
            });
        }
        match event {
            ClarificationEvent::SessionStarted { runtime_id } => {
                sqlx::query(
                    "UPDATE execution_sessions
                     SET state = 'Running', runtime_id = $2,
                         started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                         updated_at = CURRENT_TIMESTAMP,
                         last_activity_at = CURRENT_TIMESTAMP
                     WHERE id = $1",
                )
                .bind(session_id)
                .bind(runtime_id)
                .execute(&mut *transaction)
                .await?;
            }
            ClarificationEvent::AgentMessage {
                message_id,
                content,
            } => {
                let conversation_id: String = sqlx::query_scalar(
                    "SELECT conversations.id
                     FROM conversations JOIN execution_sessions
                       ON execution_sessions.requirement_id = conversations.requirement_id
                     WHERE execution_sessions.id = $1",
                )
                .bind(session_id)
                .fetch_one(&mut *transaction)
                .await?;
                sqlx::query(
                    "INSERT INTO messages
                        (id, conversation_id, author_user_id, kind, body, source_event_id)
                     VALUES ($1, $2, NULL, 'agent', $3, $4)",
                )
                .bind(message_id)
                .bind(conversation_id)
                .bind(content)
                .bind(event_id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| {
                    if matches!(&error, sqlx::Error::Database(database)
                        if database.code().as_deref() == Some("23505"))
                    {
                        ClarificationEventError::Integrity
                    } else {
                        ClarificationEventError::Projection
                    }
                })?;
                touch_session(&mut transaction, session_id).await?;
            }
            ClarificationEvent::Activity { activity } => {
                sqlx::query(
                    "INSERT INTO clarification_activities
                        (event_id, session_id, activity) VALUES ($1, $2, $3)",
                )
                .bind(event_id)
                .bind(session_id)
                .bind(activity)
                .execute(&mut *transaction)
                .await?;
                touch_session(&mut transaction, session_id).await?;
            }
            ClarificationEvent::Completed { summary } => {
                sqlx::query(
                    "UPDATE execution_sessions
                     SET state = 'Completed', terminal_summary = $2,
                         updated_at = CURRENT_TIMESTAMP,
                         last_activity_at = CURRENT_TIMESTAMP
                     WHERE id = $1",
                )
                .bind(session_id)
                .bind(summary)
                .execute(&mut *transaction)
                .await?;
            }
            ClarificationEvent::Failed { reason, .. } => {
                sqlx::query(
                    "UPDATE execution_sessions
                     SET state = 'Failed', failure_reason = $2,
                         updated_at = CURRENT_TIMESTAMP,
                         last_activity_at = CURRENT_TIMESTAMP
                     WHERE id = $1",
                )
                .bind(session_id)
                .bind(reason)
                .execute(&mut *transaction)
                .await?;
            }
        }
        insert_generic_event_dedupe(
            &mut transaction,
            EventDedupe {
                event_id,
                session_id,
                sequence,
                payload_digest,
                payload,
                outcome,
                rejection_reason,
            },
        )
        .await?;
        advance_event_watermark(&mut transaction, session_id, sequence).await?;
        transaction.commit().await?;
        Ok(crate::EventReceipt {
            event_id: event_id.to_owned(),
            session_id: session_id.to_owned(),
            daemon_event_seq,
            outcome: crate::EventReceiptOutcome::Accepted,
            rejection_reason: None,
            duplicate: false,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ClarificationEvent {
    SessionStarted { runtime_id: String },
    AgentMessage { message_id: String, content: String },
    Activity { activity: String },
    Completed { summary: String },
    Failed { recoverable: bool, reason: String },
}

#[derive(Debug)]
pub enum ClarificationEventError {
    Integrity,
    Gap { expected: u64, received: u64 },
    SequenceConflict(String),
    NotClarificationSession,
    Projection,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ClarificationEventError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, FromRow)]
struct ProjectionSessionRow {
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
    start_message_id: Option<String>,
    state: String,
    daemon_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct EventReceiptRow {
    event_id: String,
    session_id: String,
    daemon_event_seq: i64,
    payload_digest: String,
    outcome: String,
    rejection_reason: Option<String>,
}

impl EventReceiptRow {
    fn into_receipt(self, duplicate: bool) -> Result<crate::EventReceipt, ClarificationEventError> {
        Ok(crate::EventReceipt {
            event_id: self.event_id,
            session_id: self.session_id,
            daemon_event_seq: u64::try_from(self.daemon_event_seq)
                .map_err(|_| ClarificationEventError::Integrity)?,
            outcome: if self.outcome == "accepted" {
                crate::EventReceiptOutcome::Accepted
            } else {
                crate::EventReceiptOutcome::Rejected
            },
            rejection_reason: self.rejection_reason,
            duplicate,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarificationActivity {
    pub id: i64,
    pub event_id: String,
    pub session_id: String,
    pub activity: String,
    pub created_at: String,
}

#[derive(Debug, FromRow)]
struct ActivityRow {
    id: i64,
    event_id: String,
    session_id: String,
    activity: String,
    created_at: String,
}

impl From<ActivityRow> for ClarificationActivity {
    fn from(row: ActivityRow) -> Self {
        Self {
            id: row.id,
            event_id: row.event_id,
            session_id: row.session_id,
            activity: row.activity,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessView {
    pub id: String,
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub event_requirement_id: String,
    pub requirement_id: Option<String>,
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
}

#[derive(Debug, FromRow)]
struct ReadinessRow {
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
    repositories_reviewed: Value,
    outcome: String,
    rejection_reason: Option<String>,
    assessed_at_ms: i64,
    accepted_state_version: Option<i64>,
    created_at: String,
}

impl TryFrom<ReadinessRow> for ReadinessView {
    type Error = ClarificationError;

    fn try_from(row: ReadinessRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            event_id: row.event_id,
            session_id: row.session_id,
            daemon_event_seq: u64::try_from(row.daemon_event_seq)
                .map_err(|_| ClarificationError::InvalidContext)?,
            event_requirement_id: row.event_requirement_id,
            requirement_id: row.requirement_id,
            requirement_revision: u64::try_from(row.requirement_revision)
                .map_err(|_| ClarificationError::InvalidContext)?,
            verdict: row.verdict,
            blockers: row.blockers,
            assumptions: row.assumptions,
            repositories_reviewed: row.repositories_reviewed,
            outcome: row.outcome,
            rejection_reason: row.rejection_reason,
            assessed_at_ms: row.assessed_at_ms,
            accepted_state_version: row
                .accepted_state_version
                .map(|value| u64::try_from(value).map_err(|_| ClarificationError::InvalidContext))
                .transpose()?,
            created_at: row.created_at,
        })
    }
}

async fn run_row(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<RunRow, ClarificationError> {
    Ok(sqlx::query_as::<_, RunRow>(
        "SELECT id, requirement_id, start_message_id, daemon_id, state,
                COALESCE((
                    SELECT connected_at IS NOT NULL
                        AND revoked_at IS NULL
                        AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                    FROM daemon_registrations
                    WHERE daemon_id = execution_sessions.daemon_id
                ), FALSE) AS daemon_connected,
                cancel_requested, created_at::text AS created_at,
                updated_at::text AS updated_at,
                last_activity_at::text AS last_activity_at,
                start_command_id, cancel_command_id
         FROM execution_sessions WHERE id = $1 FOR UPDATE",
    )
    .bind(run_id)
    .fetch_one(&mut **transaction)
    .await?)
}

async fn run_row_for_requirement(
    pool: &sqlx::PgPool,
    requirement_id: &str,
    run_id: &str,
) -> Result<Option<RunRow>, ClarificationError> {
    Ok(sqlx::query_as::<_, RunRow>(
        "SELECT id, requirement_id, start_message_id, daemon_id, state,
                COALESCE((
                    SELECT connected_at IS NOT NULL
                        AND revoked_at IS NULL
                        AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
                    FROM daemon_registrations
                    WHERE daemon_id = execution_sessions.daemon_id
                ), FALSE) AS daemon_connected,
                cancel_requested, created_at::text AS created_at,
                updated_at::text AS updated_at,
                last_activity_at::text AS last_activity_at,
                start_command_id, cancel_command_id
         FROM execution_sessions
         WHERE id = $1 AND requirement_id = $2 AND start_message_id IS NOT NULL",
    )
    .bind(run_id)
    .bind(requirement_id)
    .fetch_optional(pool)
    .await?)
}

async fn choose_eligible_daemon(
    transaction: &mut Transaction<'_, Postgres>,
    required_capabilities: &[String],
) -> Result<Option<String>, ClarificationError> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT daemon_id, capabilities
         FROM daemon_registrations
         WHERE revoked_at IS NULL
           AND connected_at IS NOT NULL
           AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
           AND protocol_version = '0.1'
         ORDER BY daemon_id ASC FOR UPDATE",
    )
    .fetch_all(&mut **transaction)
    .await?;
    Ok(rows.into_iter().find_map(|(daemon_id, capabilities)| {
        let capabilities = serde_json::from_str::<Vec<String>>(&capabilities).ok()?;
        required_capabilities
            .iter()
            .all(|required| capabilities.iter().any(|capability| capability == required))
            .then_some(daemon_id)
    }))
}

async fn next_command_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<u64, ClarificationError> {
    let next: i64 = sqlx::query_scalar(
        "SELECT GREATEST(
            COALESCE((SELECT MAX(server_command_seq) FROM server_command_outbox WHERE session_id = $1), 0),
            COALESCE((SELECT MAX(server_command_seq) FROM server_command_tombstones WHERE session_id = $1), 0),
            (SELECT command_ack_through_seq FROM execution_sessions WHERE id = $1)
         ) + 1",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    u64::try_from(next).map_err(|_| ClarificationError::InvalidSessionState)
}

async fn insert_command(
    transaction: &mut Transaction<'_, Postgres>,
    command_id: &str,
    session_id: &str,
    daemon_id: &str,
    sequence: u64,
    payload: &str,
) -> Result<(), ClarificationError> {
    let payload_digest = crate::payload_digest(payload);
    let command_identity_digest = crate::command_identity_digest(payload);
    sqlx::query(
        "INSERT INTO server_command_outbox
            (command_id, session_id, daemon_id, server_command_seq, payload,
             payload_digest, command_identity_digest)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command_id)
    .bind(session_id)
    .bind(daemon_id)
    .bind(i64::try_from(sequence).map_err(|_| ClarificationError::InvalidSessionState)?)
    .bind(payload)
    .bind(payload_digest)
    .bind(command_identity_digest)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

struct EventDedupe<'a> {
    event_id: &'a str,
    session_id: &'a str,
    sequence: i64,
    payload_digest: &'a str,
    payload: &'a str,
    outcome: &'a str,
    rejection_reason: Option<&'a str>,
}

async fn insert_generic_event_dedupe(
    transaction: &mut Transaction<'_, Postgres>,
    dedupe: EventDedupe<'_>,
) -> Result<(), ClarificationEventError> {
    sqlx::query(
        "INSERT INTO server_event_dedupe
            (event_id, session_id, daemon_event_seq, payload_digest, payload,
             outcome, rejection_reason)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(dedupe.event_id)
    .bind(dedupe.session_id)
    .bind(dedupe.sequence)
    .bind(dedupe.payload_digest)
    .bind(dedupe.payload)
    .bind(dedupe.outcome)
    .bind(dedupe.rejection_reason)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn touch_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<(), ClarificationEventError> {
    sqlx::query(
        "UPDATE execution_sessions
         SET updated_at = CURRENT_TIMESTAMP, last_activity_at = CURRENT_TIMESTAMP
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn advance_event_watermark(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    sequence: i64,
) -> Result<(), ClarificationEventError> {
    let (mut through, mut sparse): (i64, Vec<i64>) = sqlx::query_as(
        "SELECT event_ack_through_seq, event_ack_sparse
         FROM execution_sessions WHERE id = $1 FOR UPDATE",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    if sequence > through {
        if sequence == through + 1 {
            through = sequence;
            loop {
                let next = through + 1;
                let Some(pos) = sparse.iter().position(|value| *value == next) else {
                    break;
                };
                sparse.remove(pos);
                through = next;
            }
        } else if !sparse.contains(&sequence) {
            sparse.push(sequence);
            sparse.sort_unstable();
        }
    }
    sqlx::query(
        "UPDATE execution_sessions
         SET event_ack_through_seq = $2, event_ack_sparse = $3
         WHERE id = $1",
    )
    .bind(session_id)
    .bind(through)
    .bind(sparse)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_bounds_are_provider_independent() {
        assert_eq!(MAX_CONTEXT_MESSAGES, 50);
        assert_eq!(MAX_CONTEXT_BYTES, 32 * 1024);
        assert_eq!(ClarificationPhase::Active.as_str(), "active");
        assert_eq!(ClarificationStatus::Unavailable.as_str(), "unavailable");
    }

    #[test]
    fn pinned_run_status_reflects_daemon_liveness_without_changing_phase() {
        let offline = test_run_row("Running", Some("daemon-1"), false, false)
            .into_run()
            .expect("offline run");
        assert_eq!(offline.phase, ClarificationPhase::Active);
        assert_eq!(offline.status, ClarificationStatus::Unavailable);

        let running = test_run_row("Running", Some("daemon-1"), true, false)
            .into_run()
            .expect("running run");
        assert_eq!(running.status, ClarificationStatus::Running);

        let cancelled = test_run_row("Completed", Some("daemon-1"), false, true)
            .into_run()
            .expect("completed cancellation");
        assert_eq!(cancelled.phase, ClarificationPhase::Terminal);
        assert_eq!(cancelled.status, ClarificationStatus::Completed);
        assert!(cancelled.cancel_requested);
    }

    fn test_run_row(
        state: &str,
        daemon_id: Option<&str>,
        daemon_connected: bool,
        cancel_requested: bool,
    ) -> RunRow {
        RunRow {
            id: "run-1".into(),
            requirement_id: "requirement-1".into(),
            start_message_id: "message-1".into(),
            daemon_id: daemon_id.map(str::to_owned),
            state: state.into(),
            daemon_connected,
            cancel_requested,
            created_at: "now".into(),
            updated_at: "now".into(),
            last_activity_at: "now".into(),
            start_command_id: None,
            cancel_command_id: None,
        }
    }
}
