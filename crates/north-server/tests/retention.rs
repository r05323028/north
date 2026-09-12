//! PostgreSQL-backed proofs for bounded ephemeral retention.
//!
//! The amnesia test is the acceptance proof: purging every eligible activity
//! row must leave every canonical product and coordination projection
//! identical, and replaying a purged activity event must not recreate it.

use north_domain::readiness::{ReadinessAssessment, ReviewedRepository, Verdict};
use north_domain::requirement::RequirementEdit;
use north_domain::status::RequirementStatus;
use north_persistence::{
    canonical_payload_digest, AuthStore, ClarificationEvent, ClarificationEventError,
    ClarificationStartInput, DaemonSetupClaim, EventReceipt, PersistenceError, PoolOptions,
    RequirementListQuery, RequirementTransition, RetentionConfig,
};
use north_protocol::{
    AgentActivity, Event, EventEnvelope, SessionFailed, SessionStarted, SCHEMA_VERSION,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::{
    env,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

static DATABASE_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

async fn database_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    DATABASE_TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn unique_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

async fn test_pool() -> PgPool {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for retention integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    pool
}

async fn independent_pool() -> PgPool {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for retention integration tests");
    PoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("connect independent test pool")
}

fn valid_small_batch() -> RetentionConfig {
    RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds")
}

/// Test-only cleanup: the production sweep is intentionally global, so
/// mechanics assertions start from an empty ephemeral set.
async fn clear_activities(pool: &PgPool) {
    sqlx::query("DELETE FROM clarification_activities")
        .execute(pool)
        .await
        .expect("clear activity telemetry");
}

async fn insert_activity(pool: &PgPool, session_id: &str, event_id: &str, expires_in_seconds: i64) {
    sqlx::query(
        "INSERT INTO clarification_activities (event_id, session_id, activity, expires_at)
         VALUES ($1, $2, 'sweep activity',
                 CURRENT_TIMESTAMP + ($3::double precision * INTERVAL '1 second'))",
    )
    .bind(event_id)
    .bind(session_id)
    .bind(expires_in_seconds)
    .execute(pool)
    .await
    .expect("insert activity row");
}

async fn activity_ids(pool: &PgPool, session_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event_id
         FROM clarification_activities
         WHERE session_id = $1
         ORDER BY id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .expect("read activity ids")
}

async fn json_snapshot(pool: &PgPool, sql: &str, bind: &str) -> Value {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("durable snapshot query")
}

async fn json_snapshot_array(pool: &PgPool, sql: &str, bind: Vec<String>) -> Value {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("durable snapshot query")
}

async fn project_started(
    store: &AuthStore,
    session_id: &str,
    sequence: u64,
    event_id: &str,
) -> Result<EventReceipt, ClarificationEventError> {
    let runtime_id = "runtime-retention";
    let envelope = EventEnvelope {
        event_id: event_id.to_owned(),
        session_id: session_id.to_owned(),
        daemon_event_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".to_owned(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionStarted(SessionStarted {
            runtime_id: runtime_id.to_owned(),
        }),
    };
    let value = serde_json::to_value(&envelope).expect("serialize started event");
    let payload = serde_json::to_string(&envelope).expect("encode started event");
    store
        .project_clarification_event(
            event_id,
            session_id,
            sequence,
            &canonical_payload_digest(&value),
            &payload,
            ClarificationEvent::SessionStarted {
                runtime_id: runtime_id.to_owned(),
            },
        )
        .await
}

