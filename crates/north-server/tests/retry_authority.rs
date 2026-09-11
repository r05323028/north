use futures_util::{SinkExt, StreamExt};
use north_persistence::{
    canonical_payload_digest, AuthStore, ClarificationError, ClarificationEvent,
    ClarificationPhase, ClarificationStartInput, ClarificationStatus, DaemonSetupClaim,
    PersistenceError, PoolOptions,
};
use north_protocol::{
    encode_daemon_frame, Command, CommandEnvelope, DaemonFrame, Event, EventEnvelope, ServerFrame,
    SessionFailed, SessionResume, SCHEMA_VERSION,
};
use serde_json::json;
use sqlx::PgPool;
use std::{
    env,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};

static DATABASE_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

async fn database_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    DATABASE_TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

async fn test_pool() -> PgPool {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for retry integration tests");
    let pool = PoolOptions::new()
        .max_connections(16)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    pool
}

async fn independent_worker_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")?;
    Ok(PoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?)
}

fn unique_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

struct RetryFixture {
    store: AuthStore,
    daemon_id: String,
    credential: String,
    requirement_id: String,
    session_id: String,
    start_command_id: String,
}

async fn retry_fixture(pool: &PgPool, prefix: &str) -> RetryFixture {
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL, last_seen_at = NULL",
    )
    .execute(pool)
    .await
    .expect("clear daemon leases");
    let store = AuthStore::new(pool.clone());
    let email = format!("{}@example.com", unique_id(prefix));
    store
        .issue_code(&email, "111111")
        .await
        .expect("issue user code");
    let user = store
        .verify_code(&email, "111111")
        .await
        .expect("verify user code");
    let setup = store
        .create_daemon_setup_request(&unique_id("retry-daemon"))
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
        .create_requirement("Retry isolation", "Must remain unchanged", &user.user.id)
        .await
        .expect("create requirement");
    let message = store
        .post_requester_message(&requirement.id, &user.user.id, "retry this clarification")
        .await
        .expect("persist start message");
    let context = json!({
        "requirement": {"id": requirement.id, "revision": requirement.revision},
        "conversation": {"excerpt": [{
            "message_id": message.id,
            "role": "requester",
            "content": message.body
        }]},
        "repositories": []
    });
    let started = store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &requirement.id,
                start_message_id: &message.id,
                expected_state_version: requirement.state_version,
                context_requirement_revision: requirement.revision,
                context: &context,
                repository_ids: &[],
                required_capabilities: &capabilities,
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("start clarification");
    let start_command_id = started.command_id.expect("assigned start command");
    RetryFixture {
        store,
        daemon_id,
        credential,
        requirement_id: requirement.id,
        session_id: started.run.run_id,
        start_command_id,
    }
}

async fn acknowledge_start(pool: &PgPool, fixture: &RetryFixture) {
    let updated = sqlx::query(
        "UPDATE server_command_outbox
         SET acknowledged_at = CURRENT_TIMESTAMP
         WHERE command_id = $1",
    )
    .bind(&fixture.start_command_id)
    .execute(pool)
    .await
    .expect("acknowledge start command");
    assert_eq!(updated.rows_affected(), 1);
    sqlx::query(
        "UPDATE execution_sessions
         SET command_ack_through_seq = 1
         WHERE id = $1",
    )
    .bind(&fixture.session_id)
    .execute(pool)
    .await
    .expect("advance command watermark");
}

async fn set_daemon_offline(pool: &PgPool, daemon_id: &str) {
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL, last_seen_at = NULL
         WHERE daemon_id = $1",
    )
    .bind(daemon_id)
    .execute(pool)
    .await
    .expect("disconnect daemon");
}

