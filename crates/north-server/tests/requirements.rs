use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Extension, Router,
};
use north_domain::role::Role;
use north_persistence::AuthStore;
use north_protocol::{EventAckStatus, ReadinessVerdictWire, RequirementAssessed};
use north_server::{
    assessment::process_requirement_assessed, requirements, AuthState, CurrentUser,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

#[allow(dead_code)]
mod support;
use support::TestDatabaseOptions;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn app(pool: north_persistence::DatabaseConnection, id: &str, role: Role) -> Router {
    requirements::router()
        .with_state(AuthState::with_log_delivery(AuthStore::new(
            pool,
            support::test_otp_key(),
        )))
        .layer(Extension(CurrentUser(north_persistence::UserRecord {
            id: id.into(),
            email: format!("{id}@example.com"),
            role,
            created_at: "2026-01-01T00:00:00+00:00".into(),
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

async fn setup_user(
    pool: &north_persistence::DatabaseConnection,
    prefix: &str,
    role: &str,
) -> String {
    let id = unique(prefix);
    support::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
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
    pool: &north_persistence::DatabaseConnection,
    prefix: &str,
    requirement_id: &str,
) -> String {
    let session_id = unique(prefix);
    support::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
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
    let pool = TestDatabaseOptions::new()
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
        &AuthStore::new(pool.clone(), support::test_otp_key()),
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
        support::query_scalar("SELECT COUNT(*) FROM transition_audit WHERE requirement_id = $1")
            .bind(&requirement_id)
            .fetch_one(&pool)
            .await
            .expect("audit count");
    assert_eq!(audit_count, 3);

    support::query("DELETE FROM server_event_dedupe WHERE session_id = $1")
        .bind(&session_id)
        .execute(&pool)
        .await
        .expect("cleanup event tombstones");
    support::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(&session_id)
        .execute(&pool)
        .await
        .expect("cleanup assessment session");
}

async fn create_board_requirement(app: Router, title: &str) -> Value {
    let response = request(
        app,
        Method::POST,
        "/requirements",
        json!({"title": title, "description": "Board ordering fixture"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn board_ordering_is_persistent_and_serializes_with_lifecycle_changes() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for requirement integration tests");
    let pool = TestDatabaseOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let user_id = setup_user(&pool, "board-owner", "Owner").await;
    let app = app(pool.clone(), &user_id, Role::Owner);

    let first = create_board_requirement(app.clone(), "Board first").await;
    let middle = create_board_requirement(app.clone(), "Board middle").await;
    let last = create_board_requirement(app.clone(), "Board last").await;
    let first_id = first["id"].as_str().expect("first id").to_owned();
    let middle_id = middle["id"].as_str().expect("middle id").to_owned();
    let last_id = last["id"].as_str().expect("last id").to_owned();

    support::query(
        "UPDATE requirements SET updated_at = CASE
             WHEN id = $1 THEN CURRENT_TIMESTAMP - INTERVAL '3 minutes'
             WHEN id = $2 THEN CURRENT_TIMESTAMP - INTERVAL '2 minutes'
             WHEN id = $3 THEN CURRENT_TIMESTAMP - INTERVAL '1 minute'
         END
         WHERE id IN ($1, $2, $3)",
    )
    .bind(&first_id)
    .bind(&middle_id)
    .bind(&last_id)
    .execute(&pool)
    .await
    .expect("set deterministic updated-time order");
    let default_order = request(
        app.clone(),
        Method::GET,
        &format!("/requirements?created_by={user_id}"),
        json!(null),
    )
    .await;
    let default_order = json_body(default_order).await;
    let default_ids: Vec<String> = default_order
        .as_array()
        .expect("default collection")
        .iter()
        .map(|requirement| {
            requirement["id"]
                .as_str()
                .expect("requirement id")
                .to_owned()
        })
        .collect();
    assert_eq!(
        default_ids,
        vec![last_id.clone(), middle_id.clone(), first_id.clone()]
    );

    support::query(
        "UPDATE requirement_board_positions SET rank = 1024
         WHERE requirement_id IN ($1, $2)",
    )
    .bind(&first_id)
    .bind(&middle_id)
    .execute(&pool)
    .await
    .expect("create deterministic rank collision");
    support::query("DELETE FROM requirement_board_positions WHERE requirement_id = $1")
        .bind(&last_id)
        .execute(&pool)
        .await
        .expect("create missing board position");
    let collision_order: Vec<String> = support::query_scalar(
        "SELECT id FROM requirements WHERE id IN ($1, $2) ORDER BY created_at ASC, id ASC",
    )
    .bind(&first_id)
    .bind(&middle_id)
    .fetch_all(&pool)
    .await
    .expect("read deterministic collision fallback");
    let mut fallback_order = collision_order;
    fallback_order.push(last_id.clone());

    let board = request(
        app.clone(),
        Method::GET,
        &format!("/requirements?sort=board&created_by={user_id}"),
        json!(null),
    )
    .await;
    assert_eq!(board.status(), StatusCode::OK);
    let board = json_body(board).await;
    let ids: Vec<String> = board
        .as_array()
        .expect("board collection")
        .iter()
        .map(|requirement| {
            requirement["id"]
                .as_str()
                .expect("requirement id")
                .to_owned()
        })
        .collect();
    assert_eq!(ids, fallback_order);

    let moved = request(
        app.clone(),
        Method::POST,
        &format!("/requirements/{middle_id}/reorder"),
        json!({
            "expected_state_version": middle["state_version"],
            "before_id": last_id,
            "after_id": null,
        }),
    )
    .await;
    assert_eq!(moved.status(), StatusCode::NO_CONTENT);
    let board = request(
        app.clone(),
        Method::GET,
        &format!("/requirements?sort=board&created_by={user_id}"),
        json!(null),
    )
    .await;
    let board = json_body(board).await;
    let ids: Vec<String> = board
        .as_array()
        .expect("board collection")
        .iter()
        .map(|requirement| {
            requirement["id"]
                .as_str()
                .expect("requirement id")
                .to_owned()
        })
        .collect();
    assert_eq!(
        ids,
        vec![first_id.clone(), last_id.clone(), middle_id.clone()]
    );

    let unchanged = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{middle_id}"),
        json!(null),
    )
    .await;
    let unchanged = json_body(unchanged).await;
    assert_eq!(unchanged["status"], "draft");
    assert_eq!(unchanged["revision"], middle["revision"]);
    assert_eq!(unchanged["state_version"], middle["state_version"]);

    let no_op = request(
        app.clone(),
        Method::POST,
        &format!("/requirements/{middle_id}/reorder"),
        json!({
            "expected_state_version": middle["state_version"],
            "before_id": last_id,
            "after_id": null,
        }),
    )
    .await;
    assert_eq!(no_op.status(), StatusCode::NO_CONTENT);

    let non_adjacent = request(
        app.clone(),
        Method::POST,
        &format!("/requirements/{first_id}/reorder"),
        json!({
            "expected_state_version": first["state_version"],
            "before_id": middle_id,
            "after_id": last_id,
        }),
    )
    .await;
    assert_eq!(non_adjacent.status(), StatusCode::CONFLICT);
    let status_injection = request(
        app.clone(),
        Method::POST,
        &format!("/requirements/{first_id}/reorder"),
        json!({
            "expected_state_version": first["state_version"],
            "before_id": null,
            "after_id": null,
            "status": "Ready",
        }),
    )
    .await;
    assert!(status_injection.status().is_client_error());

    let racing = create_board_requirement(app.clone(), "Transition race").await;
    let racing_id = racing["id"].as_str().expect("racing id").to_owned();
    let reorder_uri = format!("/requirements/{racing_id}/reorder");
    let transition_uri = format!("/requirements/{racing_id}/begin-discussion");
    let reorder = request(
        app.clone(),
        Method::POST,
        &reorder_uri,
        json!({
            "expected_state_version": racing["state_version"],
            "before_id": first_id,
            "after_id": last_id,
        }),
    );
    let transition = request(
        app.clone(),
        Method::POST,
        &transition_uri,
        json!({"expected_state_version": racing["state_version"]}),
    );
    let (reorder, transition) = tokio::join!(reorder, transition);
    assert!(matches!(
        reorder.status(),
        StatusCode::NO_CONTENT | StatusCode::CONFLICT
    ));
    let transition_status = transition.status();
    let transition_body = json_body(transition).await;
    assert_eq!(
        transition_status,
        StatusCode::OK,
        "transition response: {transition_body}"
    );
    let racing_detail = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{racing_id}"),
        json!(null),
    )
    .await;
    let racing_detail = json_body(racing_detail).await;
    assert_eq!(racing_detail["status"], "discussing");
    assert_eq!(racing_detail["state_version"], 2);
    let stale_reorder = request(
        app.clone(),
        Method::POST,
        &reorder_uri,
        json!({
            "expected_state_version": racing["state_version"],
            "before_id": null,
            "after_id": null,
        }),
    )
    .await;
    assert_eq!(stale_reorder.status(), StatusCode::CONFLICT);
    let cross_column = request(
        app.clone(),
        Method::POST,
        &reorder_uri,
        json!({
            "expected_state_version": racing_detail["state_version"],
            "before_id": first_id,
            "after_id": null,
        }),
    )
    .await;
    assert_eq!(cross_column.status(), StatusCode::CONFLICT);

    let ready_first = create_board_requirement(app.clone(), "Ready first").await;
    let ready_second = create_board_requirement(app.clone(), "Ready second").await;
    let ready_first_id = ready_first["id"]
        .as_str()
        .expect("ready first id")
        .to_owned();
    let ready_second_id = ready_second["id"]
        .as_str()
        .expect("ready second id")
        .to_owned();
    for (requirement_id, prefix) in [
        (ready_first_id.as_str(), "board-ready-first"),
        (ready_second_id.as_str(), "board-ready-second"),
    ] {
        let response = request(
            app.clone(),
            Method::POST,
            &format!("/requirements/{requirement_id}/begin-discussion"),
            json!({"expected_state_version": 1}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let edit = request(
            app.clone(),
            Method::PATCH,
            &format!("/requirements/{requirement_id}"),
            json!({
                "expected_state_version": 2,
                "acceptance_criteria": ["Board review is supported by acceptance criteria"]
            }),
        )
        .await;
        assert_eq!(edit.status(), StatusCode::OK);
        let session_id = bind_session(&pool, prefix, requirement_id).await;
        let assessment = ready_assessment(requirement_id, 2, "board review evidence");
        process_requirement_assessed(
            &AuthStore::new(pool.clone(), support::test_otp_key()),
            &unique(prefix),
            &session_id,
            1,
            &assessment,
        )
        .await
        .expect("mark board requirement ready");
    }

    let packet_before = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{ready_first_id}/review-packet"),
        json!(null),
    )
    .await;
    assert_eq!(packet_before.status(), StatusCode::OK);
    let packet_before = json_body(packet_before).await;
    let ready_detail = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{ready_first_id}"),
        json!(null),
    )
    .await;
    let ready_detail = json_body(ready_detail).await;
    let ready_version = ready_detail["state_version"]
        .as_u64()
        .expect("ready version");
    let ready_revision = ready_detail["revision"].clone();
    let reorder_ready = request(
        app.clone(),
        Method::POST,
        &format!("/requirements/{ready_first_id}/reorder"),
        json!({
            "expected_state_version": ready_version,
            "before_id": ready_second_id,
            "after_id": null,
        }),
    )
    .await;
    assert_eq!(reorder_ready.status(), StatusCode::NO_CONTENT);
    let packet_after = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{ready_first_id}/review-packet"),
        json!(null),
    )
    .await;
    assert_eq!(packet_after.status(), StatusCode::OK);
    assert_eq!(json_body(packet_after).await, packet_before);
    let ready_after = request(
        app.clone(),
        Method::GET,
        &format!("/requirements/{ready_first_id}"),
        json!(null),
    )
    .await;
    let ready_after = json_body(ready_after).await;
    assert_eq!(ready_after["status"], "ready");
    assert_eq!(ready_after["revision"], ready_revision);
    assert_eq!(ready_after["state_version"], ready_version);

    let board = request(
        app.clone(),
        Method::GET,
        &format!("/requirements?sort=board&created_by={user_id}"),
        json!(null),
    )
    .await;
    let board = json_body(board).await;
    let ready_ids: Vec<String> = board
        .as_array()
        .expect("board collection")
        .iter()
        .filter(|requirement| requirement["status"] == "ready")
        .map(|requirement| requirement["id"].as_str().expect("ready id").to_owned())
        .collect();
    assert_eq!(
        ready_ids,
        vec![ready_second_id.clone(), ready_first_id.clone()]
    );

    let demoted = request(
        app.clone(),
        Method::PATCH,
        &format!("/requirements/{ready_first_id}"),
        json!({
            "expected_state_version": ready_version,
            "description": "Edited after readiness"
        }),
    )
    .await;
    assert_eq!(demoted.status(), StatusCode::OK);
    let demoted = json_body(demoted).await;
    assert_eq!(demoted["status"], "discussing");
    assert_eq!(
        demoted["revision"],
        ready_revision.as_u64().expect("ready revision") + 1
    );
    assert_eq!(demoted["state_version"], ready_version + 1);
    let assessments: i64 = support::query_scalar(
        "SELECT COUNT(*)::bigint FROM readiness_assessments WHERE requirement_id = $1",
    )
    .bind(&ready_first_id)
    .fetch_one(&pool)
    .await
    .expect("read retained readiness evidence");
    assert_eq!(assessments, 1);

    let board = request(
        app,
        Method::GET,
        &format!("/requirements?sort=board&created_by={user_id}"),
        json!(null),
    )
    .await;
    let board = json_body(board).await;
    let discussing_ids: Vec<String> = board
        .as_array()
        .expect("board collection")
        .iter()
        .filter(|requirement| requirement["status"] == "discussing")
        .map(|requirement| {
            requirement["id"]
                .as_str()
                .expect("discussing id")
                .to_owned()
        })
        .collect();
    assert_eq!(discussing_ids, vec![racing_id, ready_first_id]);
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn transition_edges_are_state_version_guarded_and_assessment_bound() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for requirement integration tests");
    let pool = TestDatabaseOptions::new()
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
        &AuthStore::new(pool.clone(), support::test_otp_key()),
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
        &AuthStore::new(pool.clone(), support::test_otp_key()),
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

    type AuditRow = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
        bool,
    );
    let audits: Vec<AuditRow> = support::query_tuple(
        "SELECT actor_id, transition, from_status, to_status, feedback,
                assessment_id, state_version, created_at IS NOT NULL
         FROM transition_audit WHERE requirement_id = $1 ORDER BY id ASC",
    )
    .bind(&requirement_id)
    .fetch_all(&pool)
    .await
    .expect("read transition audits");
    assert_eq!(audits.len(), 6);
    assert_eq!(audits[0].1, "begin_discussion");
    assert_eq!(audits[1].1, "mark_ready");
    assert_eq!(audits[2].1, "reject");
    assert_eq!(audits[3].1, "reopen");
    assert_eq!(audits[4].1, "mark_ready");
    assert_eq!(audits[5].1, "request_changes");
    for audit in &audits {
        assert!(audit.7, "transition audit timestamp must be populated");
        assert!(
            audit.6 > 0,
            "transition audit state version must be populated"
        );
    }
    assert_eq!(audits[2].0, manager_id);
    assert_eq!(audits[2].3, "Rejected");
    assert_eq!(audits[2].5.as_deref(), Some(assessment_a_id.as_str()));
    assert_eq!(audits[2].6, 5);
    assert_eq!(audits[3].0, manager_id);
    assert_eq!(audits[3].5, None);
    assert_eq!(audits[3].6, 6);
    assert_eq!(audits[5].0, manager_id);
    assert_eq!(audits[5].4.as_deref(), Some("Clarify account scope"));
    assert_eq!(audits[5].5.as_deref(), Some(assessment_b_id.as_str()));
    assert_eq!(audits[5].6, 8);

    support::query("DELETE FROM server_event_dedupe WHERE session_id IN ($1, $2)")
        .bind(&session_a)
        .bind(&session_b)
        .execute(&pool)
        .await
        .expect("cleanup event tombstones");
    support::query("DELETE FROM execution_sessions WHERE id IN ($1, $2)")
        .bind(&session_a)
        .bind(&session_b)
        .execute(&pool)
        .await
        .expect("cleanup lifecycle sessions");
}