async fn project_activity(
    store: &AuthStore,
    session_id: &str,
    sequence: u64,
    event_id: &str,
    activity: &str,
) -> Result<EventReceipt, ClarificationEventError> {
    let envelope = EventEnvelope {
        event_id: event_id.to_owned(),
        session_id: session_id.to_owned(),
        daemon_event_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".to_owned(),
        schema_version: SCHEMA_VERSION,
        event: Event::AgentActivity(AgentActivity {
            activity: activity.to_owned(),
        }),
    };
    let value = serde_json::to_value(&envelope).expect("serialize activity event");
    let payload = serde_json::to_string(&envelope).expect("encode activity event");
    store
        .project_clarification_event(
            event_id,
            session_id,
            sequence,
            &canonical_payload_digest(&value),
            &payload,
            ClarificationEvent::Activity {
                activity: activity.to_owned(),
            },
        )
        .await
}

async fn project_failure(
    store: &AuthStore,
    session_id: &str,
    sequence: u64,
    event_id: &str,
    reason: &str,
) -> Result<EventReceipt, ClarificationEventError> {
    let envelope = EventEnvelope {
        event_id: event_id.to_owned(),
        session_id: session_id.to_owned(),
        daemon_event_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".to_owned(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionFailed(SessionFailed {
            recoverable: true,
            reason: reason.to_owned(),
        }),
    };
    let value = serde_json::to_value(&envelope).expect("serialize failure event");
    let payload = serde_json::to_string(&envelope).expect("encode failure event");
    store
        .project_clarification_event(
            event_id,
            session_id,
            sequence,
            &canonical_payload_digest(&value),
            &payload,
            ClarificationEvent::Failed {
                recoverable: true,
                reason: reason.to_owned(),
            },
        )
        .await
}

struct AmnesiaFixture {
    store: AuthStore,
    daemon_id: String,
    repository_id: String,
    setup_label: String,
    user_id: String,
    requirement_id: String,
    requirement_revision: u64,
    session_id: String,
}

async fn amnesia_fixture(pool: &PgPool) -> AmnesiaFixture {
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL, last_seen_at = NULL",
    )
    .execute(pool)
    .await
    .expect("clear daemon leases");
    let store = AuthStore::new(pool.clone());
    let email = format!("{}@example.com", unique_id("retention-user"));
    store
        .issue_code(&email, "111111")
        .await
        .expect("issue user code");
    let user = store
        .verify_code(&email, "111111")
        .await
        .expect("verify user code");
    let repository = store
        .create_repository(
            &unique_id("Retention Repository"),
            &format!("https://example.test/{}.git", unique_id("retention")),
            "retention evidence",
        )
        .await
        .expect("create repository");
    let setup_label = unique_id("retention-daemon");
    let setup = store
        .create_daemon_setup_request(&setup_label)
        .await
        .expect("create daemon setup request");
    store
        .approve_daemon_setup_request(&setup.request_token, &user.user.id)
        .await
        .expect("approve daemon setup request");
    let (daemon_id, credential) = match store
        .claim_daemon_setup_request(&setup.request_token)
        .await
        .expect("claim daemon setup request")
    {
        DaemonSetupClaim::Claimed {
            daemon_id,
            credential,
        } => (daemon_id, credential),
        DaemonSetupClaim::Pending => panic!("approved daemon setup did not claim"),
    };
    let capabilities = vec!["agent".to_owned()];
    store
        .connect_daemon(&daemon_id, &credential, "0.1", &capabilities)
        .await
        .expect("connect test daemon");
    let requirement = store
        .create_requirement(
            "Retention amnesia",
            "canonical truth must survive",
            &user.user.id,
        )
        .await
        .expect("create requirement");
    let requirement = store
        .edit_requirement_with_actor(
            &requirement.id,
            requirement.state_version,
            &user.user.id,
            &RequirementEdit {
                title: None,
                description: None,
                summary: Some("bounded retention fixture".to_owned()),
                acceptance_criteria: Some(vec![
                    "Telemetry expires without touching durable state".to_owned()
                ]),
                assumptions: None,
                open_questions: None,
            },
        )
        .await
        .expect("add acceptance criteria");
    let message = store
        .post_requester_message(&requirement.id, &user.user.id, "start clarification")
        .await
        .expect("persist start message");
    let context = json!({
        "requirement": {"id": requirement.id, "revision": requirement.revision},
        "conversation": {"excerpt": [{
            "message_id": message.id,
            "role": "requester",
            "content": message.body
        }]},
        "repositories": [{
            "repository_id": repository.id,
            "name": repository.name,
            "url": repository.url,
            "description": repository.description
        }]
    });
    let repository_ids = vec![repository.id.clone()];
    let started = store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &requirement.id,
                start_message_id: &message.id,
                expected_state_version: requirement.state_version,
                context_requirement_revision: requirement.revision,
                context: &context,
                repository_ids: &repository_ids,
                required_capabilities: &capabilities,
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("start clarification");
    AmnesiaFixture {
        store,
        daemon_id,
        repository_id: repository.id,
        setup_label,
        user_id: user.user.id,
        requirement_id: requirement.id,
        requirement_revision: requirement.revision,
        session_id: started.run.run_id,
    }
}

