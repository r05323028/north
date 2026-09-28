#[allow(dead_code)]
mod support;

use north_persistence::{DatabaseConnection, MigrationError};
use sea_orm::{ConnectionTrait, FromQueryResult};
use std::{
    env,
    net::{TcpListener, TcpStream},
    process::Output,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{process::Command, time::timeout};

async fn database_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn schema_name() -> String {
    format!(
        "north_seaorm_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

async fn isolated_schema(database_url: &str) -> (String, DatabaseConnection, DatabaseConnection) {
    let schema = schema_name();
    let admin = support::database(database_url, 1)
        .await
        .expect("connect migration test database");
    support::query(format!(r#"CREATE SCHEMA "{schema}""#))
        .execute(&admin)
        .await
        .expect("create isolated migration schema");

    let database = support::database(database_url, 1)
        .await
        .expect("connect isolated migration database");
    support::query(format!(r#"SET search_path TO "{schema}""#))
        .execute(&database)
        .await
        .expect("set isolated migration schema");
    (schema, admin, database)
}

async fn drop_isolated_schema(
    schema: &str,
    admin: &DatabaseConnection,
    database: DatabaseConnection,
) {
    let _ = database.close().await;
    support::query(format!(r#"DROP SCHEMA "{schema}" CASCADE"#))
        .execute(admin)
        .await
        .expect("drop isolated migration schema");
}

fn schema_database_url(database_url: &str, schema: &str) -> String {
    let separator = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{separator}options=-c%20search_path%3D{schema}")
}

fn database_url_with_credentials(database_url: &str, username: &str, password: &str) -> String {
    let (scheme, authority) = database_url.split_once("://").expect("database URL scheme");
    let host_and_database = authority
        .split_once('@')
        .map_or(authority, |(_, host_and_database)| host_and_database);
    format!("{scheme}://{username}:{password}@{host_and_database}")
}

async fn assert_schema_database_url(database_url: &str, schema: &str) {
    let database = support::database(database_url, 1)
        .await
        .expect("connect schema-scoped migration URL");
    let current_schema: String = support::query_scalar("SELECT current_schema()")
        .fetch_one(&database)
        .await
        .expect("read schema-scoped connection");
    assert_eq!(current_schema, schema);
    let _ = database.close().await;
}

async fn run_migrate_command(database_url: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north-server"));
    command
        .arg("migrate")
        .current_dir(env::temp_dir())
        .env("DATABASE_URL", database_url)
        .env_remove(north_persistence::OTP_HMAC_KEY_ENV)
        .kill_on_drop(true);
    timeout(Duration::from_secs(30), command.output())
        .await
        .expect("migrate command must exit without starting HTTP")
        .expect("run packaged migrate command")
}

fn migration_error_category(stderr: &str) -> &'static str {
    if stderr.contains("manually recreate") {
        "manual-recreate"
    } else if stderr.contains("north-server migrate failed: database connection") {
        "database-connection"
    } else if stderr.contains("north-server migrate failed: configuration") {
        "configuration"
    } else if stderr.contains("north-server migrate failed: database schema inspection failed") {
        "schema-inspection"
    } else if stderr.contains("north-server migrate failed: database migration failed") {
        "migration-failed"
    } else if stderr.is_empty() {
        "empty"
    } else {
        "other"
    }
}

fn assert_command_rejected_without_secrets(output: &Output, database_url: &str) {
    assert!(!output.status.success(), "migration unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("manually recreate"),
        "expected safe rejection (status={:?}, stderr-category={})",
        output.status,
        migration_error_category(&stderr)
    );
    assert!(!stderr.contains(database_url));
    assert!(!stderr.contains("postgres://"));
    assert!(!stderr.to_ascii_lowercase().contains("password"));
}

#[derive(FromQueryResult)]
struct MigrationRow {
    version: String,
}

#[derive(Debug, PartialEq, Eq, FromQueryResult)]
struct CatalogColumn {
    table_name: String,
    column_name: String,
    data_type: String,
    udt_name: String,
    is_nullable: String,
    is_identity: String,
    column_default: Option<String>,
    identity_sequence: Option<String>,
}

#[derive(Debug, PartialEq, Eq, FromQueryResult)]
struct CatalogConstraint {
    table_name: String,
    constraint_name: String,
    constraint_type: String,
    definition: String,
}

#[derive(Debug, PartialEq, Eq, FromQueryResult)]
struct CatalogIndex {
    table_name: String,
    index_name: String,
    definition: String,
}

#[derive(Debug, PartialEq, Eq, FromQueryResult)]
struct CatalogTrigger {
    table_name: String,
    trigger_name: String,
    action_timing: String,
    event_manipulation: String,
    action_statement: String,
}

#[derive(Debug, PartialEq, Eq, FromQueryResult)]
struct CatalogSequence {
    sequence_name: String,
    data_type: String,
    start_value: String,
    minimum_value: String,
    maximum_value: String,
    increment: String,
    cycle_option: String,
}

#[derive(Debug, PartialEq, Eq)]
struct CatalogSnapshot {
    columns: Vec<CatalogColumn>,
    constraints: Vec<CatalogConstraint>,
    indexes: Vec<CatalogIndex>,
    triggers: Vec<CatalogTrigger>,
    sequences: Vec<CatalogSequence>,
}

async fn schema_catalog(database: &DatabaseConnection, schema: &str) -> CatalogSnapshot {
    let mut columns: Vec<CatalogColumn> = support::query_as(
        "SELECT table_name, column_name, data_type, udt_name, is_nullable, is_identity,
                column_default,
                pg_get_serial_sequence(format('%I.%I', table_schema, table_name), column_name)
                    AS identity_sequence
         FROM information_schema.columns
         WHERE table_schema = current_schema() AND table_name <> 'seaql_migrations'
         ORDER BY table_name, column_name",
    )
    .fetch_all(database)
    .await
    .expect("read schema columns");
    for column in &mut columns {
        let generated = column.is_identity == "YES"
            || column
                .column_default
                .as_deref()
                .is_some_and(|value| value.starts_with("nextval("));
        if generated {
            assert!(
                column.identity_sequence.is_some(),
                "generated ID has no sequence"
            );
            column.is_identity = "GENERATED".to_owned();
            column.column_default = Some("<generated>".to_owned());
        }
        if let Some(default) = &mut column.column_default {
            *default = default.replace(schema, "<schema>");
        }
        if let Some(sequence) = &mut column.identity_sequence {
            *sequence = sequence.replace(schema, "<schema>");
        }
    }

    let mut constraints: Vec<CatalogConstraint> = support::query_as(
        "SELECT relation.relname AS table_name, constraint_row.conname AS constraint_name,
                constraint_row.contype::text AS constraint_type,
                pg_get_constraintdef(constraint_row.oid) AS definition
         FROM pg_constraint AS constraint_row
         JOIN pg_class AS relation ON relation.oid = constraint_row.conrelid
         JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
         WHERE namespace.nspname = current_schema()
           AND relation.relname <> 'seaql_migrations'
         ORDER BY relation.relname, constraint_row.conname",
    )
    .fetch_all(database)
    .await
    .expect("read schema constraints");
    for constraint in &mut constraints {
        if constraint.constraint_type == "p" {
            constraint.constraint_name = format!("{}_pkey", constraint.table_name);
        }
        constraint.definition = constraint.definition.replace(schema, "<schema>");
    }
    constraints.sort_by(|left, right| {
        (&left.table_name, &left.constraint_name).cmp(&(&right.table_name, &right.constraint_name))
    });

    let mut indexes: Vec<CatalogIndex> = support::query_as(
        "SELECT tablename AS table_name, indexname AS index_name, indexdef AS definition
         FROM pg_indexes
         WHERE schemaname = current_schema() AND tablename <> 'seaql_migrations'
         ORDER BY tablename, indexname",
    )
    .fetch_all(database)
    .await
    .expect("read schema indexes");
    for index in &mut indexes {
        if index.index_name == format!("pk-{}", index.table_name)
            || index.index_name == format!("{}_pkey", index.table_name)
        {
            index.index_name = format!("{}_pkey", index.table_name);
            let (_, definition) = index
                .definition
                .split_once(" ON ")
                .expect("primary-key index definition has ON clause");
            index.definition = format!("CREATE UNIQUE INDEX {} ON {definition}", index.index_name);
        }
        index.definition = index.definition.replace(schema, "<schema>");
    }
    indexes.sort_by(|left, right| {
        (&left.table_name, &left.index_name).cmp(&(&right.table_name, &right.index_name))
    });

    let mut triggers: Vec<CatalogTrigger> = support::query_as(
        "SELECT event_object_table AS table_name, trigger_name, action_timing,
                event_manipulation, action_statement
         FROM information_schema.triggers
         WHERE trigger_schema = current_schema()
           AND event_object_table <> 'seaql_migrations'
         ORDER BY event_object_table, trigger_name, event_manipulation",
    )
    .fetch_all(database)
    .await
    .expect("read schema triggers");
    for trigger in &mut triggers {
        trigger.action_statement = trigger.action_statement.replace(schema, "<schema>");
    }

    let sequences: Vec<CatalogSequence> = support::query_as(
        "SELECT relation.relname AS sequence_name,
                format_type(seq.seqtypid, NULL) AS data_type,
                seq.seqstart::text AS start_value, seq.seqmin::text AS minimum_value,
                seq.seqmax::text AS maximum_value, seq.seqincrement::text AS increment,
                CASE WHEN seq.seqcycle THEN 'YES' ELSE 'NO' END AS cycle_option
         FROM pg_sequence AS seq
         JOIN pg_class AS relation ON relation.oid = seq.seqrelid
         JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
         WHERE namespace.nspname = current_schema()
         ORDER BY relation.relname",
    )
    .fetch_all(database)
    .await
    .expect("read schema sequences");

    CatalogSnapshot {
        columns,
        constraints,
        indexes,
        triggers,
        sequences,
    }
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn compiled_migration_matches_former_postgres_catalog() {
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for catalog parity tests");
    let (legacy_schema, legacy_admin, legacy_database) = isolated_schema(&database_url).await;
    let (seaorm_schema, seaorm_admin, seaorm_database) = isolated_schema(&database_url).await;

    legacy_database
        .execute_unprepared(include_str!("fixtures/legacy_baseline.sql"))
        .await
        .expect("apply former SQL baseline to empty schema");
    north_persistence::run_migrations(&seaorm_database)
        .await
        .expect("apply compiled SeaORM baseline");

    let legacy_catalog = schema_catalog(&legacy_database, &legacy_schema).await;
    let seaorm_catalog = schema_catalog(&seaorm_database, &seaorm_schema).await;
    assert_eq!(
        seaorm_catalog, legacy_catalog,
        "compiled schema differs from former baseline"
    );

    let legacy_id: i64 = support::query_scalar(
        "INSERT INTO verification_codes (email, code_hash, expires_at)\n         VALUES ('catalog@example.com', decode('00', 'hex'), CURRENT_TIMESTAMP)\n         RETURNING id",
    )
    .fetch_one(&legacy_database)
    .await
    .expect("legacy baseline generates verification-code ID");
    let seaorm_id: i64 = support::query_scalar(
        "INSERT INTO verification_codes (email, code_hash, expires_at)\n         VALUES ('catalog@example.com', decode('00', 'hex'), CURRENT_TIMESTAMP)\n         RETURNING id",
    )
    .fetch_one(&seaorm_database)
    .await
    .expect("SeaORM baseline generates verification-code ID");
    assert_eq!(seaorm_id, legacy_id);

    let legacy_seed: i64 =
        support::query_scalar("SELECT COUNT(*) FROM instance_settings WHERE id = 1")
            .fetch_one(&legacy_database)
            .await
            .expect("read former baseline seed");
    let seaorm_seed: i64 =
        support::query_scalar("SELECT COUNT(*) FROM instance_settings WHERE id = 1")
            .fetch_one(&seaorm_database)
            .await
            .expect("read SeaORM baseline seed");
    assert_eq!(seaorm_seed, 1);
    assert_eq!(seaorm_seed, legacy_seed);

    drop_isolated_schema(&legacy_schema, &legacy_admin, legacy_database).await;
    drop_isolated_schema(&seaorm_schema, &seaorm_admin, seaorm_database).await;
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn fresh_install_repeat_up_and_readiness_preserve_data() {
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for fresh-install tests");
    let (schema, admin, database) = isolated_schema(&database_url).await;

    assert!(matches!(
        north_persistence::verify_migrations(&database).await,
        Err(MigrationError::MigrationsPending)
    ));
    let migration_table: bool =
        support::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(&database)
            .await
            .expect("check startup did not create migration table");
    assert!(
        !migration_table,
        "read-only verification must not issue DDL"
    );

    north_persistence::run_migrations(&database)
        .await
        .expect("apply SeaORM baseline");
    north_persistence::verify_migrations(&database)
        .await
        .expect("verify current migration head");

    let applied: Vec<MigrationRow> =
        support::query_as("SELECT version FROM seaql_migrations ORDER BY version")
            .fetch_all(&database)
            .await
            .expect("read SeaORM migration head");
    assert_eq!(
        applied
            .iter()
            .map(|row| row.version.as_str())
            .collect::<Vec<_>>(),
        ["m0001_initial_schema"]
    );

    for table in [
        "users",
        "verification_codes",
        "sessions",
        "instance_settings",
        "requirements",
        "transition_audit",
        "conversations",
        "messages",
        "readiness_assessments",
        "daemon_setup_requests",
        "daemon_registrations",
        "execution_sessions",
        "server_command_outbox",
        "server_command_tombstones",
        "server_message_command_map",
        "server_event_dedupe",
        "repositories",
        "execution_attempts",
        "clarification_activities",
    ] {
        let exists: bool = support::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(&database)
            .await
            .unwrap_or_else(|error| panic!("check fresh-install table {table}: {error}"));
        assert!(exists, "fresh install missing table {schema}.{table}");
    }

    let invalid_code_insert = support::query(
        "INSERT INTO verification_codes (email, code_hash, expires_at, failed_attempts)
         VALUES ($1, $2, CURRENT_TIMESTAMP, -1)",
    )
    .bind(format!("invalid-{schema}@example.com"))
    .bind(vec![0_u8])
    .execute(&database)
    .await;
    assert!(
        invalid_code_insert.is_err(),
        "negative verification attempts must fail"
    );

    let user_id = format!("migration-{}", schema);
    let email = format!("{user_id}@example.com");
    support::query("INSERT INTO users (id, email) VALUES ($1, $2)")
        .bind(&user_id)
        .bind(&email)
        .execute(&database)
        .await
        .expect("insert preservation sentinel");
    north_persistence::run_migrations(&database)
        .await
        .expect("repeat migration is idempotent");
    let preserved_email: String = support::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(&user_id)
        .fetch_one(&database)
        .await
        .expect("read preservation sentinel");
    assert_eq!(preserved_email, email);

    drop_isolated_schema(&schema, &admin, database).await;
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn sqlx_history_is_rejected_without_schema_or_data_changes() {
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for migration tests");
    let (schema, admin, database) = isolated_schema(&database_url).await;
    let command_url = schema_database_url(&database_url, &schema);
    assert_schema_database_url(&command_url, &schema).await;
    support::query(
        "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, success BOOLEAN NOT NULL)",
    )
    .execute(&database)
    .await
    .expect("create legacy SQLx ledger fixture");
    support::query("INSERT INTO _sqlx_migrations (version, success) VALUES (1, TRUE)")
        .execute(&database)
        .await
        .expect("insert legacy SQLx ledger row");

    let error = north_persistence::run_migrations(&database)
        .await
        .expect_err("reject SQLx migration history");
    assert_eq!(error, MigrationError::UnsupportedSqlxHistory);
    assert!(error.to_string().contains("manually recreate"));
    assert_command_rejected_without_secrets(&run_migrate_command(&command_url).await, &command_url);

    let ledger_rows: i64 = support::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&database)
        .await
        .expect("read unchanged legacy ledger");
    let seaorm_ledger: bool =
        support::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(&database)
            .await
            .expect("check migration preflight did not create SeaORM ledger");
    assert_eq!(ledger_rows, 1);
    assert!(!seaorm_ledger);

    drop_isolated_schema(&schema, &admin, database).await;
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn partial_north_schema_is_rejected_without_mutation() {
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for migration tests");
    let (schema, admin, database) = isolated_schema(&database_url).await;
    let command_url = schema_database_url(&database_url, &schema);
    assert_schema_database_url(&command_url, &schema).await;
    support::query("CREATE TABLE users (id TEXT PRIMARY KEY, email TEXT NOT NULL)")
        .execute(&database)
        .await
        .expect("create partial schema fixture");
    support::query("INSERT INTO users (id, email) VALUES ('kept', 'kept@example.com')")
        .execute(&database)
        .await
        .expect("insert partial schema sentinel");

    let error = north_persistence::run_migrations(&database)
        .await
        .expect_err("reject partial North schema");
    assert_eq!(error, MigrationError::ExistingSchemaWithoutSeaOrmHistory);
    assert_command_rejected_without_secrets(&run_migrate_command(&command_url).await, &command_url);
    let email: String = support::query_scalar("SELECT email FROM users WHERE id = 'kept'")
        .fetch_one(&database)
        .await
        .expect("read unchanged partial schema sentinel");
    let seaorm_ledger: bool =
        support::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(&database)
            .await
            .expect("check preflight did not create migration table");
    assert_eq!(email, "kept@example.com");
    assert!(!seaorm_ledger);

    drop_isolated_schema(&schema, &admin, database).await;
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn migrate_command_runs_fresh_and_current_without_otp_or_http() {
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for command tests");
    let (schema, admin, database) = isolated_schema(&database_url).await;
    let command_url = schema_database_url(&database_url, &schema);
    assert_schema_database_url(&command_url, &schema).await;

    let first = run_migrate_command(&command_url).await;
    assert!(first.status.success(), "migrate command failed");
    assert!(String::from_utf8_lossy(&first.stdout).contains("database schema is current"));
    north_persistence::verify_migrations(&database)
        .await
        .expect("verify fresh command migration");

    let second = run_migrate_command(&command_url).await;
    assert!(second.status.success(), "repeat migrate command failed");
    assert!(String::from_utf8_lossy(&second.stdout).contains("database schema is current"));
    drop_isolated_schema(&schema, &admin, database).await;
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn server_startup_rejects_pending_schema_without_ddl_or_listener() {
    const TEST_OTP_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    let _guard = database_test_lock().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for startup tests");
    let (schema, admin, database) = isolated_schema(&database_url).await;
    let role = format!(
        "north_readonly_{}",
        schema.trim_start_matches("north_seaorm_")
    );
    let password = format!(
        "p{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let database_name: String = support::query_scalar("SELECT current_database()")
        .fetch_one(&admin)
        .await
        .expect("read test database name");
    let database_name = database_name.replace('"', "\"\"");
    support::query(format!(
        r#"CREATE ROLE "{role}" LOGIN PASSWORD '{password}'"#
    ))
    .execute(&admin)
    .await
    .expect("create read-only startup role");
    support::query(format!(
        r#"GRANT CONNECT ON DATABASE "{database_name}" TO "{role}""#
    ))
    .execute(&admin)
    .await
    .expect("grant database connection");
    support::query(format!(r#"GRANT USAGE ON SCHEMA "{schema}" TO "{role}""#))
        .execute(&admin)
        .await
        .expect("grant schema usage");
    let role_database_url = database_url_with_credentials(&database_url, &role, &password);
    let command_url = schema_database_url(&role_database_url, &schema);
    assert_schema_database_url(&command_url, &schema).await;
    let readonly_database = support::database(&command_url, 1)
        .await
        .expect("connect read-only startup role");
    let can_create: bool = support::query_scalar(
        "SELECT has_schema_privilege(current_user, current_schema(), 'CREATE')",
    )
    .fetch_one(&readonly_database)
    .await
    .expect("check startup role DDL privilege");
    assert!(
        !can_create,
        "startup test role unexpectedly has schema DDL privilege"
    );
    let _ = readonly_database.close().await;
    assert!(matches!(
        north_persistence::verify_migrations(&database).await,
        Err(MigrationError::MigrationsPending)
    ));

    let address = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve server port");
        listener.local_addr().expect("read server port")
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_north-server"));
    command
        .env("DATABASE_URL", &command_url)
        .env("NORTH_BIND_ADDR", address.to_string())
        .env(north_persistence::OTP_HMAC_KEY_ENV, TEST_OTP_KEY)
        .kill_on_drop(true);
    let process = timeout(Duration::from_secs(10), command.output()).await;
    let timed_out = process.is_err();
    let output = process.ok().and_then(Result::ok);
    let status_success = output
        .as_ref()
        .is_some_and(|output| output.status.success());
    let stderr = output
        .as_ref()
        .map(|output| String::from_utf8_lossy(&output.stderr).into_owned())
        .unwrap_or_default();
    let listener_accepts = TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok();
    let migration_table: bool =
        support::query_scalar("SELECT to_regclass('seaql_migrations') IS NOT NULL")
            .fetch_one(&database)
            .await
            .expect("check startup did not create migration table");
    let table_count: i64 = support::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables\n         WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
    )
    .fetch_one(&database)
    .await
    .expect("check startup did not create application tables");

    drop_isolated_schema(&schema, &admin, database).await;
    support::query(format!(
        r#"REVOKE CONNECT ON DATABASE "{database_name}" FROM "{role}""#
    ))
    .execute(&admin)
    .await
    .expect("revoke test database connection");
    support::query(format!(r#"DROP ROLE "{role}""#))
        .execute(&admin)
        .await
        .expect("drop read-only startup role");

    assert!(!timed_out, "server stayed running with migrations pending");
    assert!(output.is_some(), "server startup command failed to execute");
    assert!(!status_success, "server started with migrations pending");
    assert!(
        !listener_accepts,
        "server listened before schema was current"
    );
    assert!(
        stderr.contains("migration"),
        "expected migration-specific failure"
    );
    assert!(!stderr.contains(&command_url));
    assert!(!stderr.contains(&password));
    assert!(!stderr.contains(TEST_OTP_KEY));
    assert!(!stderr.to_ascii_lowercase().contains("password"));
    assert!(!migration_table);
    assert_eq!(table_count, 0);
}

#[test]
fn migration_errors_do_not_expose_connection_details() {
    for error in [
        MigrationError::UnsupportedSqlxHistory,
        MigrationError::ExistingSchemaWithoutSeaOrmHistory,
        MigrationError::DatabaseInspectionFailed,
        MigrationError::MigrationExecutionFailed,
    ] {
        assert!(!error.to_string().contains("postgres://"));
        assert!(!error.to_string().contains("password"));
    }
}
