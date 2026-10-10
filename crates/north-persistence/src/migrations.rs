use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use sea_orm_migration::{prelude::*, MigratorTrait};
use std::{error::Error, fmt};

#[path = "m0001_initial_schema.rs"]
mod m0001_initial_schema;
#[path = "m0002_requirement_board_positions.rs"]
mod m0002_requirement_board_positions;
#[path = "m0003_user_preferences.rs"]
mod m0003_user_preferences;

const MIGRATION_LOCK_KEY: &str = "north:seaorm:schema-migrations";

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m0001_initial_schema::Migration),
            Box::new(m0002_requirement_board_positions::Migration),
            Box::new(m0003_user_preferences::Migration),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationError {
    UnsupportedSqlxHistory,
    ExistingSchemaWithoutSeaOrmHistory,
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
                "existing North tables have no SeaORM migration history; manually recreate database before migrating",
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
    let has_migration_table: bool =
        crate::query::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(database)
            .await
            .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if !has_migration_table {
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

    let application_tables: i64 = crate::query::query_scalar(
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
         WHERE to_regclass(expected.name) IS NOT NULL",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if application_tables != 21 {
        return Err(MigrationError::IncompleteSchema);
    }
    Ok(())
}

async fn preflight<C: ConnectionTrait>(database: &C) -> Result<(), MigrationError> {
    reject_sqlx_history(database).await?;
    let has_migration_table: bool =
        crate::query::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(database)
            .await
            .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if has_migration_table {
        return Ok(());
    }

    let north_tables: i64 = crate::query::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM unnest(ARRAY[
             'users', 'verification_codes', 'sessions', 'instance_settings',
             'requirements', 'transition_audit', 'conversations', 'messages',
             'readiness_assessments', 'daemon_setup_requests', 'daemon_registrations',
             'execution_sessions', 'server_command_outbox', 'repositories',
             'server_command_tombstones', 'server_message_command_map',
             'server_event_dedupe', 'clarification_activities', 'execution_attempts',
             'requirement_board_positions', 'user_requirement_preferences'
         ]::text[]) AS existing(name)
         WHERE to_regclass(existing.name) IS NOT NULL",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if north_tables > 0 {
        return Err(MigrationError::ExistingSchemaWithoutSeaOrmHistory);
    }
    Ok(())
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