struct ReviewFixture {
    requirement_id: String,
    session_id: String,
    assessment_id: String,
    state_version: u64,
}

/// Drive a second Requirement to Ready so a real human review decision can be
/// recorded while the first Ready Requirement keeps its review packet.
async fn ready_requirement(
    store: &AuthStore,
    user_id: &str,
    repository_id: &str,
    title: &str,
) -> ReviewFixture {
    let capabilities = vec!["agent".to_owned()];
    let requirement = store
        .create_requirement(title, "review fixture", user_id)
        .await
        .expect("create review requirement");
    let requirement = store
        .edit_requirement_with_actor(
            &requirement.id,
            requirement.state_version,
            user_id,
            &RequirementEdit {
                title: None,
                description: None,
                summary: None,
                acceptance_criteria: Some(vec!["review fixture criterion".to_owned()]),
                assumptions: None,
                open_questions: None,
            },
        )
        .await
        .expect("edit review requirement");
    let message = store
        .post_requester_message(&requirement.id, user_id, "start review run")
        .await
        .expect("review start message");
    let context = json!({
        "requirement": {"id": requirement.id, "revision": requirement.revision},
        "conversation": {"excerpt": [{
            "message_id": message.id,
            "role": "requester",
            "content": message.body
        }]},
        "repositories": [{
            "repository_id": repository_id,
            "name": "review",
            "url": "https://example.test/review.git",
            "description": "review fixture"
        }]
    });
    let repository_ids = vec![repository_id.to_owned()];
    let started = store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &requirement.id,
                start_message_id: &message.id,
                expected_state_version: requirement.state_version,
                context_requirement_revision: requirement.revision,
                context: &context,
                repository_ids: &repository_ids,
                required_capabilities: &capabilities,
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("start review run");
    let session_id = started.run.run_id;
    project_started(store, &session_id, 1, &unique_id("review-started"))
        .await
        .expect("project review started");
    let assessment = ReadinessAssessment {
        requirement_revision: requirement.revision,
        verdict: Verdict::Ready,
        blockers: Vec::new(),
        assumptions: vec!["review assumption".to_owned()],
        repositories_reviewed: vec![ReviewedRepository {
            repository_id: repository_id.to_owned(),
            commit_sha: "abcdef0123456789abcdef0123456789abcdef01".to_owned(),
        }],
        assessed_at_ms: 0,
    };
    store
        .record_readiness_assessment(
            &unique_id("review-assessment"),
            &session_id,
            2,
            &requirement.id,
            &assessment,
        )
        .await
        .expect("record review assessment");
    let ready = store
        .requirement_by_id(&requirement.id)
        .await
        .expect("read review requirement")
        .expect("review requirement exists");
    assert_eq!(ready.status, RequirementStatus::Ready);
    let assessment_id: String = sqlx::query_scalar(
        "SELECT id FROM readiness_assessments
         WHERE requirement_id = $1 AND outcome = 'accepted'
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(&requirement.id)
    .fetch_one(store.pool())
    .await
    .expect("accepted review assessment identity");
    ReviewFixture {
        requirement_id: requirement.id,
        session_id,
        assessment_id,
        state_version: ready.state_version,
    }
}

async fn durable_snapshot(
    pool: &PgPool,
    requirement_ids: &[String],
    session_ids: &[String],
    daemon_id: &str,
    repository_id: &str,
    setup_label: &str,
    user_id: &str,
) -> Value {
    let requirements = json_snapshot_array(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM requirements WHERE id = ANY($1)) AS row_data",
        requirement_ids.to_vec(),
    )
    .await;
    let conversations = json_snapshot_array(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM conversations WHERE requirement_id = ANY($1)) AS row_data",
        requirement_ids.to_vec(),
    )
    .await;
    let messages = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data) ORDER BY row_data.created_at, row_data.id),
                    '[]'::jsonb)
         FROM (
             SELECT messages.*
             FROM messages
             JOIN conversations ON conversations.id = messages.conversation_id
             WHERE conversations.requirement_id = ANY($1)
         ) AS row_data",
        requirement_ids.to_vec(),
    )
    .await;
    let audit = json_snapshot_array(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM transition_audit WHERE requirement_id = ANY($1)) AS row_data",
        requirement_ids.to_vec(),
    )
    .await;
    let evidence = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data) ORDER BY row_data.created_at, row_data.id),
                    '[]'::jsonb)
         FROM (SELECT * FROM readiness_assessments WHERE requirement_id = ANY($1)) AS row_data",
        requirement_ids.to_vec(),
    )
    .await;
    let sessions = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data) ORDER BY row_data.created_at, row_data.id),
                    '[]'::jsonb)
         FROM (SELECT * FROM execution_sessions WHERE id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let attempts = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data)
                              ORDER BY row_data.session_id, row_data.attempt_number),
                    '[]'::jsonb)
         FROM (SELECT * FROM execution_attempts WHERE session_id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let outbox = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data)
                              ORDER BY row_data.session_id, row_data.server_command_seq),
                    '[]'::jsonb)
         FROM (SELECT * FROM server_command_outbox WHERE session_id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let tombstones = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data)
                              ORDER BY row_data.session_id, row_data.server_command_seq),
                    '[]'::jsonb)
         FROM (SELECT * FROM server_command_tombstones WHERE session_id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let dedupe = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data)
                              ORDER BY row_data.session_id, row_data.daemon_event_seq),
                    '[]'::jsonb)
         FROM (SELECT * FROM server_event_dedupe WHERE session_id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let message_map = json_snapshot_array(
        pool,
        "SELECT COALESCE(
                    jsonb_agg(to_jsonb(row_data)
                              ORDER BY row_data.session_id, row_data.message_id),
                    '[]'::jsonb)
         FROM (SELECT * FROM server_message_command_map WHERE session_id = ANY($1)) AS row_data",
        session_ids.to_vec(),
    )
    .await;
    let repositories = json_snapshot(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM repositories WHERE id = $1) AS row_data",
        repository_id,
    )
    .await;
    let daemons = json_snapshot(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.daemon_id), '[]'::jsonb)
         FROM (SELECT * FROM daemon_registrations WHERE daemon_id = $1) AS row_data",
        daemon_id,
    )
    .await;
    let setup_rows = json_snapshot(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM daemon_setup_requests WHERE label = $1) AS row_data",
        setup_label,
    )
    .await;
    let users = json_snapshot(
        pool,
        "SELECT COALESCE(jsonb_agg(to_jsonb(row_data) ORDER BY row_data.id), '[]'::jsonb)
         FROM (SELECT * FROM users WHERE id = $1) AS row_data",
        user_id,
    )
    .await;
    json!({
        "requirements": requirements,
        "conversations": conversations,
        "messages": messages,
        "transition_audit": audit,
        "readiness_assessments": evidence,
        "execution_sessions": sessions,
        "execution_attempts": attempts,
        "server_command_outbox": outbox,
        "server_command_tombstones": tombstones,
        "server_event_dedupe": dedupe,
        "server_message_command_map": message_map,
        "repositories": repositories,
        "daemon_registrations": daemons,
        "daemon_setup_requests": setup_rows,
        "users": users,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn purging_all_ephemeral_telemetry_preserves_canonical_state() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = amnesia_fixture(&pool).await;
    let store = &fixture.store;
    let requirement_id = fixture.requirement_id.as_str();
    let session_id = fixture.session_id.as_str();

    project_started(store, session_id, 1, &unique_id("retention-started"))
        .await
        .expect("project session started");
    let first_activity_event = unique_id("retention-activity");
    let first_activity_text = "indexing sources";
    project_activity(
        store,
        session_id,
        2,
        &first_activity_event,
        first_activity_text,
    )
    .await
    .expect("project first activity");
    project_activity(
        store,
        session_id,
        3,
        &unique_id("retention-activity"),
        "summarising findings",
    )
    .await
    .expect("project second activity");

    let windowed_store = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(60, 60, 500, 20).expect("valid retained window"),
    );
    let windowed_event = unique_id("retention-windowed");
    project_activity(
        &windowed_store,
        session_id,
        4,
        &windowed_event,
        "windowed activity",
    )
    .await
    .expect("project windowed activity");
    let windowed_seconds: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (expires_at - created_at))::double precision
         FROM clarification_activities
         WHERE event_id = $1",
    )
    .bind(&windowed_event)
    .fetch_one(&pool)
    .await
    .expect("read windowed expiry");
    assert!(
        (windowed_seconds - 60.0).abs() < 5.0,
        "activity expiry must use the configured window, got {windowed_seconds} seconds"
    );

    let assessment = ReadinessAssessment {
        requirement_revision: fixture.requirement_revision,
        verdict: Verdict::Ready,
        blockers: Vec::new(),
        assumptions: vec!["North 0.1 scope".to_owned()],
        repositories_reviewed: vec![ReviewedRepository {
            repository_id: fixture.repository_id.clone(),
            commit_sha: "abcdef0123456789abcdef0123456789abcdef01".to_owned(),
        }],
        assessed_at_ms: 0,
    };
    store
        .record_readiness_assessment(
            &unique_id("retention-assessment"),
            session_id,
            5,
            requirement_id,
            &assessment,
        )
        .await
        .expect("record readiness assessment");
    let ready = store
        .requirement_by_id(requirement_id)
        .await
        .expect("read requirement")
        .expect("requirement exists");
    assert_eq!(ready.status, RequirementStatus::Ready);
    project_failure(
        store,
        session_id,
        6,
        &unique_id("retention-failed"),
        "runtime_failure",
    )
    .await
    .expect("project failure fact");

    let review = ready_requirement(
        store,
        &fixture.user_id,
        &fixture.repository_id,
        "Retention review fixture",
    )
    .await;
    store
        .transition_requirement_with_feedback(
            &review.requirement_id,
            review.state_version,
            &fixture.user_id,
            RequirementTransition::Accept,
            None,
            Some(&review.assessment_id),
        )
        .await
        .expect("record human review decision");

    for _ in 0..3 {
        insert_activity(&pool, session_id, &unique_id("retention-extra"), -3_600).await;
    }
    assert_eq!(activity_ids(&pool, session_id).await.len(), 6);
    sqlx::query(
        "UPDATE clarification_activities
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
         WHERE session_id = $1",
    )
    .bind(session_id)
    .execute(&pool)
    .await
    .expect("expire all telemetry");

    let requirement_ids = vec![
        fixture.requirement_id.clone(),
        review.requirement_id.clone(),
    ];
    let session_ids = vec![fixture.session_id.clone(), review.session_id.clone()];

    let requirement_before = store
        .requirement_by_id(requirement_id)
        .await
        .expect("requirement snapshot");
    let list_before = store
        .list_requirements(&RequirementListQuery::default())
        .await
        .expect("board/list snapshot");
    let conversation_before = store
        .conversation_page(requirement_id, 0, 100)
        .await
        .expect("conversation snapshot");
    let packet_before = store
        .review_packet(requirement_id)
        .await
        .expect("review packet snapshot");
    let run_before = store
        .latest_clarification_run(requirement_id)
        .await
        .expect("run snapshot");
    let review_before = store
        .requirement_by_id(&review.requirement_id)
        .await
        .expect("review requirement snapshot");
    let durable_before = durable_snapshot(
        &pool,
        &requirement_ids,
        &session_ids,
        &fixture.daemon_id,
        &fixture.repository_id,
        &fixture.setup_label,
        &fixture.user_id,
    )
    .await;

    let mut purged = 0_u64;
    loop {
        let deleted = store
            .purge_expired_clarification_activities()
            .await
            .expect("retention sweep");
        if deleted == 0 {
            break;
        }
        purged += deleted;
    }
    assert_eq!(purged, 6, "every eligible ephemeral row must be purged");

    assert_eq!(
        requirement_before,
        store
            .requirement_by_id(requirement_id)
            .await
            .expect("requirement snapshot after")
    );
    assert_eq!(
        list_before,
        store
            .list_requirements(&RequirementListQuery::default())
            .await
            .expect("board/list snapshot after")
    );
    assert_eq!(
        conversation_before,
        store
            .conversation_page(requirement_id, 0, 100)
            .await
            .expect("conversation snapshot after")
    );
    assert_eq!(
        packet_before,
        store
            .review_packet(requirement_id)
            .await
            .expect("review packet snapshot after")
    );
    assert_eq!(
        run_before,
        store
            .latest_clarification_run(requirement_id)
            .await
            .expect("run snapshot after")
    );
    assert_eq!(
        review_before,
        store
            .requirement_by_id(&review.requirement_id)
            .await
            .expect("review requirement snapshot after")
    );
    assert_eq!(
        durable_before,
        durable_snapshot(
            &pool,
            &requirement_ids,
            &session_ids,
            &fixture.daemon_id,
            &fixture.repository_id,
            &fixture.setup_label,
            &fixture.user_id
        )
        .await,
        "durable product and coordination state must survive the purge unchanged"
    );
    assert!(
        activity_ids(&pool, session_id).await.is_empty(),
        "only ephemeral activity telemetry may disappear"
    );

    let replay = project_activity(
        store,
        session_id,
        2,
        &first_activity_event,
        first_activity_text,
    )
    .await
    .expect("replay purged activity event");
    assert!(
        replay.duplicate,
        "replaying an acknowledged activity event must return the recorded duplicate outcome"
    );
    assert!(
        activity_ids(&pool, session_id).await.is_empty(),
        "replay must not recreate purged telemetry"
    );
    assert_eq!(
        durable_before,
        durable_snapshot(
            &pool,
            &requirement_ids,
            &session_ids,
            &fixture.daemon_id,
            &fixture.repository_id,
            &fixture.setup_label,
            &fixture.user_id
        )
        .await,
        "replay must not change watermarks or dedupe state"
    );
}