async fn set_retry_due(pool: &PgPool, session_id: &str) {
    sqlx::query(
        "UPDATE execution_sessions
         SET next_retry_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(pool)
    .await
    .expect("make retry due");
}

async fn project_failure(
    fixture: &RetryFixture,
    event_id: &str,
    daemon_event_seq: u64,
    recoverable: bool,
    reason: &str,
) -> Result<north_persistence::EventReceipt, north_persistence::ClarificationEventError> {
    let envelope = EventEnvelope {
        event_id: event_id.to_owned(),
        session_id: fixture.session_id.clone(),
        daemon_event_seq,
        sent_at: "2026-01-01T00:00:00Z".to_owned(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionFailed(SessionFailed {
            recoverable,
            reason: reason.to_owned(),
        }),
    };
    let value = serde_json::to_value(&envelope).expect("serialize failure event");
    let payload = serde_json::to_string(&envelope).expect("encode failure event");
    fixture
        .store
        .project_clarification_event(
            event_id,
            &fixture.session_id,
            daemon_event_seq,
            &canonical_payload_digest(&value),
            &payload,
            ClarificationEvent::Failed {
                recoverable,
                reason: reason.to_owned(),
            },
        )
        .await
}

fn resume_payload(
    _daemon_id: &str,
    session_id: &str,
    command_id: &str,
    sequence: u64,
) -> Result<String, PersistenceError> {
    ServerFrame::Command(CommandEnvelope {
        command_id: command_id.to_owned(),
        session_id: session_id.to_owned(),
        server_command_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".to_owned(),
        schema_version: SCHEMA_VERSION,
        command: Command::SessionResume(SessionResume {}),
    })
    .to_json()
    .map_err(|_| PersistenceError::InvalidCommandPayload)
}

async fn next_server_frame<S>(socket: &mut WebSocketStream<S>) -> ServerFrame
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("server frame deadline")
            .expect("server frame")
            .expect("WebSocket transport");
        match message {
            Message::Text(text) => {
                return ServerFrame::from_json(text.as_ref()).expect("server frame")
            }
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.expect("pong"),
            Message::Close(_) => panic!("server closed before protocol frame"),
            other => panic!("unexpected server message: {other:?}"),
        }
    }
}

