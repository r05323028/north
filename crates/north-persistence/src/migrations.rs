use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use sea_orm_migration::{prelude::*, MigratorTrait};
use std::{error::Error, fmt};

#[path = "initial_schema.rs"]
mod m0001_initial_schema;

const MIGRATION_LOCK_KEY: &str = "north:seaorm:schema-migrations";

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m0001_initial_schema::Migration)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationError {
    UnsupportedSqlxHistory,
    ExistingSchemaWithoutSeaOrmHistory,
    UnsupportedSeaOrmHistory,
    DatabaseInspectionFailed,
    MigrationExecutionFailed,
    MigrationsPending,
    IncompleteSchema,
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSqlxHistory => f.write_str(
                "unsupported pre-release SQLx migration history; manually recreate database before migrating",
            ),
            Self::ExistingSchemaWithoutSeaOrmHistory => f.write_str(
                "existing schema objects have no SeaORM migration history; manually recreate database before migrating",
            ),
            Self::UnsupportedSeaOrmHistory => f.write_str(
                "unsupported or unverifiable SeaORM migration history; back up and manually recreate database before migrating",
            ),
            Self::DatabaseInspectionFailed => f.write_str("database schema inspection failed"),
            Self::MigrationExecutionFailed => f.write_str("database migration failed"),
            Self::MigrationsPending => f.write_str(
                "database schema is not current; run `north-server migrate` before starting the server",
            ),
            Self::IncompleteSchema => f.write_str("database schema is incomplete"),
        }
    }
}

impl Error for MigrationError {}

/// Apply pending migrations under one database-scoped lock and transaction.
pub async fn apply(database: &DatabaseConnection) -> Result<(), MigrationError> {
    let transaction = database
        .begin()
        .await
        .map_err(|_| MigrationError::MigrationExecutionFailed)?;
    crate::query::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&transaction)
        .await
        .map_err(|_| MigrationError::MigrationExecutionFailed)?;

    preflight(&transaction).await?;
    Migrator::up(&transaction, None)
        .await
        .map_err(|_| MigrationError::MigrationExecutionFailed)?;
    verify(&transaction).await?;
    transaction
        .commit()
        .await
        .map_err(|_| MigrationError::MigrationExecutionFailed)
}

/// Verify migration state and required tables without issuing DDL.
pub async fn verify<C: ConnectionTrait>(database: &C) -> Result<(), MigrationError> {
    reject_sqlx_history(database).await?;
    let (has_migration_table, has_applied_migrations) = validate_seaorm_history(database).await?;
    if !has_migration_table || !has_applied_migrations {
        return Err(MigrationError::MigrationsPending);
    }

    let pending = Migrator::get_pending_migrations_read_only(database)
        .await
        .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    let applied = Migrator::get_applied_migrations_read_only(database)
        .await
        .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if !pending.is_empty() || applied.len() != Migrator::migrations().len() {
        return Err(MigrationError::MigrationsPending);
    }

    if required_baseline_table_count(database).await? != REQUIRED_BASELINE_TABLE_COUNT {
        return Err(MigrationError::IncompleteSchema);
    }
    Ok(())
}

async fn preflight<C: ConnectionTrait>(database: &C) -> Result<(), MigrationError> {
    reject_sqlx_history(database).await?;
    let (has_migration_table, has_applied_migrations) = validate_seaorm_history(database).await?;
    if has_migration_table {
        if has_applied_migrations
            && required_baseline_table_count(database).await? != REQUIRED_BASELINE_TABLE_COUNT
        {
            return Err(MigrationError::UnsupportedSeaOrmHistory);
        }
        return Ok(());
    }
    if has_existing_schema_objects(database).await? {
        return Err(MigrationError::ExistingSchemaWithoutSeaOrmHistory);
    }
    Ok(())
}

#[derive(sea_orm::FromQueryResult)]
struct AppliedMigrationVersion {
    version: String,
}

const REQUIRED_BASELINE_TABLE_COUNT: i64 = 21;

async fn validate_seaorm_history<C: ConnectionTrait>(
    database: &C,
) -> Result<(bool, bool), MigrationError> {
    let has_migration_table: bool = crate::query::query_scalar(
        "SELECT to_regclass(format('%I.seaql_migrations', current_schema())) IS NOT NULL",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if !has_migration_table {
        return Ok((false, false));
    }

    let registered_versions: Vec<String> = Migrator::migrations()
        .iter()
        .map(|migration| migration.name().to_owned())
        .collect();
    let applied_versions: Vec<String> =
        crate::query::query_as::<AppliedMigrationVersion>("SELECT version FROM seaql_migrations")
            .fetch_all(database)
            .await
            .map_err(|_| MigrationError::DatabaseInspectionFailed)?
            .into_iter()
            .map(|row| row.version)
            .collect();
    if applied_versions.len() > registered_versions.len()
        || registered_versions
            .iter()
            .take(applied_versions.len())
            .any(|version| !applied_versions.contains(version))
    {
        return Err(MigrationError::UnsupportedSeaOrmHistory);
    }

    let schema_marker: Option<String> = crate::query::query_scalar(
        "SELECT obj_description(
            to_regclass(format('%I.seaql_migrations', current_schema())),
            'pg_class'
        )",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if applied_versions.is_empty() {
        if schema_marker.is_some() || has_existing_schema_objects(database).await? {
            return Err(MigrationError::UnsupportedSeaOrmHistory);
        }
        return Ok((true, false));
    }
    if schema_marker.as_deref() != Some(m0001_initial_schema::BASELINE_MARKER) {
        return Err(MigrationError::UnsupportedSeaOrmHistory);
    }
    Ok((true, true))
}

async fn required_baseline_table_count<C: ConnectionTrait>(
    database: &C,
) -> Result<i64, MigrationError> {
    crate::query::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM unnest(ARRAY[
             'users', 'verification_codes', 'sessions', 'instance_settings',
             'requirements', 'transition_audit', 'conversations', 'messages',
             'readiness_assessments', 'daemon_setup_requests', 'daemon_registrations',
             'execution_sessions', 'server_command_outbox', 'repositories',
             'server_command_tombstones', 'server_message_command_map',
             'server_event_dedupe', 'clarification_activities', 'execution_attempts',
             'requirement_board_positions', 'user_requirement_preferences'
         ]::text[]) AS expected(name)
         WHERE to_regclass(format('%I.%I', current_schema(), expected.name)) IS NOT NULL",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)
}

async fn has_existing_schema_objects<C: ConnectionTrait>(
    database: &C,
) -> Result<bool, MigrationError> {
    crate::query::query_scalar(
        "SELECT EXISTS (
            SELECT 1
            FROM pg_class AS relation
            JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
            WHERE namespace.nspname = current_schema()
              AND relation.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
              AND relation.relname <> 'seaql_migrations'
        )",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)
}

async fn reject_sqlx_history<C: ConnectionTrait>(database: &C) -> Result<(), MigrationError> {
    let has_sqlx_history: bool =
        crate::query::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
            .fetch_one(database)
            .await
            .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if has_sqlx_history {
        return Err(MigrationError::UnsupportedSqlxHistory);
    }
    Ok(())
}