struct SweepFixture {
    session_id: String,
}

async fn sweep_fixture(pool: &PgPool) -> SweepFixture {
    let session_id = unique_id("retention-sweep-session");
    sqlx::query("INSERT INTO execution_sessions (id, state) VALUES ($1, 'Idle')")
        .bind(&session_id)
        .execute(pool)
        .await
        .expect("insert sweep session");
    SweepFixture { session_id }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn sweep_honours_expiry_and_batch_bound() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::with_retention(pool.clone(), valid_small_batch());
    assert_eq!(
        store
            .purge_expired_clarification_activities()
            .await
            .expect("empty sweep"),
        0
    );
    for _ in 0..3 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -3_600).await;
    }
    let future = unique_id("future");
    insert_activity(&pool, &fixture.session_id, &future, 3_600).await;

    assert_eq!(
        store
            .purge_expired_clarification_activities()
            .await
            .expect("first bounded pass"),
        2
    );
    assert_eq!(activity_ids(&pool, &fixture.session_id).await.len(), 2);
    assert_eq!(
        store
            .purge_expired_clarification_activities()
            .await
            .expect("second bounded pass"),
        1
    );
    assert_eq!(
        store
            .purge_expired_clarification_activities()
            .await
            .expect("drained pass"),
        0
    );
    assert_eq!(activity_ids(&pool, &fixture.session_id).await, vec![future]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn sweep_includes_exact_expiry_boundary() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::new(pool.clone());
    let boundary = unique_id("boundary");
    let future = unique_id("future");
    insert_activity(&pool, &fixture.session_id, &boundary, 0).await;
    insert_activity(&pool, &fixture.session_id, &future, 60).await;

    assert_eq!(
        store
            .purge_expired_clarification_activities()
            .await
            .expect("boundary sweep"),
        1
    );
    assert_eq!(activity_ids(&pool, &fixture.session_id).await, vec![future]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn sweeps_are_idempotent_concurrent_and_restart_safe() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    for _ in 0..5 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }
    let first = AuthStore::with_retention(pool.clone(), valid_small_batch());
    let second_pool = independent_pool().await;
    let second = AuthStore::with_retention(second_pool, valid_small_batch());
    let (first_deleted, second_deleted) = tokio::join!(
        first.purge_expired_clarification_activities(),
        second.purge_expired_clarification_activities()
    );
    let first_deleted = first_deleted.expect("first concurrent sweep");
    let second_deleted = second_deleted.expect("second concurrent sweep");
    assert_eq!(
        first_deleted + second_deleted,
        4,
        "two bounded batch-2 sweeps on five rows delete disjoint rows"
    );
    assert_eq!(activity_ids(&pool, &fixture.session_id).await.len(), 1);

    let restarted_pool = independent_pool().await;
    let restarted = AuthStore::new(restarted_pool);
    assert_eq!(
        restarted
            .purge_expired_clarification_activities()
            .await
            .expect("post-restart sweep"),
        1
    );
    assert_eq!(
        restarted
            .purge_expired_clarification_activities()
            .await
            .expect("repeat sweep"),
        0
    );
    assert!(activity_ids(&pool, &fixture.session_id).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn drain_recovers_backlog_beyond_one_batch() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(604_800, 60, 2, 20).expect("valid retention bounds"),
    );
    for _ in 0..12 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }
    let future = unique_id("future");
    insert_activity(&pool, &fixture.session_id, &future, 600).await;

    let drain = store
        .drain_expired_clarification_activities()
        .await
        .expect("drain backlog");
    assert_eq!(
        drain.deleted, 12,
        "one cycle must drain beyond a single batch"
    );
    assert!(
        drain.passes > 1,
        "a backlog larger than one batch needs several passes"
    );
    assert!(!drain.drain_limit_reached, "the backlog was exhausted");
    assert_eq!(
        activity_ids(&pool, &fixture.session_id).await,
        vec![future],
        "non-expired rows must never be deleted"
    );
    let repeat = store
        .drain_expired_clarification_activities()
        .await
        .expect("repeat drain");
    assert_eq!(repeat.deleted, 0);
    assert_eq!(repeat.passes, 1);
    assert!(!repeat.drain_limit_reached);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn drain_recovers_backlog_larger_than_cycle_capacity() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds"),
    );
    for _ in 0..6 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }
    let future = unique_id("future");
    insert_activity(&pool, &fixture.session_id, &future, 600).await;

    let first = store
        .drain_expired_clarification_activities()
        .await
        .expect("first bounded cycle");
    assert_eq!(
        first.deleted, 4,
        "the cycle must stop at its configured capacity"
    );
    assert_eq!(first.passes, 2);
    assert!(
        first.drain_limit_reached,
        "expired rows still remain after the pass budget"
    );
    assert!(store
        .expired_activity_remains()
        .await
        .expect("bounded backlog probe"));

    let second = store
        .drain_expired_clarification_activities()
        .await
        .expect("second cycle");
    assert_eq!(second.deleted, 2, "the next cycle continues the drain");
    assert!(!second.drain_limit_reached);
    assert_eq!(
        activity_ids(&pool, &fixture.session_id).await,
        vec![future],
        "non-expired rows must never be deleted"
    );
    assert!(!store
        .expired_activity_remains()
        .await
        .expect("bounded backlog probe"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn drain_exact_capacity_reports_no_remaining_backlog() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds"),
    );
    for _ in 0..4 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }

    let drain = store
        .drain_expired_clarification_activities()
        .await
        .expect("capacity drain");
    assert_eq!(drain.deleted, 4, "exactly one cycle of capacity");
    assert_eq!(drain.passes, 2);
    assert!(
        !drain.drain_limit_reached,
        "exact capacity must not report a remaining backlog"
    );
    assert!(!store
        .expired_activity_remains()
        .await
        .expect("bounded backlog probe"));
    assert!(activity_ids(&pool, &fixture.session_id).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn drain_small_and_empty_backlogs_stop_early() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    let store = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds"),
    );
    for _ in 0..3 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }

    let drain = store
        .drain_expired_clarification_activities()
        .await
        .expect("small backlog drain");
    assert_eq!(drain.deleted, 3, "a partial final pass stops the cycle");
    assert_eq!(drain.passes, 2);
    assert!(!drain.drain_limit_reached);
    assert!(activity_ids(&pool, &fixture.session_id).await.is_empty());

    let empty = store
        .drain_expired_clarification_activities()
        .await
        .expect("empty drain");
    assert_eq!(empty.deleted, 0);
    assert_eq!(empty.passes, 1);
    assert!(!empty.drain_limit_reached);
    assert!(!store
        .expired_activity_remains()
        .await
        .expect("bounded backlog probe"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn drains_are_concurrent_and_restart_safe() {
    let _guard = database_test_lock().await;
    let pool = test_pool().await;
    clear_activities(&pool).await;
    let fixture = sweep_fixture(&pool).await;
    for _ in 0..4 {
        insert_activity(&pool, &fixture.session_id, &unique_id("expired"), -600).await;
    }
    let first = AuthStore::with_retention(
        pool.clone(),
        RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds"),
    );
    let second_pool = independent_pool().await;
    let second = AuthStore::with_retention(
        second_pool,
        RetentionConfig::new(604_800, 60, 2, 2).expect("valid retention bounds"),
    );
    let (first_drain, second_drain) = tokio::join!(
        first.drain_expired_clarification_activities(),
        second.drain_expired_clarification_activities()
    );
    let first_drain = first_drain.expect("first concurrent drain");
    let second_drain = second_drain.expect("second concurrent drain");
    assert_eq!(
        first_drain.deleted + second_drain.deleted,
        4,
        "each expired row must be deleted exactly once across concurrent drains"
    );
    assert!(activity_ids(&pool, &fixture.session_id).await.is_empty());

    let restarted_pool = independent_pool().await;
    let restarted = AuthStore::new(restarted_pool);
    let restart_drain = restarted
        .drain_expired_clarification_activities()
        .await
        .expect("post-restart drain");
    assert_eq!(restart_drain.deleted, 0);
    assert_eq!(restart_drain.passes, 1);
    assert!(!restart_drain.drain_limit_reached);
    assert!(activity_ids(&pool, &fixture.session_id).await.is_empty());
}
