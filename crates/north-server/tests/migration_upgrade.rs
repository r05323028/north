use sqlx::{Connection, PgConnection};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn legacy_command_payload(command_id: &str, session_id: &str, sequence: i64) -> String {
    serde_json::json!({
        "frame": "command",
        "payload": {
            "command_id": command_id,
            "session_id": session_id,
            "server_command_seq": sequence,
            "sent_at": "2026-01-01T00:00:00Z",
            "schema_version": 1,
            "command": {"type": "session.resume", "payload": {}},
        },
    })
    .to_string()
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
async fn historical_main_head_upgrades_to_current_head() {
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

    // Test-only helper: execute the exact migration SQL used by the production
    // sqlx migrator. This is a real main-head upgrade: repositories did not
    // exist while historical migrations 0001–0012 were applied.
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

    let legacy_gap_requirement = unique("legacy-gap-requirement");
    let legacy_gap_first = unique("legacy-gap-first");
    let legacy_gap_third = unique("legacy-gap-third");
    insert_requirement(
        &mut connection,
        &legacy_gap_requirement,
        &user_id,
        "Discussing",
        1,
    )
    .await;
    insert_assessment(
        &mut connection,
        &legacy_gap_first,
        &legacy_gap_requirement,
        1,
        1,
        "needs_clarification",
        "rejected",
        Some("first legacy gap fixture"),
    )
    .await;
    sqlx::query(
        "INSERT INTO readiness_assessments (
             id, event_id, session_id, daemon_event_seq, event_requirement_id,
             requirement_id, requirement_revision, verdict, blockers, assumptions,
             repositories_reviewed, outcome, rejection_reason, assessed_at_ms
         ) VALUES ($1, $2, $3, 3, $4, $4, 1, 'needs_clarification',
                   ARRAY[]::TEXT[], ARRAY[]::TEXT[], '[]'::jsonb,
                   'rejected', 'third legacy gap fixture', 0)",
    )
    .bind(&legacy_gap_third)
    .bind(format!("event-{legacy_gap_third}"))
    .bind(format!("session-{legacy_gap_first}"))
    .bind(&legacy_gap_requirement)
    .execute(&mut connection)
    .await
    .expect("insert sparse legacy assessment");

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

    let legacy_gap_session = format!("session-{legacy_gap_first}");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&legacy_gap_session)
        .bind(&legacy_gap_requirement)
        .execute(&mut connection)
        .await
        .expect("insert sparse legacy session");

    let legacy_assessment_session = format!("session-{assessment_a}");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&legacy_assessment_session)
        .bind(&requirement_a)
        .execute(&mut connection)
        .await
        .expect("insert legacy assessment session");

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
    apply_migration(
        &mut connection,
        "0012_transition_audit_provenance",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0012_transition_audit_provenance.sql"
        )),
    )
    .await;

    let legacy_daemon = unique("legacy-delivery-daemon");
    let legacy_session = unique("legacy-delivery-session");
    let legacy_command = unique("legacy-delivery-command");
    let legacy_unack_command = unique("legacy-unack-command");
    let legacy_payload = legacy_command_payload(&legacy_command, &legacy_session, 1);
    let legacy_unack_payload = legacy_command_payload(&legacy_unack_command, &legacy_session, 2);
    sqlx::query(
        "INSERT INTO daemon_registrations
            (daemon_id, credential_hash, label, created_by, protocol_version, capabilities)
         VALUES ($1, $2, $3, $4, '0.1', '[]')",
    )
    .bind(&legacy_daemon)
    .bind(legacy_daemon.as_bytes())
    .bind(&legacy_daemon)
    .bind(&user_id)
    .execute(&mut connection)
    .await
    .expect("insert legacy daemon");
    sqlx::query("INSERT INTO execution_sessions (id, daemon_id) VALUES ($1, $2)")
        .bind(&legacy_session)
        .bind(&legacy_daemon)
        .execute(&mut connection)
        .await
        .expect("insert legacy session");
    sqlx::query(
        "INSERT INTO server_command_outbox
            (command_id, session_id, daemon_id, server_command_seq, payload, acknowledged_at)
         VALUES ($1, $2, $3, 1, $4, CURRENT_TIMESTAMP)",
    )
    .bind(&legacy_command)
    .bind(&legacy_session)
    .bind(&legacy_daemon)
    .bind(legacy_payload)
    .execute(&mut connection)
    .await
    .expect("insert acknowledged legacy outbox");
    sqlx::query(
        "INSERT INTO server_command_outbox
            (command_id, session_id, daemon_id, server_command_seq, payload)
         VALUES ($1, $2, $3, 2, $4)",
    )
    .bind(&legacy_unack_command)
    .bind(&legacy_session)
    .bind(&legacy_daemon)
    .bind(legacy_unack_payload)
    .execute(&mut connection)
    .await
    .expect("insert unacknowledged legacy outbox");

    let repositories_before_upgrade: Option<String> =
        sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(format!("{schema}.repositories"))
            .fetch_one(&mut connection)
            .await
            .expect("inspect historical repository table");
    assert_eq!(repositories_before_upgrade, None);

    let citation_repository_id = "00000000-0000-4000-8000-000000000001";
    let citation_requirement = unique("legacy-citation-requirement");
    let citation_session = unique("legacy-citation-session");
    let citation_assessment = unique("legacy-citation-assessment");
    insert_requirement(
        &mut connection,
        &citation_requirement,
        &user_id,
        "Discussing",
        13,
    )
    .await;
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&citation_session)
        .bind(&citation_requirement)
        .execute(&mut connection)
        .await
        .expect("insert legacy citation session");
    sqlx::query(
        "INSERT INTO readiness_assessments (
             id, event_id, session_id, daemon_event_seq, event_requirement_id,
             requirement_id, requirement_revision, verdict, blockers, assumptions,
             repositories_reviewed, outcome, rejection_reason, assessed_at_ms,
             accepted_state_version, generation_unknown
         ) VALUES ($1, $2, $3, 1, $4, $4, 13, 'needs_clarification',
                   ARRAY[]::TEXT[], ARRAY[]::TEXT[], $5::jsonb,
                   'rejected', 'legacy citation fixture', 0, NULL, FALSE)",
    )
    .bind(&citation_assessment)
    .bind(format!("event-{citation_assessment}"))
    .bind(&citation_session)
    .bind(&citation_requirement)
    .bind(
        serde_json::json!([{
            "repository_id": citation_repository_id,
            "commit_sha": "abcdef0123456789abcdef0123456789abcdef01",
        }])
        .to_string(),
    )
    .execute(&mut connection)
    .await
    .expect("insert legacy repository citation");

    apply_migration(
        &mut connection,
        "0013_repositories",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0013_repositories.sql"
        )),
    )
    .await;

    sqlx::query(
        "INSERT INTO repositories (id, name, name_normalized, url, description)
         VALUES ($1, 'Legacy Repository', 'legacy repository',
                 'https://example.test/legacy.git', 'retained history')",
    )
    .bind(citation_repository_id)
    .execute(&mut connection)
    .await
    .expect("insert upgraded repository identity");

    apply_migration(
        &mut connection,
        "0014_protocol_delivery",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0014_protocol_delivery.sql"
        )),
    )
    .await;

    let repository_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'repositories'
           AND column_name IN (
               'id', 'name', 'name_normalized', 'url', 'description',
               'created_at', 'updated_at', 'disabled_at'
           )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("inspect repository columns");
    assert_eq!(repository_columns, 8);
    let repository_unique: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM pg_constraint
             WHERE conrelid = 'repositories'::regclass AND contype = 'u'
         )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("inspect repository uniqueness");
    assert!(repository_unique, "repository names must remain unique");
    let repository_trigger: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM pg_trigger
             WHERE tgrelid = 'repositories'::regclass
               AND tgname = 'repositories_url_immutable'
               AND NOT tgisinternal
         )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("inspect repository trigger");
    assert!(repository_trigger, "repository URL trigger must exist");
    let url_error = sqlx::query(
        "UPDATE repositories SET url = 'https://example.test/retargeted.git' WHERE id = $1",
    )
    .bind(citation_repository_id)
    .execute(&mut connection)
    .await
    .expect_err("repository URL must remain immutable");
    assert!(
        url_error.to_string().contains("repository identity, URL")
            || url_error.to_string().contains("immutable"),
        "unexpected repository URL mutation error: {url_error}"
    );

    let (legacy_title, legacy_revision): (String, i64) =
        sqlx::query_as("SELECT title, revision FROM requirements WHERE id = $1")
            .bind(&requirement_a)
            .fetch_one(&mut connection)
            .await
            .expect("read retained legacy requirement");
    assert_eq!(legacy_title, format!("Legacy {requirement_a}"));
    assert_eq!(legacy_revision, 7);
    let legacy_user_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
            .bind(&user_id)
            .fetch_one(&mut connection)
            .await
            .expect("read retained legacy user");
    assert!(legacy_user_exists);
    let (legacy_label, legacy_protocol): (String, String) = sqlx::query_as(
        "SELECT label, protocol_version
         FROM daemon_registrations WHERE daemon_id = $1",
    )
    .bind(&legacy_daemon)
    .fetch_one(&mut connection)
    .await
    .expect("read retained daemon registration");
    assert_eq!(legacy_label, legacy_daemon);
    assert_eq!(legacy_protocol, "0.1");
    let (legacy_session_requirement, legacy_session_state): (Option<String>, String) =
        sqlx::query_as("SELECT requirement_id, state FROM execution_sessions WHERE id = $1")
            .bind(&legacy_session)
            .fetch_one(&mut connection)
            .await
            .expect("read retained execution session");
    assert_eq!(legacy_session_requirement, None);
    assert_eq!(legacy_session_state, "Idle");
    let legacy_outbox_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&legacy_session)
            .fetch_one(&mut connection)
            .await
            .expect("count retained legacy outbox");
    assert_eq!(legacy_outbox_count, 2);
    let delivery_table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM pg_class
         WHERE relnamespace = current_schema()::regnamespace
           AND relname IN (
               'server_command_tombstones',
               'server_message_command_map',
               'server_event_dedupe'
           )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("inspect delivery tables");
    assert_eq!(delivery_table_count, 3);
    let delivery_column_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND (
               (table_name = 'execution_sessions' AND column_name IN (
                   'command_ack_through_seq', 'event_ack_through_seq',
                   'event_ack_sparse', 'repository_ids',
                   'repository_context_initialized'
               ))
               OR (table_name = 'server_command_outbox' AND column_name IN (
                   'payload_digest', 'command_identity_digest'
               ))
           )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("inspect delivery columns");
    assert_eq!(delivery_column_count, 7);
    let legacy_event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments")
        .fetch_one(&mut connection)
        .await
        .expect("count retained readiness assessments");
    let legacy_event_tombstone_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_event_dedupe WHERE legacy_identity")
            .fetch_one(&mut connection)
            .await
            .expect("count backfilled readiness identities");
    let eligible_legacy_event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM readiness_assessments AS assessment
         WHERE EXISTS (
             SELECT 1 FROM execution_sessions AS session
             WHERE session.id = assessment.session_id
         )",
    )
    .fetch_one(&mut connection)
    .await
    .expect("count readiness identities with session provenance");
    assert_eq!(legacy_event_tombstone_count, eligible_legacy_event_count);
    assert!(
        legacy_event_tombstone_count < legacy_event_count,
        "orphaned readiness rows must not become event identities"
    );
    let (citation_repository, citation_sha): (String, String) = sqlx::query_as(
        "SELECT repositories_reviewed->0->>'repository_id',
                repositories_reviewed->0->>'commit_sha'
         FROM readiness_assessments WHERE id = $1",
    )
    .bind(&citation_assessment)
    .fetch_one(&mut connection)
    .await
    .expect("read retained repository citation");
    assert_eq!(citation_repository, citation_repository_id);
    assert_eq!(citation_sha, "abcdef0123456789abcdef0123456789abcdef01");

    let legacy_watermark: i64 =
        sqlx::query_scalar("SELECT command_ack_through_seq FROM execution_sessions WHERE id = $1")
            .bind(&legacy_session)
            .fetch_one(&mut connection)
            .await
            .expect("read legacy command watermark");
    assert_eq!(legacy_watermark, 1);
    let (payload_digest, identity_digest): (String, String) = sqlx::query_as(
        "SELECT payload_digest, command_identity_digest
         FROM server_command_outbox WHERE command_id = $1",
    )
    .bind(&legacy_command)
    .fetch_one(&mut connection)
    .await
    .expect("read legacy command digests");
    let expected_digest: String =
        sqlx::query_scalar("SELECT MD5(payload) FROM server_command_outbox WHERE command_id = $1")
            .bind(&legacy_command)
            .fetch_one(&mut connection)
            .await
            .expect("compute legacy payload digest");
    assert_eq!(payload_digest, expected_digest);
    assert_eq!(identity_digest, expected_digest);
    let legacy_event_watermark: i64 = sqlx::query_scalar(
        "SELECT event_ack_through_seq
         FROM execution_sessions WHERE id = $1",
    )
    .bind(&legacy_assessment_session)
    .fetch_one(&mut connection)
    .await
    .expect("read legacy event watermark");
    assert_eq!(legacy_event_watermark, 1);
    let (legacy_gap_watermark, legacy_gap_sparse): (i64, Vec<i64>) = sqlx::query_as(
        "SELECT event_ack_through_seq, event_ack_sparse
         FROM execution_sessions WHERE id = $1",
    )
    .bind(&legacy_gap_session)
    .fetch_one(&mut connection)
    .await
    .expect("read sparse legacy watermark");
    assert_eq!(legacy_gap_watermark, 1);
    assert_eq!(legacy_gap_sparse, vec![3]);

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
