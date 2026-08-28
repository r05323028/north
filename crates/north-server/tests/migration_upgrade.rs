use sqlx::{Connection, PgConnection};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

async fn apply_migration(connection: &mut PgConnection, name: &str, sql: &str) {
    sqlx::raw_sql(sql)
        .execute(&mut *connection)
        .await
        .unwrap_or_else(|error| panic!("apply migration {name}: {error}"));
}

async fn insert_user(connection: &mut PgConnection, user_id: &str) {
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, 'Requester')")
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .execute(&mut *connection)
        .await
        .expect("insert legacy user");
}

async fn insert_requirement(
    connection: &mut PgConnection,
    requirement_id: &str,
    user_id: &str,
    status: &str,
    revision: i64,
) {
    sqlx::query(
        "INSERT INTO requirements (id, title, description, status, revision, created_by)
         VALUES ($1, $2, 'legacy description', $3, $4, $5)",
    )
    .bind(requirement_id)
    .bind(format!("Legacy {requirement_id}"))
    .bind(status)
    .bind(revision)
    .bind(user_id)
    .execute(&mut *connection)
    .await
    .expect("insert legacy requirement");
}

// Fixture arguments mirror legacy readiness columns and keep each case explicit.
#[allow(clippy::too_many_arguments)]
async fn insert_assessment(
    connection: &mut PgConnection,
    assessment_id: &str,
    requirement_id: &str,
    requirement_revision: i64,
    sequence: i64,
    verdict: &str,
    outcome: &str,
    rejection_reason: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO readiness_assessments (
             id, event_id, session_id, daemon_event_seq, event_requirement_id,
             requirement_id, requirement_revision, verdict, blockers, assumptions,
             repositories_reviewed, outcome, rejection_reason, assessed_at_ms
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8,
             ARRAY[]::TEXT[], ARRAY[]::TEXT[], '[]'::jsonb,
             $9, $10, 0
         )",
    )
    .bind(assessment_id)
    .bind(format!("event-{assessment_id}"))
    .bind(format!("session-{assessment_id}"))
    .bind(sequence)
    .bind(requirement_id)
    .bind(requirement_id)
    .bind(requirement_revision)
    .bind(verdict)
    .bind(outcome)
    .bind(rejection_reason)
    .execute(&mut *connection)
    .await
    .expect("insert legacy readiness assessment");
}

async fn generation(connection: &mut PgConnection, assessment_id: &str) -> (Option<i64>, bool) {
    sqlx::query_as(
        "SELECT accepted_state_version, generation_unknown
         FROM readiness_assessments
         WHERE id = $1",
    )
    .bind(assessment_id)
    .fetch_one(&mut *connection)
    .await
    .expect("read readiness generation")
}

async fn current_evidence_count(connection: &mut PgConnection, requirement_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM readiness_assessments AS assessment
         INNER JOIN requirements AS requirement
             ON requirement.id = assessment.requirement_id
         WHERE assessment.requirement_id = $1
           AND assessment.outcome = 'accepted'
           AND assessment.accepted_state_version = requirement.state_version
           AND assessment.generation_unknown = FALSE",
    )
    .bind(requirement_id)
    .fetch_one(&mut *connection)
    .await
    .expect("count current evidence")
}