fn hello(daemon_id: &str, credential: &str) -> DaemonFrame {
    DaemonFrame::Hello(north_protocol::Hello::new(
        daemon_id,
        credential,
        vec!["agent".to_owned()],
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn postgres_retry_claim_is_skip_locked_atomic_and_restart_safe(
) -> Result<(), Box<dyn std::error::Error>> {
    let _database_test_guard = database_test_lock().await;
    let pool = test_pool().await;
    let fixture = retry_fixture(&pool, "retry-claim").await;
    acknowledge_start(&pool, &fixture).await;
    project_failure(
        &fixture,
        &unique_id("retry-failure"),
        1,
        false,
        "runtime process exited",
    )
    .await
    .expect("project runtime failure");
    set_daemon_offline(&pool, &fixture.daemon_id).await;
    set_retry_due(&pool, &fixture.session_id).await;

    let mut lock_transaction = pool.begin().await.expect("begin row lock");
    sqlx::query("SELECT id FROM execution_sessions WHERE id = $1 FOR UPDATE")
        .bind(&fixture.session_id)
        .execute(&mut *lock_transaction)
        .await
        .expect("hold retry row lock");
    let skipped = fixture
        .store
        .claim_due_retries(1, resume_payload)
        .await
        .expect("skip locked claim");
    assert!(skipped.is_empty(), "locked due row must be skipped");
    lock_transaction.rollback().await.expect("release row lock");

    let atomic_failure = AuthStore::new(pool.clone())
        .claim_due_retries(1, |_daemon_id, _session_id, _command_id, _sequence| {
            Err::<String, PersistenceError>(PersistenceError::InvalidCommandPayload)
        })
        .await;
    assert!(matches!(
        atomic_failure,
        Err(ClarificationError::InvalidContext)
    ));
    let (state, attempt_count, current_attempt_id, next_retry_at): (
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT state, attempt_count, current_attempt_id, next_retry_at::text
             FROM execution_sessions WHERE id = $1",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("read rolled back retry");
    assert_eq!(state, "Retrying");
    assert_eq!(attempt_count, 1);
    assert!(current_attempt_id.is_none());
    assert!(next_retry_at.is_some());
    let attempt_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count attempts after rollback");
    let command_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count commands after rollback");
    assert_eq!(attempt_rows, 1);
    assert_eq!(command_rows, 1);

    let restarted = AuthStore::new(pool.clone());
    let persisted = restarted
        .clarification_run(&fixture.requirement_id, &fixture.session_id)
        .await
        .expect("load retry after restart");
    assert_eq!(persisted.status, ClarificationStatus::Retrying);
    assert_eq!(persisted.phase, ClarificationPhase::Active);
    assert_eq!(persisted.attempt_count, 1);
    assert!(persisted.assigned());

    let left_pool = independent_worker_pool().await?;
    let right_pool = independent_worker_pool().await?;
    let left_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&left_pool)
        .await?;
    let right_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&right_pool)
        .await?;
    assert_ne!(left_pid, right_pid);
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let left_barrier = barrier.clone();
    let left_store = AuthStore::new(left_pool);
    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        left_store.claim_due_retries(1, resume_payload).await
    });
    let right_barrier = barrier.clone();
    let right_store = AuthStore::new(right_pool);
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        right_store.claim_due_retries(1, resume_payload).await
    });
    barrier.wait().await;
    let left = left.await.expect("left retry worker").expect("left claim");
    let right = right
        .await
        .expect("right retry worker")
        .expect("right claim");
    assert_eq!(left.len() + right.len(), 1);
    let resume = left
        .into_iter()
        .chain(right)
        .find_map(|work| work.command)
        .expect("one resume command");
    assert_eq!(resume.daemon_id, fixture.daemon_id);
    let (state, attempt_count, current_attempt_id, next_retry_at): (
        String,
        i64,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT state, attempt_count, current_attempt_id, next_retry_at::text
             FROM execution_sessions WHERE id = $1",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("read claimed retry");
    assert_eq!(state, "Running");
    assert_eq!(attempt_count, 2);
    assert!(current_attempt_id.is_some());
    assert!(next_retry_at.is_none());
    let (attempt_id, attempt_command_id): (String, String) = sqlx::query_as(
        "SELECT id, command_id FROM execution_attempts
         WHERE session_id = $1 AND attempt_number = 2",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("read resume attempt");
    assert_eq!(Some(attempt_id), current_attempt_id);
    assert_eq!(attempt_command_id, resume.command_id);
    let attempt_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count committed attempts");
    let command_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count committed commands");
    assert_eq!(attempt_rows, 2);
    assert_eq!(command_rows, 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn postgres_retry_failure_is_idempotent_and_preserves_slot_and_requirement() {
    let _database_test_guard = database_test_lock().await;
    let pool = test_pool().await;
    let fixture = retry_fixture(&pool, "retry-isolation").await;
    acknowledge_start(&pool, &fixture).await;
    let requirement_before = fixture
        .store
        .requirement_by_id(&fixture.requirement_id)
        .await
        .expect("read requirement before failure")
        .expect("requirement before failure");
    let readiness_before = fixture
        .store
        .latest_readiness(&fixture.requirement_id)
        .await
        .expect("readiness before failure");
    let failure_id = unique_id("runtime-failure");
    let receipt = project_failure(&fixture, &failure_id, 1, false, "runtime process exited")
        .await
        .expect("project first failure");
    assert!(!receipt.duplicate);
    let retrying = fixture
        .store
        .clarification_run(&fixture.requirement_id, &fixture.session_id)
        .await
        .expect("read retrying run");
    assert_eq!(retrying.phase, ClarificationPhase::Active);
    assert_eq!(retrying.status, ClarificationStatus::Retrying);
    assert!(retrying.assigned());
    assert!(retrying.next_retry_at.is_some());

    let duplicate = project_failure(&fixture, &failure_id, 1, false, "runtime process exited")
        .await
        .expect("project duplicate failure");
    assert!(duplicate.duplicate);
    let requirement_after = fixture
        .store
        .requirement_by_id(&fixture.requirement_id)
        .await
        .expect("read requirement after failure")
        .expect("requirement after failure");
    assert_eq!(requirement_after, requirement_before);
    assert_eq!(
        fixture
            .store
            .latest_readiness(&fixture.requirement_id)
            .await
            .expect("readiness after failure"),
        readiness_before
    );
    let (attempt_outcome, attempt_class, failure_event_id): (String, String, String) =
        sqlx::query_as(
            "SELECT outcome, failure_class, failure_event_id
             FROM execution_attempts WHERE session_id = $1 AND attempt_number = 1",
        )
        .bind(&fixture.session_id)
        .fetch_one(&pool)
        .await
        .expect("read failed attempt");
    assert_eq!(attempt_outcome, "failed");
    assert_eq!(attempt_class, "runtime_failure");
    assert_eq!(failure_event_id, failure_id);

    let competing_message = fixture
        .store
        .post_requester_message(
            &fixture.requirement_id,
            &requirement_before.created_by,
            "competing clarification",
        )
        .await
        .expect("persist competing message");
    let competing_context = json!({
        "requirement": {"id": fixture.requirement_id, "revision": requirement_before.revision},
        "conversation": {"excerpt": [{
            "message_id": competing_message.id,
            "role": "requester",
            "content": competing_message.body
        }]},
        "repositories": []
    });
    let occupied = fixture
        .store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &fixture.requirement_id,
                start_message_id: &competing_message.id,
                expected_state_version: requirement_before.state_version,
                context_requirement_revision: requirement_before.revision,
                context: &competing_context,
                repository_ids: &[],
                required_capabilities: &["agent".to_owned()],
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await;
    assert!(matches!(
        occupied,
        Err(ClarificationError::ExistingRunDifferentStart)
    ));

    set_retry_due(&pool, &fixture.session_id).await;
    let canceled = fixture
        .store
        .cancel_clarification(
            &fixture.requirement_id,
            &fixture.session_id,
            |_daemon_id, _run_id, _command_id, _sequence| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("cancel retrying run");
    assert_eq!(canceled.run.phase, ClarificationPhase::Terminal);
    assert_eq!(canceled.run.status, ClarificationStatus::Failed);
    assert_eq!(canceled.run.failure_reason.as_deref(), Some("cancelled"));
    assert!(canceled.command_id.is_empty());
    let later_work = AuthStore::new(pool.clone())
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim after cancellation");
    assert!(later_work.is_empty());
    let attempts_after_cancel: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count attempts after cancellation");
    assert_eq!(attempts_after_cancel, 1);

    let next_message = fixture
        .store
        .post_requester_message(
            &fixture.requirement_id,
            &requirement_before.created_by,
            "new clarification after failure",
        )
        .await
        .expect("persist post-failure message");
    let next_context = json!({
        "requirement": {"id": fixture.requirement_id, "revision": requirement_before.revision},
        "conversation": {"excerpt": [{
            "message_id": next_message.id,
            "role": "requester",
            "content": next_message.body
        }]},
        "repositories": []
    });
    let next = fixture
        .store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &fixture.requirement_id,
                start_message_id: &next_message.id,
                expected_state_version: requirement_before.state_version,
                context_requirement_revision: requirement_before.revision,
                context: &next_context,
                repository_ids: &[],
                required_capabilities: &["agent".to_owned()],
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("start after terminal failure");
    assert_ne!(next.run.run_id, fixture.session_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn postgres_cancellation_race_cannot_create_later_retry_attempt() {
    let _database_test_guard = database_test_lock().await;
    let pool = test_pool().await;
    let fixture = retry_fixture(&pool, "retry-cancel-race").await;
    acknowledge_start(&pool, &fixture).await;
    project_failure(
        &fixture,
        &unique_id("cancel-race-failure"),
        1,
        false,
        "runtime process exited",
    )
    .await
    .expect("project cancellation-race failure");
    set_retry_due(&pool, &fixture.session_id).await;

    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let cancel_barrier = barrier.clone();
    let cancel_store = AuthStore::new(pool.clone());
    let cancel_requirement_id = fixture.requirement_id.clone();
    let cancel_session_id = fixture.session_id.clone();
    let cancel = tokio::spawn(async move {
        cancel_barrier.wait().await;
        cancel_store
            .cancel_clarification(
                &cancel_requirement_id,
                &cancel_session_id,
                |_daemon_id, _run_id, _command_id, _sequence| {
                    Ok::<_, PersistenceError>("{}".to_owned())
                },
            )
            .await
    });
    let retry_barrier = barrier.clone();
    let retry_store = AuthStore::new(pool.clone());
    let retry = tokio::spawn(async move {
        retry_barrier.wait().await;
        retry_store.claim_due_retries(1, resume_payload).await
    });
    barrier.wait().await;
    let canceled = cancel
        .await
        .expect("cancellation race task")
        .expect("cancellation race");
    let retry_work = retry.await.expect("retry race task").expect("retry race");
    let run = fixture
        .store
        .clarification_run(&fixture.requirement_id, &fixture.session_id)
        .await
        .expect("read cancellation race result");
    let resume_attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_attempts
         WHERE session_id = $1 AND command_kind = 'session.resume'",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("count cancellation-race resumes");
    match run.status {
        ClarificationStatus::Failed => {
            assert_eq!(run.failure_reason.as_deref(), Some("cancelled"));
            assert!(retry_work.is_empty());
            assert!(canceled.command_id.is_empty());
            assert_eq!(resume_attempts, 0);
        }
        ClarificationStatus::Starting | ClarificationStatus::Running => {
            assert!(run.cancel_requested);
            assert_eq!(run.attempt_count, 2);
            assert_eq!(retry_work.len(), 1);
            assert!(!canceled.command_id.is_empty());
            assert_eq!(resume_attempts, 1);
        }
        status => panic!("unexpected cancellation-race status: {status:?}"),
    }
    assert!(fixture
        .store
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim after cancellation race")
        .is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn postgres_retry_exhaustion_and_unknown_outcomes_are_terminal_once() {
    let _database_test_guard = database_test_lock().await;
    let pool = test_pool().await;
    let exhausted = retry_fixture(&pool, "retry-exhaustion").await;
    acknowledge_start(&pool, &exhausted).await;
    sqlx::query("UPDATE execution_sessions SET max_attempts = 1 WHERE id = $1")
        .bind(&exhausted.session_id)
        .execute(&pool)
        .await
        .expect("set one-attempt budget");
    let exhausted_failure_id = unique_id("exhausted-failure");
    project_failure(
        &exhausted,
        &exhausted_failure_id,
        1,
        false,
        "runtime process exited",
    )
    .await
    .expect("project exhausted failure");
    let exhausted_run = exhausted
        .store
        .clarification_run(&exhausted.requirement_id, &exhausted.session_id)
        .await
        .expect("read exhausted run");
    assert_eq!(exhausted_run.phase, ClarificationPhase::Terminal);
    assert_eq!(exhausted_run.status, ClarificationStatus::Failed);
    assert_eq!(
        exhausted_run.failure_reason.as_deref(),
        Some("retry_exhausted")
    );
    assert!(exhausted_run.next_retry_at.is_none());
    let duplicate = project_failure(
        &exhausted,
        &exhausted_failure_id,
        1,
        false,
        "runtime process exited",
    )
    .await
    .expect("project duplicate exhausted failure");
    assert!(duplicate.duplicate);
    let (state, attempt_count, next_retry_at): (String, i64, Option<String>) = sqlx::query_as(
        "SELECT state, attempt_count, next_retry_at::text
         FROM execution_sessions WHERE id = $1",
    )
    .bind(&exhausted.session_id)
    .fetch_one(&pool)
    .await
    .expect("read exhausted state");
    assert_eq!(state, "Failed");
    assert_eq!(attempt_count, 1);
    assert!(next_retry_at.is_none());
    let exhausted_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_event_dedupe WHERE session_id = $1")
            .bind(&exhausted.session_id)
            .fetch_one(&pool)
            .await
            .expect("count exhausted events");
    let exhausted_attempts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&exhausted.session_id)
            .fetch_one(&pool)
            .await
            .expect("count exhausted attempts");
    assert_eq!(exhausted_events, 1);
    assert_eq!(exhausted_attempts, 1);
    assert!(exhausted
        .store
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim after exhaustion")
        .is_empty());

    let requirement = exhausted
        .store
        .requirement_by_id(&exhausted.requirement_id)
        .await
        .expect("read requirement after exhaustion")
        .expect("requirement after exhaustion");
    let next_message = exhausted
        .store
        .post_requester_message(
            &exhausted.requirement_id,
            &requirement.created_by,
            "new clarification after exhaustion",
        )
        .await
        .expect("persist post-exhaustion message");
    let next_context = json!({
        "requirement": {"id": exhausted.requirement_id, "revision": requirement.revision},
        "conversation": {"excerpt": [{
            "message_id": next_message.id,
            "role": "requester",
            "content": next_message.body
        }]},
        "repositories": []
    });
    let next = exhausted
        .store
        .start_clarification(
            ClarificationStartInput {
                requirement_id: &exhausted.requirement_id,
                start_message_id: &next_message.id,
                expected_state_version: requirement.state_version,
                context_requirement_revision: requirement.revision,
                context: &next_context,
                repository_ids: &[],
                required_capabilities: &["agent".to_owned()],
            },
            |_daemon_id, _run_id, _command_id, _sequence, _context| {
                Ok::<_, PersistenceError>("{}".to_owned())
            },
        )
        .await
        .expect("start after exhausted failure");
    assert_ne!(next.run.run_id, exhausted.session_id);

    let unknown = retry_fixture(&pool, "retry-unknown").await;
    acknowledge_start(&pool, &unknown).await;
    let unknown_failure_id = unique_id("unknown-failure");
    project_failure(
        &unknown,
        &unknown_failure_id,
        1,
        true,
        "execution_outcome_unknown",
    )
    .await
    .expect("project unknown outcome");
    let unknown_run = unknown
        .store
        .clarification_run(&unknown.requirement_id, &unknown.session_id)
        .await
        .expect("read unknown run");
    assert_eq!(unknown_run.phase, ClarificationPhase::Terminal);
    assert_eq!(unknown_run.status, ClarificationStatus::Failed);
    assert_eq!(
        unknown_run.failure_reason.as_deref(),
        Some("execution_outcome_unknown")
    );
    assert!(unknown_run.next_retry_at.is_none());
    let duplicate = project_failure(
        &unknown,
        &unknown_failure_id,
        1,
        true,
        "execution_outcome_unknown",
    )
    .await
    .expect("project duplicate unknown outcome");
    assert!(duplicate.duplicate);
    assert!(unknown
        .store
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim after unknown outcome")
        .is_empty());
    let unknown_attempts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&unknown.session_id)
            .fetch_one(&pool)
            .await
            .expect("count unknown attempts");
    assert_eq!(unknown_attempts, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn postgres_retry_dispatch_failure_redelivers_same_pinned_command() {
    let _database_test_guard = database_test_lock().await;
    let pool = test_pool().await;
    let fixture = retry_fixture(&pool, "retry-reconnect").await;
    acknowledge_start(&pool, &fixture).await;
    project_failure(
        &fixture,
        &unique_id("reconnect-failure"),
        1,
        false,
        "runtime process exited",
    )
    .await
    .expect("project reconnect failure");
    set_daemon_offline(&pool, &fixture.daemon_id).await;
    set_retry_due(&pool, &fixture.session_id).await;
    let work = AuthStore::new(pool.clone())
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim offline retry");
    let resume = work
        .into_iter()
        .find_map(|item| item.command)
        .expect("durable resume command");
    assert_eq!(resume.daemon_id, fixture.daemon_id);

    let runtime = north_server::daemon::DaemonRuntime::new(fixture.store.clone());
    assert!(matches!(
        runtime.dispatch_pinned_command(&resume).await,
        Err(north_server::daemon::DaemonDispatchError::DaemonUnavailable)
    ));
    let next_poll = fixture
        .store
        .claim_due_retries(1, resume_payload)
        .await
        .expect("claim after dispatch failure");
    assert!(next_poll.is_empty());
    let (state, attempt_count, current_attempt_id): (String, i64, Option<String>) = sqlx::query_as(
        "SELECT state, attempt_count, current_attempt_id
             FROM execution_sessions WHERE id = $1",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("read committed retry after dispatch failure");
    assert_eq!(state, "Running");
    assert_eq!(attempt_count, 2);
    let (attempt_id, attempt_command_id): (String, String) = sqlx::query_as(
        "SELECT id, command_id FROM execution_attempts
         WHERE session_id = $1 AND attempt_number = 2",
    )
    .bind(&fixture.session_id)
    .fetch_one(&pool)
    .await
    .expect("read committed retry attempt");
    assert_eq!(current_attempt_id.as_deref(), Some(attempt_id.as_str()));
    assert_eq!(attempt_command_id, resume.command_id);
    let attempt_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_attempts WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count attempts after dispatch failure");
    let command_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&fixture.session_id)
            .fetch_one(&pool)
            .await
            .expect("count commands after dispatch failure");
    assert_eq!(attempt_rows, 2);
    assert_eq!(command_rows, 2);

    let app = north_server::build_app(pool.clone(), Arc::new(north_server::LogCodeDelivery))
        .await
        .expect("build restarted server");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve restarted server");
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("connect pinned daemon");
    socket
        .send(Message::Text(
            encode_daemon_frame(&hello(&fixture.daemon_id, &fixture.credential))
                .expect("encode hello")
                .into(),
        ))
        .await
        .expect("send hello");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Welcome(_)
    ));
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Reconcile(_)
    ));
    let ServerFrame::Command(command) = next_server_frame(&mut socket).await else {
        panic!("expected durable resume command");
    };
    assert_eq!(command.command_id, resume.command_id);
    assert_eq!(command.session_id, fixture.session_id);
    assert_eq!(command.server_command_seq, 2);
    assert!(matches!(command.command, Command::SessionResume(_)));
    assert!(
        fixture
            .store
            .daemon_by_id(&fixture.daemon_id)
            .await
            .expect("read reconnected daemon")
            .expect("reconnected daemon record")
            .connected
    );
    drop(socket);
    server.abort();
}
