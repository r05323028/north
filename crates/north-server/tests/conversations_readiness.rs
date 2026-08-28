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

async fn setup_user(pool: &north_persistence::PgPool, prefix: &str) -> String {
    let id = unique(prefix);
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
        .bind(&id)
        .bind(format!("{id}@example.com"))
        .bind("Requester")
        .execute(pool)
        .await
        .expect("insert fixture user");
    id
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

    sqlx::query(
        "UPDATE requirements
         SET status = 'Ready', acceptance_criteria = ARRAY['criterion']::TEXT[]
         WHERE id = $1",
    )
    .bind(&requirement_id)
    .execute(&pool)
    .await
    .expect("make ready fixture");
    let conversation_edit = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}/conversation/structured"),
        json!({"expected_revision":1,"summary":"Edited beside conversation"}),
    )
    .await;
    assert_eq!(conversation_edit.status(), StatusCode::OK);
    let conversation_edit_body = json_body(conversation_edit).await;
    assert_eq!(conversation_edit_body["status"], "discussing");
    assert_eq!(conversation_edit_body["revision"], 2);

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
        json!({"expected_revision":1,"summary":"stale edit"}),
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

    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("cleanup requirement");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
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
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let edited = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({
            "expected_revision":1,
            "summary":"Human-readable handoff",
            "acceptance_criteria":["Users can finish login"],
            "assumptions":["Canonical assumption"],
            "open_questions":["Unanswered question"]
        }),
    )
    .await;
    assert_eq!(edited.status(), StatusCode::OK);
    assert_eq!(json_body(edited).await["revision"], 2);

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
    assert_eq!(json_body(ready).await["status"], "ready");

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
        json!({"expected_revision":2,"summary":"Changed after assessment"}),
    )
    .await;
    assert_eq!(demoted.status(), StatusCode::OK);
    assert_eq!(json_body(demoted).await["status"], "discussing");

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

    let removed_criteria = request(
        app(pool.clone(), &user_id, Role::Requester),
        Method::PATCH,
        &format!("/requirements/{requirement_id}"),
        json!({"expected_revision":3,"acceptance_criteria":[]}),
    )
    .await;
    assert_eq!(removed_criteria.status(), StatusCode::OK);
    assert_eq!(json_body(removed_criteria).await["revision"], 4);
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
    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("cleanup requirement");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("cleanup user");
}
