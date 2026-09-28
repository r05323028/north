use sea_orm::DatabaseConnection;
use sea_orm_migration::{prelude::*, MigratorTrait};
use std::{error::Error, fmt};

#[path = "m0001_initial_schema.rs"]
mod m0001_initial_schema;

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

/// Apply pending migrations after rejecting unsupported pre-release databases.
pub async fn apply(database: &DatabaseConnection) -> Result<(), MigrationError> {
    preflight(database).await?;
    Migrator::up(database, None)
        .await
        .map_err(|_| MigrationError::MigrationExecutionFailed)?;
    verify(database).await
}

/// Verify migration state and required tables without issuing DDL.
pub async fn verify(database: &DatabaseConnection) -> Result<(), MigrationError> {
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
             'server_event_dedupe', 'clarification_activities', 'execution_attempts'
         ]::text[]) AS expected(name)
         WHERE to_regclass(expected.name) IS NOT NULL",
    )
    .fetch_one(database)
    .await
    .map_err(|_| MigrationError::DatabaseInspectionFailed)?;
    if application_tables != 19 {
        return Err(MigrationError::IncompleteSchema);
    }
    Ok(())
}

async fn preflight(database: &DatabaseConnection) -> Result<(), MigrationError> {
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
             'server_event_dedupe', 'clarification_activities', 'execution_attempts'
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

async fn reject_sqlx_history(database: &DatabaseConnection) -> Result<(), MigrationError> {
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
