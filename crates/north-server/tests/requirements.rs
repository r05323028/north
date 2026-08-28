use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Extension, Router,
};
use north_domain::role::Role;
use north_persistence::{AuthStore, PoolOptions};
use north_server::{requirements, AuthState, CurrentUser};
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

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn requirement_api_enforces_domain_and_revision_contracts() {
    let database_url = match std::env::var("NORTH_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => panic!("NORTH_TEST_DATABASE_URL is required for requirement integration tests"),
    };
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let requester_id = unique("requirement-requester");
    let manager_id = unique("requirement-manager");
    for (id, role) in [
        (&requester_id, "Requester"),
        (&manager_id, "RequirementManager"),
    ] {
        sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(format!("{id}@example.com"))
            .bind(role)
            .execute(&pool)
            .await
            .expect("insert fixture user");
    }

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
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);
    assert_eq!(json_body(began).await["status"], "discussing");

    let accept_uri = format!("/requirements/{requirement_id}/accept");
    let denied = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::POST,
        &accept_uri,
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let edit_uri = format!("/requirements/{requirement_id}");
    let edited = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({"expected_revision":1,"summary":"first"}),
    )
    .await;
    assert_eq!(edited.status(), StatusCode::OK);
    assert_eq!(json_body(edited).await["revision"], 2);

    let stale = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({"expected_revision":1,"summary":"stale"}),
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

    let criterion = format!("criterion-{requirement_id}");
    sqlx::query(
        "UPDATE requirements
         SET status = 'Ready', acceptance_criteria = ARRAY[$2]::TEXT[]
         WHERE id = $1",
    )
    .bind(&requirement_id)
    .bind(&criterion)
    .execute(&pool)
    .await
    .expect("make ready fixture");
    let criteria_search = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::GET,
        &format!("/requirements?search={criterion}"),
        json!({}),
    )
    .await;
    assert_eq!(criteria_search.status(), StatusCode::OK);
    assert_eq!(
        json_body(criteria_search).await.as_array().map(Vec::len),
        Some(1)
    );
    let demoted = request(
        app(pool.clone(), &requester_id, Role::Requester),
        Method::PATCH,
        &edit_uri,
        json!({"expected_revision":2,"summary":"second"}),
    )
    .await;
    assert_eq!(demoted.status(), StatusCode::OK);
    let demoted = json_body(demoted).await;
    assert_eq!(demoted["status"], "discussing");
    assert_eq!(demoted["revision"], 3);

    sqlx::query("UPDATE requirements SET status = 'Ready' WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("restore ready fixture");
    let accepted = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &accept_uri,
        json!({"expected_revision":3}),
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(json_body(accepted).await["status"], "accepted");

    let terminal = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::PATCH,
        &edit_uri,
        json!({"expected_revision":3,"summary":"forbidden"}),
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

    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("cleanup requirement");
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(&requester_id)
        .bind(&manager_id)
        .execute(&pool)
        .await
        .expect("cleanup users");
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn transition_edges_are_illegal_revision_guarded_and_audited() {
    let database_url = match std::env::var("NORTH_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => panic!("NORTH_TEST_DATABASE_URL is required for requirement integration tests"),
    };
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let requester_id = unique("lifecycle-requester");
    let manager_id = unique("lifecycle-manager");
    for (id, role) in [
        (&requester_id, "Requester"),
        (&manager_id, "RequirementManager"),
    ] {
        sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(format!("{id}@example.com"))
            .bind(role)
            .execute(&pool)
            .await
            .expect("insert lifecycle fixture user");
    }

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
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(began.status(), StatusCode::OK);

    let reject_uri = format!("/requirements/{requirement_id}/reject");
    let illegal = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &reject_uri,
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(illegal.status(), StatusCode::BAD_REQUEST);
    let stale = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &begin_uri,
        json!({"expected_revision":0}),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let early_audits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transition_audit WHERE requirement_id = $1")
            .bind(&requirement_id)
            .fetch_one(&pool)
            .await
            .expect("count early audits");
    assert_eq!(early_audits, 1);

    sqlx::query(
        "UPDATE requirements
         SET status = 'Ready', acceptance_criteria = ARRAY['criterion']::TEXT[]
         WHERE id = $1",
    )
    .bind(&requirement_id)
    .execute(&pool)
    .await
    .expect("make lifecycle fixture ready");
    let rejected = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &reject_uri,
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::OK);
    assert_eq!(json_body(rejected).await["status"], "rejected");

    let reopen_uri = format!("/requirements/{requirement_id}/reopen");
    let reopened = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &reopen_uri,
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(reopened.status(), StatusCode::OK);
    assert_eq!(json_body(reopened).await["status"], "discussing");

    sqlx::query("UPDATE requirements SET status = 'Ready' WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("restore lifecycle fixture ready");
    let request_changes_uri = format!("/requirements/{requirement_id}/request-changes");
    let changes = request(
        app(pool.clone(), &manager_id, Role::RequirementManager),
        Method::POST,
        &request_changes_uri,
        json!({"expected_revision":1,"feedback":"Clarify account scope"}),
    )
    .await;
    assert_eq!(changes.status(), StatusCode::OK);
    assert_eq!(json_body(changes).await["status"], "discussing");

    let audits: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT transition, from_status, to_status, feedback
         FROM transition_audit WHERE requirement_id = $1 ORDER BY id ASC",
    )
    .bind(&requirement_id)
    .fetch_all(&pool)
    .await
    .expect("read transition audits");
    assert_eq!(audits.len(), 4);
    assert_eq!(audits[0].0, "begin_discussion");
    assert_eq!(audits[1].0, "reject");
    assert_eq!(audits[2].0, "reopen");
    assert_eq!(audits[3].0, "request_changes");
    assert_eq!(audits[3].3.as_deref(), Some("Clarify account scope"));

    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&requirement_id)
        .execute(&pool)
        .await
        .expect("cleanup lifecycle requirement");
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(&requester_id)
        .bind(&manager_id)
        .execute(&pool)
        .await
        .expect("cleanup lifecycle users");
}
