use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Extension, Router,
};
use north_domain::role::Role;
use north_persistence::{AuthStore, PoolOptions};
use north_protocol::{EventAckStatus, ReadinessVerdictWire, RequirementAssessed};
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
        .layer(Extension(CurrentUser(north_persistence::UserRecord {
            id: id.into(),
            email: format!("{id}@example.com"),
            role,
        })))
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

async fn setup_user(pool: &north_persistence::PgPool, prefix: &str, role: &str) -> String {
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

fn ready_assessment(
    requirement_id: &str,
    requirement_revision: u64,
    assumption: &str,
) -> RequirementAssessed {
    RequirementAssessed {
        requirement_id: requirement_id.into(),
        requirement_revision,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: vec![assumption.into()],
        repositories_reviewed: Vec::new(),
    }
}

async fn bind_session(
    pool: &north_persistence::PgPool,
    prefix: &str,
    requirement_id: &str,
) -> String {
    let session_id = unique(prefix);
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&session_id)
        .bind(requirement_id)
        .execute(pool)
        .await
        .expect("bind assessment session");
    session_id
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn requirement_api_enforces_state_version_and_review_contracts() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for requirement integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let requester_id = setup_user(&pool, "requirement-requester", "Requester").await;
    let manager_id = setup_user(&pool, "requirement-manager", "RequirementManager").await;

    let created = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        "/requirements",
        json!({"title":"Login", "description":"Describe login"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    let requirement_id = created["id"].as_str().expect("requirement id").to_owned();
    assert_eq!(created["status"], "draft");
    assert_eq!(created["revision"], 1);
    assert_eq!(created["state_version"], 1);

    let listed = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::GET,
        "/requirements?search=login&status=draft&sort=updated_asc",
        json!({}),
    )
    .await;
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await.as_array().expect("list").len(), 1);

    let begin_uri = format!("/requirements/{requirement_id}/begin-discussion");
    let began = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &begin_uri,
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    assert_eq!(json_body(began).await["state_version"], 2);

    let accept_uri = format!("/requirements/{requirement_id}/accept");
    let denied = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &accept_uri,
        json!({"expected_state_version":2}),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let edit_uri = format!("/requirements/{requirement_id}");
    let edited = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({"expected_state_version":2,"summary":"first"}),
    )
    .await;
    assert_eq!(edited.status(), StatusCode::OK);
    let edited = json_body(edited).await;
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["state_version"], 3);

    let stale = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({"expected_state_version":2,"summary":"stale"}),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let current = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::GET,
        &edit_uri,
        json!({}),
    )
    .await;
    let current = json_body(current).await;
    assert_eq!(current["summary"], "first");
    assert_eq!(current["revision"], 2);
    assert_eq!(current["state_version"], 3);

    let cleared = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({
            "expected_state_version":3,
            "summary":"",
            "acceptance_criteria":["Users can finish login"]
        }),
    )
    .await;
    assert_eq!(cleared.status(), StatusCode::OK);
    let cleared = json_body(cleared).await;
    assert_eq!(cleared["summary"], "");
    assert_eq!(cleared["revision"], 3);
    assert_eq!(cleared["state_version"], 4);

    let noop = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({
            "expected_state_version":4,
            "summary":"",
            "acceptance_criteria":["Users can finish login"]
        }),
    )
    .await;
    assert_eq!(noop.status(), StatusCode::OK);
    let noop = json_body(noop).await;
    assert_eq!(noop["revision"], 3);
    assert_eq!(noop["state_version"], 4);

    let session_id = bind_session(&pool, "requirement-assessment", &requirement_id).await;
    let assessment = ready_assessment(&requirement_id, 3, "current evidence");
    let ack = process_requirement_assessed(
        &AuthStore::new(pool.clone()),
        &unique("requirement-assessment-event"),
        &session_id,
        1,
        &assessment,
    )
    .await
    .expect("process assessment");
    assert_eq!(ack.status, EventAckStatus::Accepted);

    let ready = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::GET,
        &edit_uri,
        json!({}),
    )
    .await;
    let ready = json_body(ready).await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["revision"], 3);
    assert_eq!(ready["state_version"], 5);
    let packet = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    assert_eq!(packet.status(), StatusCode::OK);
    let packet = json_body(packet).await;
    let assessment_id = packet["assessment_id"]
        .as_str()
        .expect("assessment id")
        .to_owned();
    assert_eq!(packet["requirement_revision"], 3);
    assert_eq!(packet["requirement_state_version"], 5);

    let accepted = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &accept_uri,
        json!({
            "expected_state_version":5,
            "assessment_id":assessment_id
        }),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let accepted = json_body(accepted).await;
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["state_version"], 6);

    let terminal = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::PATCH,
        &edit_uri,
        json!({"expected_state_version":6,"summary":"forbidden"}),
    )
    .await;
    assert_eq!(terminal.status(), StatusCode::BAD_REQUEST);
    let audit_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transition_audit WHERE requirement_id = $1")
            .bind(&requirement_id)
            .fetch_one(&pool)
            .await
            .expect("audit count");
    assert_eq!(audit_count, 3);

    sqlx::query("DELETE FROM server_event_dedupe WHERE session_id = $1")
        .bind(&session_id)
        .execute(&pool)
        .await
        .expect("cleanup event tombstones");
    sqlx::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(&session_id)
        .execute(&pool)
        .await
        .expect("cleanup assessment session");
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn transition_edges_are_state_version_guarded_and_assessment_bound() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for requirement integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let requester_id = setup_user(&pool, "lifecycle-requester", "Requester").await;
    let manager_id = setup_user(&pool, "lifecycle-manager", "RequirementManager").await;

    let created = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        "/requirements",
        json!({"title":"Lifecycle","description":"Transition coverage"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let requirement_id = json_body(created).await["id"]
        .as_str()
        .expect("requirement id")
        .to_owned();
    let begin_uri = format!("/requirements/{requirement_id}/begin-discussion");
    let began = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &begin_uri,
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    let illegal = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &begin_uri,
        json!({"expected_state_version":2}),
    )
    .await;
    assert_eq!(illegal.status(), StatusCode::BAD_REQUEST);
    let stale = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &begin_uri,
        json!({"expected_state_version":1}),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);

    let edit_uri = format!("/requirements/{requirement_id}");
    let criteria = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({
            "expected_state_version":2,
            "acceptance_criteria":["The review path is auditable"]
        }),
    )
    .await;
    assert_eq!(criteria.status(), StatusCode::OK);
    let criteria = json_body(criteria).await;
    assert_eq!(criteria["revision"], 2);
    assert_eq!(criteria["state_version"], 3);

    let session_a = bind_session(&pool, "lifecycle-session-a", &requirement_id).await;
    let assessment_a = ready_assessment(&requirement_id, 2, "A");
    let ack_a = process_requirement_assessed(
        &AuthStore::new(pool.clone()),
        &unique("lifecycle-event-a"),
        &session_a,
        1,
        &assessment_a,
    )
    .await
    .expect("process assessment A");
    assert_eq!(ack_a.status, EventAckStatus::Accepted);
    let packet_a = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    let packet_a = json_body(packet_a).await;
    let assessment_a_id = packet_a["assessment_id"]
        .as_str()
        .expect("assessment A id")
        .to_owned();
    assert_eq!(packet_a["requirement_revision"], 2);
    assert_eq!(packet_a["requirement_state_version"], 4);

    let reject_uri = format!("/requirements/{requirement_id}/reject");
    let rejected = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &reject_uri,
        json!({
            "expected_state_version":4,
            "assessment_id":assessment_a_id
        }),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::OK);
    let rejected = json_body(rejected).await;
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["state_version"], 5);

    let reopen_uri = format!("/requirements/{requirement_id}/reopen");
    let reopened = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &reopen_uri,
        json!({"expected_state_version":5}),
    )
    .await;
    assert_eq!(reopened.status(), StatusCode::OK);
    assert_eq!(json_body(reopened).await["state_version"], 6);

    let session_b = bind_session(&pool, "lifecycle-session-b", &requirement_id).await;
    let assessment_b = ready_assessment(&requirement_id, 2, "B");
    let ack_b = process_requirement_assessed(
        &AuthStore::new(pool.clone()),
        &unique("lifecycle-event-b"),
        &session_b,
        1,
        &assessment_b,
    )
    .await
    .expect("process assessment B");
    assert_eq!(ack_b.status, EventAckStatus::Accepted);
    let packet_b = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::GET,
        &format!("/requirements/{requirement_id}/review-packet"),
        json!({}),
    )
    .await;
    let packet_b = json_body(packet_b).await;
    let assessment_b_id = packet_b["assessment_id"]
        .as_str()
        .expect("assessment B id")
        .to_owned();
    assert_eq!(packet_b["requirement_revision"], 2);
    assert_eq!(packet_b["requirement_state_version"], 7);

    let request_changes_uri = format!("/requirements/{requirement_id}/request-changes");
    let changes = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &request_changes_uri,
        json!({
            "expected_state_version":7,
            "assessment_id":assessment_b_id,
            "feedback":"Clarify account scope"
        }),
    )
    .await;
    assert_eq!(changes.status(), StatusCode::OK);
    let changes = json_body(changes).await;
    assert_eq!(changes["status"], "discussing");
    assert_eq!(changes["revision"], 2);
    assert_eq!(changes["state_version"], 8);

    let audits: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT transition, from_status, to_status, feedback
         FROM transition_audit WHERE requirement_id = $1 ORDER BY id ASC",
    )
    .bind(&requirement_id)
    .fetch_all(&pool)
    .await
    .expect("read transition audits");
    assert_eq!(audits.len(), 6);
    assert_eq!(audits[0].0, "begin_discussion");
    assert_eq!(audits[1].0, "mark_ready");
    assert_eq!(audits[2].0, "reject");
    assert_eq!(audits[3].0, "reopen");
    assert_eq!(audits[4].0, "mark_ready");
    assert_eq!(audits[5].0, "request_changes");
    assert_eq!(audits[5].3.as_deref(), Some("Clarify account scope"));

    sqlx::query("DELETE FROM server_event_dedupe WHERE session_id IN ($1, $2)")
        .bind(&session_a)
        .bind(&session_b)
        .execute(&pool)
        .await
        .expect("cleanup event tombstones");
    sqlx::query("DELETE FROM execution_sessions WHERE id IN ($1, $2)")
        .bind(&session_a)
        .bind(&session_b)
        .execute(&pool)
        .await
        .expect("cleanup lifecycle sessions");
}