fn assert_immutable(error: sqlx::Error, operation: &str) {
    assert!(
        error
            .to_string()
            .contains("readiness assessments are immutable"),
        "{operation} should hit immutable trigger, got: {error}"
    );
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn legacy_readiness_upgrade_backfills_generations_conservatively() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for migration upgrade tests");
    let mut connection = PgConnection::connect(&database_url)
        .await
        .expect("connect test database");
    let schema = unique("legacy-readiness-upgrade");
    sqlx::query(&format!(r####"CREATE SCHEMA "{schema}""####))
        .execute(&mut connection)
        .await
        .expect("create isolated migration schema");
    sqlx::query(&format!(r####"SET search_path TO "{schema}", public"####))
        .execute(&mut connection)
        .await
        .expect("set isolated migration search path");

    // Test-only helper: execute the same migration SQL used by the production
    // sqlx migrator. Legacy readiness fixtures are inserted after 0005, before
    // runtime and state/readiness upgrade migrations run.
    for (name, sql) in [
        (
            "0001_email_auth",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0001_email_auth.sql"
            )),
        ),
        (
            "0002_role_model",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0002_role_model.sql"
            )),
        ),
        (
            "0003_requirements",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0003_requirements.sql"
            )),
        ),
        (
            "0004_conversations",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0004_conversations.sql"
            )),
        ),
        (
            "0005_readiness_assessments",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0005_readiness_assessments.sql"
            )),
        ),
    ] {
        apply_migration(&mut connection, name, sql).await;
    }

    let user_id = unique("legacy-user");
    insert_user(&mut connection, &user_id).await;

    // Case A: one accepted assessment exactly identifies the current Ready row.
    let requirement_a = unique("requirement-a");
    let assessment_a = unique("assessment-a");
    insert_requirement(&mut connection, &requirement_a, &user_id, "Ready", 7).await;
    insert_assessment(
        &mut connection,
        &assessment_a,
        &requirement_a,
        7,
        1,
        "ready",
        "accepted",
        None,
    )
    .await;

    // Case B: multiple accepted assessments make generation identity ambiguous.
    let requirement_b = unique("requirement-b");
    let assessment_b1 = unique("assessment-b1");
    let assessment_b2 = unique("assessment-b2");
    insert_requirement(&mut connection, &requirement_b, &user_id, "Ready", 8).await;
    insert_assessment(
        &mut connection,
        &assessment_b1,
        &requirement_b,
        8,
        2,
        "ready",
        "accepted",
        None,
    )
    .await;
    insert_assessment(
        &mut connection,
        &assessment_b2,
        &requirement_b,
        8,
        3,
        "ready",
        "accepted",
        None,
    )
    .await;

    // Case C: accepted evidence does not represent the current Ready state.
    let requirement_c_terminal = unique("requirement-c-terminal");
    let assessment_c_terminal = unique("assessment-c-terminal");
    insert_requirement(
        &mut connection,
        &requirement_c_terminal,
        &user_id,
        "Accepted",
        9,
    )
    .await;
    insert_assessment(
        &mut connection,
        &assessment_c_terminal,
        &requirement_c_terminal,
        9,
        4,
        "ready",
        "accepted",
        None,
    )
    .await;

    let requirement_c_revision = unique("requirement-c-revision");
    let assessment_c_revision = unique("assessment-c-revision");
    insert_requirement(
        &mut connection,
        &requirement_c_revision,
        &user_id,
        "Ready",
        11,
    )
    .await;
    insert_assessment(
        &mut connection,
        &assessment_c_revision,
        &requirement_c_revision,
        10,
        5,
        "ready",
        "accepted",
        None,
    )
    .await;

    // Case D: rejected evidence remains a known non-generation row.
    let requirement_d = unique("requirement-d");
    let assessment_d = unique("assessment-d");
    insert_requirement(&mut connection, &requirement_d, &user_id, "Discussing", 12).await;
    insert_assessment(
        &mut connection,
        &assessment_d,
        &requirement_d,
        12,
        6,
        "needs_clarification",
        "rejected",
        Some("needs more detail"),
    )
    .await;

    for (name, sql) in [
        (
            "0007_daemon_runtime",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0007_daemon_runtime.sql"
            )),
        ),
        (
            "0008_runtime_hardening",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0008_runtime_hardening.sql"
            )),
        ),
        (
            "0009_execution_session_requirement",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/0009_execution_session_requirement.sql"
            )),
        ),
    ] {
        apply_migration(&mut connection, name, sql).await;
    }

    apply_migration(
        &mut connection,
        "0010_requirement_state_version",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0010_requirement_state_version.sql"
        )),
    )
    .await;
    apply_migration(
        &mut connection,
        "0011_readiness_generation_immutable_fk",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0011_readiness_generation_immutable_fk.sql"
        )),
    )
    .await;

    // 0011's controlled backfill chooses only unambiguous current Ready evidence.
    assert_eq!(
        generation(&mut connection, &assessment_a).await,
        (Some(1), false)
    );
    assert_eq!(
        generation(&mut connection, &assessment_b1).await,
        (None, true)
    );
    assert_eq!(
        generation(&mut connection, &assessment_b2).await,
        (None, true)
    );
    assert_eq!(
        generation(&mut connection, &assessment_c_terminal).await,
        (None, true)
    );
    assert_eq!(
        generation(&mut connection, &assessment_c_revision).await,
        (None, true)
    );
    assert_eq!(
        generation(&mut connection, &assessment_d).await,
        (None, false)
    );

    assert_eq!(
        current_evidence_count(&mut connection, &requirement_a).await,
        1
    );
    assert_eq!(
        current_evidence_count(&mut connection, &requirement_b).await,
        0
    );
    assert_eq!(
        current_evidence_count(&mut connection, &requirement_c_terminal).await,
        0
    );
    assert_eq!(
        current_evidence_count(&mut connection, &requirement_c_revision).await,
        0
    );

    let trigger_enabled: bool = sqlx::query_scalar(
        "SELECT tgenabled = 'O'
         FROM pg_trigger
         WHERE tgrelid = 'readiness_assessments'::regclass
           AND tgname = 'readiness_assessments_immutable'",
    )
    .fetch_one(&mut connection)
    .await
    .expect("read immutable trigger state");
    assert!(
        trigger_enabled,
        "0011 must re-enable immutable trigger after backfill"
    );

    // Direct mutation remains forbidden after migration-time backfill completes.
    let update_error = sqlx::query(
        "UPDATE readiness_assessments
         SET assessed_at_ms = assessed_at_ms + 1
         WHERE id = $1",
    )
    .bind(&assessment_a)
    .execute(&mut connection)
    .await
    .expect_err("direct readiness UPDATE must remain forbidden");
    assert_immutable(update_error, "UPDATE");

    let delete_error = sqlx::query("DELETE FROM readiness_assessments WHERE id = $1")
        .bind(&assessment_a)
        .execute(&mut connection)
        .await
        .expect_err("direct readiness DELETE must remain forbidden");
    assert_immutable(delete_error, "DELETE");

    // The upgraded evidence FK is restrictive; Requirement deletion cannot detach evidence.
    let requirement_delete_error = sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_a)
        .execute(&mut connection)
        .await
        .expect_err("Requirement with readiness evidence must not be deleted");
    assert!(
        requirement_delete_error
            .to_string()
            .contains("violates foreign key constraint"),
        "expected restrictive readiness FK, got: {requirement_delete_error}"
    );

    sqlx::query(&format!(r####"DROP SCHEMA "{schema}" CASCADE"####))
        .execute(&mut connection)
        .await
        .expect("drop isolated migration schema");
}
