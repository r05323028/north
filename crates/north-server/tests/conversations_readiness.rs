use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use north_domain::role::Role;
use north_persistence::{AuthStore, PoolOptions};
use north_protocol::{
    EventAckStatus, ReadinessVerdictWire, RequirementAssessed, ReviewedRepositoryWire,
};
use north_server::{
    assessment::process_requirement_assessed, requirements, AuthState, CurrentUser,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn app(pool: north_persistence::PgPool, id: &str, role: Role) -> Router {
    requirements::router()
        .with_state(AuthState::with_log_delivery(AuthStore::new(pool)))
        .layer(axum::Extension(CurrentUser(
            north_persistence::UserRecord {
                id: id.into(),
                email: format!("{id}@example.com"),
                role,
            },
        )))
}

async fn request(app: Router, method: Method, uri: &str, body: Value) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request"),
    )
    .await
    .expect("response")
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json")
}

async fn setup_user_with_role(
    pool: &north_persistence::PgPool,
    prefix: &str,
    role: &str,
) -> String {
    let id = unique(prefix);
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
        .bind(&id)
        .bind(format!("{id}@example.com"))
        .bind(role)
        .execute(pool)
        .await
        .expect("insert fixture user");
    id
}

async fn setup_user(pool: &north_persistence::PgPool, prefix: &str) -> String {
    setup_user_with_role(pool, prefix, "Requester").await
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn conversation_pruning_preserves_structured_requirement() {
    let database_url = match std::env::var("NORTH_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => panic!(
            "NORTH_TEST_DATABASE_URL is required for conversation/readiness integration tests"
        ),
    };
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let user_id = setup_user(&pool, "conversation-user").await;
    let store = AuthStore::new(pool.clone());
    let requirement = store
        .create_requirement("Conversation", "Structured source", &user_id)
        .await
        .expect("create requirement");
    let requirement_id = requirement.id.clone();

    let first = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/conversation/messages"),
        json!({"body":"Please clarify scope"}),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(json_body(first).await["kind"], "requester");
    let activity_payload = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/conversation/messages"),
        json!({
            "body":"Visible requester message",
            "kind":"agent",
            "activity":"raw tool output must stay telemetry"
        }),
    )
    .await;
    assert_eq!(activity_payload.status(), StatusCode::CREATED);
    let activity_payload = json_body(activity_payload).await;
    assert_eq!(activity_payload["kind"], "requester");
    assert!(activity_payload.get("activity").is_none());

    let second = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/messages"),
        json!({"body":"Keep structured fields canonical"}),
    )
    .await;
    assert_eq!(second.status(), StatusCode::CREATED);

    let began = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/begin-discussion"),
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let prepared = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({
            "expected_state_version":2,
            "summary":"Initial handoff",
            "acceptance_criteria":["The conversation remains supporting context"]
        }),
    )
    .await;
    assert_eq!(prepared.status(), StatusCode::OK);
    let prepared_body = json_body(prepared).await;
    assert_eq!(prepared_body["revision"], 2);
    assert_eq!(prepared_body["state_version"], 3);

    let assessment_session_id = unique("conversation-assessment-session");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&assessment_session_id)
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("bind conversation assessment session");
    let assessment = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 2,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: Vec::new(),
        repositories_reviewed: Vec::new(),
    };
    let ack = process_requirement_assessed(
        &AuthStore::new(pool.clone()),
        &unique("conversation-assessment-event"),
        &assessment_session_id,
        1,
        &assessment,
    )
    .await
    .expect("promote conversation requirement");
    assert_eq!(ack.status, EventAckStatus::Accepted);
    let conversation_edit = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}/conversation/structured"),
        json!({"expected_state_version":4,"summary":"Edited beside conversation"}),
    )
    .await;
    assert_eq!(conversation_edit.status(), StatusCode::OK);
    let conversation_edit_body = json_body(conversation_edit).await;
    assert_eq!(conversation_edit_body["status"], "discussing");
    assert_eq!(conversation_edit_body["revision"], 3);
    assert_eq!(conversation_edit_body["state_version"], 5);

    let before_stale = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    let before_stale = json_body(before_stale).await;
    let stale_edit = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}/conversation/structured"),
        json!({"expected_state_version":4,"summary":"stale edit"}),
    )
    .await;
    assert_eq!(stale_edit.status(), StatusCode::CONFLICT);
    let after_stale = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    assert_eq!(before_stale, json_body(after_stale).await);
    let unchanged_conversation = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}/conversation?limit=10"),
        json!({}),
    )
    .await;
    assert_eq!(unchanged_conversation.status(), StatusCode::OK);
    assert_eq!(
        json_body(unchanged_conversation).await["messages"]
            .as_array()
            .expect("messages")
            .len(),
        3
    );

    let page = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}/conversation?limit=1"),
        json!({}),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let page = json_body(page).await;
    assert_eq!(page["messages"].as_array().expect("messages").len(), 1);
    assert_eq!(page["next_offset"], 1);

    let before = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    let before = json_body(before).await;
    sqlx::query(
        "DELETE FROM messages
         WHERE conversation_id = (SELECT id FROM conversations WHERE requirement_id = $1)",
    )
    .bind(&requirement_id)
    .execute(&pool)
    .await
    .expect("prune messages");
    let after = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    assert_eq!(before, json_body(after).await);

    sqlx::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(&assessment_session_id)
        .execute(&pool)
        .await
        .expect("cleanup conversation assessment session");
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn readiness_ingestion_is_revision_bound_and_deduplicated() {
    let database_url = match std::env::var("NORTH_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => panic!(
            "NORTH_TEST_DATABASE_URL is required for conversation/readiness integration tests"
        ),
    };
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let user_id = setup_user(&pool, "readiness-user").await;

    let created = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        "/requirements",
        json!({"title":"Ready requirement","description":"Bounded scope"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let requirement_id = json_body(created).await["id"]
        .as_str()
        .expect("requirement id")
        .to_owned();
    let began = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/begin-discussion"),
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let edited = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({
            "expected_state_version":2,
            "summary":"Human-readable handoff",
            "acceptance_criteria":["Users can finish login"],
            "assumptions":["Canonical assumption"],
            "open_questions":["Unanswered question"]
        }),
    )
    .await;
    assert_eq!(edited.status(), StatusCode::OK);
    let edited_body = json_body(edited).await;
    assert_eq!(edited_body["revision"], 2);
    assert_eq!(edited_body["state_version"], 3);

    let assessment_session_id = unique("assessment-session");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&assessment_session_id)
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("bind assessment session");
    let assessment_event_id = unique("assessment-event");
    let assessment_payload = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 2,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: vec!["One account".into()],
        repositories_reviewed: vec![ReviewedRepositoryWire {
            repository_id: "north".into(),
            commit_sha: "abc123".into(),
        }],
    };
    let store = AuthStore::new(pool.clone());
    let accepted = process_requirement_assessed(
        &store,
        &assessment_event_id,
        &assessment_session_id,
        1,
        &assessment_payload,
    )
    .await
    .expect("process assessment");
    assert_eq!(accepted.status, EventAckStatus::Accepted);

    let ready = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_body = json_body(ready).await;
    assert_eq!(ready_body["status"], "ready");
    assert_eq!(ready_body["revision"], 2);
    assert_eq!(ready_body["state_version"], 4);

    let packet = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    assert_eq!(packet.status(), StatusCode::OK);
    let packet = json_body(packet).await;
    assert_eq!(packet["goal"], "Ready requirement");
    assert_eq!(packet["scope"], "Bounded scope");
    assert_eq!(packet["summary"], "Human-readable handoff");
    assert_eq!(packet["acceptance_criteria"][0], "Users can finish login");
    assert_eq!(packet["assumptions"][0], "Canonical assumption");
    assert_eq!(packet["open_questions"][0], "Unanswered question");
    assert_eq!(packet["blockers"].as_array().expect("blockers").len(), 0);
    assert_eq!(packet["assessment_assumptions"][0], "One account");
    assert_eq!(packet["repositories_reviewed"][0]["repository_id"], "north");
    assert_eq!(packet["repositories_reviewed"][0]["commit_sha"], "abc123");

    let duplicate = process_requirement_assessed(
        &store,
        &assessment_event_id,
        &assessment_session_id,
        1,
        &assessment_payload,
    )
    .await
    .expect("process duplicate assessment");
    assert_eq!(duplicate.status, EventAckStatus::Accepted);
    let after_duplicate = store
        .requirement_by_id(&requirement_id)
        .await
        .expect("read duplicate requirement")
        .expect("duplicate requirement");
    assert_eq!(after_duplicate.revision, 2);
    assert_eq!(after_duplicate.state_version, 4);
    let sequence_conflict = process_requirement_assessed(
        &store,
        &unique("assessment-event-sequence-conflict"),
        &assessment_session_id,
        1,
        &assessment_payload,
    )
    .await;
    assert!(matches!(
        sequence_conflict,
        Err(north_server::assessment::AssessmentError::Persistence(
            north_persistence::ReadinessError::SequenceConflict
        ))
    ));
    let identity_conflict = process_requirement_assessed(
        &store,
        &assessment_event_id,
        &assessment_session_id,
        99,
        &assessment_payload,
    )
    .await;
    assert!(matches!(
        identity_conflict,
        Err(north_server::assessment::AssessmentError::Persistence(
            north_persistence::ReadinessError::EventIdentityConflict
        ))
    ));
    let assessment_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments WHERE requirement_id = $1")
            .bind(&requirement_id)
            .fetch_one(&pool)
            .await
            .expect("count conflict assessments");
    assert_eq!(assessment_total, 1);
    let assessment_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments WHERE event_id = $1")
            .bind(&assessment_event_id)
            .fetch_one(&pool)
            .await
            .expect("count assessments");
    assert_eq!(assessment_count, 1);
    let (outcome, assessed_revision): (String, i64) = sqlx::query_as(
        "SELECT outcome, requirement_revision
         FROM readiness_assessments WHERE event_id = $1",
    )
    .bind(&assessment_event_id)
    .fetch_one(&pool)
    .await
    .expect("read assessment");
    assert_eq!(outcome, "accepted");
    assert_eq!(assessed_revision, 2);
    let ready_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transition_audit
         WHERE requirement_id = $1 AND transition = 'mark_ready'",
    )
    .bind(&requirement_id)
    .fetch_one(&pool)
    .await
    .expect("count readiness audits");
    assert_eq!(ready_audits, 1);
    let immutable_update =
        sqlx::query("UPDATE readiness_assessments SET outcome = 'rejected' WHERE event_id = $1")
            .bind(&assessment_event_id)
            .execute(&pool)
            .await;
    assert!(immutable_update.is_err());
    let immutable_delete = sqlx::query("DELETE FROM readiness_assessments WHERE event_id = $1")
        .bind(&assessment_event_id)
        .execute(&pool)
        .await;
    assert!(immutable_delete.is_err());

    let demoted = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({"expected_state_version":4,"summary":"Changed after assessment"}),
    )
    .await;
    assert_eq!(demoted.status(), StatusCode::OK);
    let demoted_body = json_body(demoted).await;
    assert_eq!(demoted_body["status"], "discussing");
    assert_eq!(demoted_body["revision"], 3);
    assert_eq!(demoted_body["state_version"], 5);

    let stale_event_id = unique("assessment-event-stale");
    let stale_payload = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 2,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: Vec::new(),
        repositories_reviewed: Vec::new(),
    };
    let stale = process_requirement_assessed(
        &store,
        &stale_event_id,
        &assessment_session_id,
        2,
        &stale_payload,
    )
    .await
    .expect("process stale assessment");
    assert_eq!(stale.status, EventAckStatus::Rejected);
    assert!(stale
        .reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("stale_assessment:")));
    let (stale_outcome, stale_revision): (String, i64) = sqlx::query_as(
        "SELECT outcome, requirement_revision
         FROM readiness_assessments WHERE event_id = $1",
    )
    .bind(&stale_event_id)
    .fetch_one(&pool)
    .await
    .expect("read stale assessment");
    assert_eq!(stale_outcome, "rejected");
    assert_eq!(stale_revision, 2);

    let blocked_payload = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 3,
        verdict: ReadinessVerdictWire::Ready,
        blockers: vec!["Open scope question".into()],
        assumptions: Vec::new(),
        repositories_reviewed: Vec::new(),
    };
    let blocked = process_requirement_assessed(
        &store,
        &unique("assessment-event-blocked"),
        &assessment_session_id,
        3,
        &blocked_payload,
    )
    .await
    .expect("process blocked assessment");
    assert_eq!(blocked.status, EventAckStatus::Rejected);
    assert_eq!(blocked.reason.as_deref(), Some("blockers_present"));

    let unclear_payload = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 3,
        verdict: ReadinessVerdictWire::NeedsClarification,
        blockers: Vec::new(),
        assumptions: Vec::new(),
        repositories_reviewed: Vec::new(),
    };
    let unclear = process_requirement_assessed(
        &store,
        &unique("assessment-event-verdict"),
        &assessment_session_id,
        4,
        &unclear_payload,
    )
    .await
    .expect("process unclear assessment");
    assert_eq!(unclear.status, EventAckStatus::Rejected);
    assert_eq!(unclear.reason.as_deref(), Some("verdict_not_ready"));
    let unchanged_after_rejections = store
        .requirement_by_id(&requirement_id)
        .await
        .expect("read rejected-assessment requirement")
        .expect("rejected-assessment requirement");
    assert_eq!(
        unchanged_after_rejections.status,
        north_domain::status::RequirementStatus::Discussing
    );
    assert_eq!(unchanged_after_rejections.revision, 3);
    assert_eq!(unchanged_after_rejections.state_version, 5);

    let removed_criteria = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({"expected_state_version":5,"acceptance_criteria":[]}),
    )
    .await;
    assert_eq!(removed_criteria.status(), StatusCode::OK);
    let removed_body = json_body(removed_criteria).await;
    assert_eq!(removed_body["revision"], 4);
    assert_eq!(removed_body["state_version"], 6);
    let missing_criteria_payload = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 4,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: Vec::new(),
        repositories_reviewed: Vec::new(),
    };
    let missing_criteria = process_requirement_assessed(
        &store,
        &unique("assessment-event-criteria"),
        &assessment_session_id,
        5,
        &missing_criteria_payload,
    )
    .await
    .expect("process missing-criteria assessment");
    assert_eq!(missing_criteria.status, EventAckStatus::Rejected);
    assert_eq!(
        missing_criteria.reason.as_deref(),
        Some("missing_acceptance_criteria")
    );

    let current = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    let current = json_body(current).await;
    assert_eq!(current["status"], "discussing");
    assert_eq!(current["revision"], 4);
    assert_eq!(current["state_version"], 6);
    let ready_audits_after_stale: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transition_audit
         WHERE requirement_id = $1 AND transition = 'mark_ready'",
    )
    .bind(&requirement_id)
    .fetch_one(&pool)
    .await
    .expect("count readiness audits after stale event");
    assert_eq!(ready_audits_after_stale, 1);
    let stale_packet = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    assert_eq!(stale_packet.status(), StatusCode::CONFLICT);

    sqlx::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(&assessment_session_id)
        .execute(&pool)
        .await
        .expect("cleanup assessment session");
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn stale_review_cannot_decide_replaced_readiness_assessment() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for review race integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let requester_id = setup_user(&pool, "review-requester").await;
    let manager_id = setup_user_with_role(&pool, "review-manager", "RequirementManager").await;
    let store = AuthStore::new(pool.clone());

    let created = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        "/requirements",
        json!({"title":"Review race","description":"Same content revision"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let requirement_id = json_body(created).await["id"]
        .as_str()
        .expect("requirement id")
        .to_owned();
    let began = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/begin-discussion"),
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let edited = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({
            "expected_state_version":2,
            "acceptance_criteria":["Reviewers can verify this content"],
            "summary":"Stable content"
        }),
    )
    .await;
    assert_eq!(edited.status(), StatusCode::OK);
    let edited = json_body(edited).await;
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["state_version"], 3);

    let session_a = unique("review-session-a");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&session_a)
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("bind assessment A session");
    let assessment_a = RequirementAssessed {
        requirement_id: requirement_id.clone(),
        requirement_revision: 2,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: vec!["Assessment A".into()],
        repositories_reviewed: Vec::new(),
    };
    let ack_a = process_requirement_assessed(
        &store,
        &unique("review-event-a"),
        &session_a,
        1,
        &assessment_a,
    )
    .await
    .expect("commit assessment A");
    assert_eq!(ack_a.status, EventAckStatus::Accepted);
    let assessment_a_generation: i64 = sqlx::query_scalar(
        "SELECT accepted_state_version FROM readiness_assessments WHERE event_id = $1",
    )
    .bind(&ack_a.event_id)
    .fetch_one(&pool)
    .await
    .expect("read assessment A generation");
    assert_eq!(assessment_a_generation, 4);

    let packet_a_response = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    assert_eq!(packet_a_response.status(), StatusCode::OK);
    let packet_a = json_body(packet_a_response).await;
    let assessment_a_id = packet_a["assessment_id"]
        .as_str()
        .expect("assessment A id")
        .to_owned();
    let packet_a_revision = packet_a["requirement_revision"]
        .as_u64()
        .expect("assessment A revision");
    let packet_a_state_version = packet_a["requirement_state_version"]
        .as_u64()
        .expect("assessment A state version");
    assert_eq!(packet_a_revision, 2);
    assert_eq!(packet_a_state_version, 4);
    let requester_review = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/request-changes"),
        json!({
            "expected_state_version":packet_a_state_version,
            "assessment_id":assessment_a_id,
            "feedback":"Requester cannot review"
        }),
    )
    .await;
    assert_eq!(requester_review.status(), StatusCode::FORBIDDEN);

    let changes = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &format!("/requirements/{requirement_id}/request-changes"),
        json!({
            "expected_state_version":packet_a_state_version,
            "assessment_id":assessment_a_id,
            "feedback":"Recheck evidence"
        }),
    )
    .await;
    assert_eq!(changes.status(), StatusCode::OK);
    let changes = json_body(changes).await;
    assert_eq!(changes["status"], "discussing");
    assert_eq!(changes["revision"], packet_a_revision);
    assert_eq!(changes["state_version"], 5);

    let session_b = unique("review-session-b");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&session_b)
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("bind assessment B session");
    let assessment_b = RequirementAssessed {
        assumptions: vec!["Assessment B".into()],
        ..assessment_a.clone()
    };
    let ack_b = process_requirement_assessed(
        &store,
        &unique("review-event-b"),
        &session_b,
        1,
        &assessment_b,
    )
    .await
    .expect("commit assessment B");
    assert_eq!(ack_b.status, EventAckStatus::Accepted);
    let assessment_b_generation: i64 = sqlx::query_scalar(
        "SELECT accepted_state_version FROM readiness_assessments WHERE event_id = $1",
    )
    .bind(&ack_b.event_id)
    .fetch_one(&pool)
    .await
    .expect("read assessment B generation");
    assert_eq!(assessment_b_generation, 6);

    let packet_b_response = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    assert_eq!(packet_b_response.status(), StatusCode::OK);
    let packet_b = json_body(packet_b_response).await;
    let assessment_b_id = packet_b["assessment_id"]
        .as_str()
        .expect("assessment B id")
        .to_owned();
    let packet_b_state_version = packet_b["requirement_state_version"]
        .as_u64()
        .expect("assessment B state version");
    assert_ne!(assessment_a_id, assessment_b_id);
    assert_eq!(packet_b["requirement_revision"], packet_a_revision);
    assert_eq!(packet_b_state_version, 6);

    let stale = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &format!("/requirements/{requirement_id}/accept"),
        json!({
            "expected_state_version":packet_a_state_version,
            "assessment_id":assessment_a_id
        }),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale_evidence = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &format!("/requirements/{requirement_id}/accept"),
        json!({
            "expected_state_version":packet_b_state_version,
            "assessment_id":assessment_a_id
        }),
    )
    .await;
    assert_eq!(stale_evidence.status(), StatusCode::CONFLICT);
    let before_accept = store
        .requirement_by_id(&requirement_id)
        .await
        .expect("read stale review requirement")
        .expect("stale review requirement");
    assert_eq!(
        before_accept.status,
        north_domain::status::RequirementStatus::Ready
    );
    assert_eq!(before_accept.revision, packet_a_revision);
    assert_eq!(before_accept.state_version, packet_b_state_version);
    let audit_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transition_audit WHERE requirement_id = $1")
            .bind(&requirement_id)
            .fetch_one(&pool)
            .await
            .expect("count stale review audits");
    assert_eq!(audit_count, 4);

    let accepted = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &format!("/requirements/{requirement_id}/accept"),
        json!({
            "expected_state_version":packet_b_state_version,
            "assessment_id":assessment_b_id
        }),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let accepted = json_body(accepted).await;
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["revision"], packet_a_revision);
    assert_eq!(accepted["state_version"], 7);
    let (audit_assessment_id, audit_state_version): (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT assessment_id, state_version
             FROM transition_audit
             WHERE requirement_id = $1 AND transition = 'accept'
             ORDER BY id DESC LIMIT 1",
    )
    .bind(&requirement_id)
    .fetch_one(&pool)
    .await
    .expect("read review audit provenance");
    assert_eq!(
        audit_assessment_id.as_deref(),
        Some(assessment_b_id.as_str())
    );
    assert_eq!(audit_state_version, Some(7));
    sqlx::query("DELETE FROM execution_sessions WHERE id IN ($1, $2)")
        .bind(&session_a)
        .bind(&session_b)
        .execute(&pool)
        .await
        .expect("cleanup review sessions");
    let requirement_delete = sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await;
    assert!(requirement_delete.is_err());
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn workspace_users_can_collaborate_without_requirement_acl() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for workspace policy tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let creator_id = setup_user(&pool, "workspace-creator").await;
    let collaborator_id = setup_user(&pool, "workspace-collaborator").await;
    let created = request(
        app(pool.clone(), &creator_id, Role::Requester),
        Method::POST,
        "/requirements",
        json!({"title":"Shared","description":"Workspace visible"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    let requirement_id = created["id"].as_str().expect("requirement id").to_owned();

    let visible = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    assert_eq!(visible.status(), StatusCode::OK);
    let began = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/begin-discussion"),
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let message = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::POST,
        &format!("/requirements/{requirement_id}/conversation/messages"),
        json!({"body":"Collaborative context"}),
    )
    .await;
    assert_eq!(message.status(), StatusCode::CREATED);
    let requirement_uri = format!("/requirements/{requirement_id}");
    let left = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::PATCH,
        &requirement_uri,
        json!({"expected_state_version":2,"summary":"Left edit"}),
    );
    let right = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::PATCH,
        &requirement_uri,
        json!({"expected_state_version":2,"summary":"Right edit"}),
    );
    let (left, right) = tokio::join!(left, right);
    assert!(
        (left.status() == StatusCode::OK && right.status() == StatusCode::CONFLICT)
            || (left.status() == StatusCode::CONFLICT && right.status() == StatusCode::OK)
    );
    let edited = request(
        app(pool.clone(), &collaborator_id, Role::Requester),
        Method::GET,
        &format!("/requirements/{requirement_id}"),
        json!({}),
    )
    .await;
    let edited = json_body(edited).await;
    assert!(edited["summary"] == "Left edit" || edited["summary"] == "Right edit");
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["state_version"], 3);

    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("cleanup workspace requirement");
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(&creator_id)
        .bind(&collaborator_id)
        .execute(&pool)
        .await
        .expect("cleanup workspace users");
}
