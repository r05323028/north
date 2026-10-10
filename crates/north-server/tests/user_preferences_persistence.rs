use axum::{
    body::{to_bytes, Body},
    http::{Method, Request},
    response::Response,
    Router,
};
use north_domain::status::RequirementStatus;
use north_persistence::{
    AuthStore, PersistenceError, RequirementSort, RequirementWorkspacePreferences,
    RequirementWorkspaceView,
};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

#[allow(dead_code)]
mod support;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

async fn setup_http_user(
    pool: &north_persistence::DatabaseConnection,
    prefix: &str,
    role: &str,
) -> String {
    let id = unique(prefix);
    let email = format!("{id}@example.test");
    support::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
        .bind(&id)
        .bind(&email)
        .bind(role)
        .execute(pool)
        .await
        .expect("insert HTTP fixture user");
    email
}

async fn issue_session(store: &AuthStore, email: &str) -> String {
    store
        .issue_code(email, "123456")
        .await
        .expect("issue fixture verification code");
    store
        .verify_code(email, "123456")
        .await
        .expect("verify fixture code")
        .token
}

async fn http_request(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("cookie", format!("north_session={token}"));
    }
    app.clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("build HTTP request"),
        )
        .await
        .expect("send HTTP request")
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body"),
    )
    .expect("parse response JSON")
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn requirement_workspace_preferences_default_upsert_and_validation() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for preference persistence tests");
    let pool = support::TestDatabaseOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let user_id = unique("preferences-user");
    support::query("INSERT INTO users (id, email) VALUES ($1, $2)")
        .bind(&user_id)
        .bind(format!("{user_id}@example.test"))
        .execute(&pool)
        .await
        .expect("insert fixture user");

    let store = AuthStore::new(pool.clone(), support::test_otp_key());
    assert_eq!(
        store
            .get_requirement_workspace_preferences(&user_id)
            .await
            .expect("load missing preference defaults"),
        RequirementWorkspacePreferences::default()
    );

    let saved = RequirementWorkspacePreferences {
        default_view: RequirementWorkspaceView::List,
        list_status: Some(RequirementStatus::Draft),
        list_created_by: Some("creator-1".into()),
        list_sort: RequirementSort::UpdatedAscending,
    };
    store
        .save_requirement_workspace_preferences(&user_id, &saved)
        .await
        .expect("save preferences");
    assert_eq!(
        store
            .get_requirement_workspace_preferences(&user_id)
            .await
            .expect("read saved preferences"),
        saved
    );

    let replacement = RequirementWorkspacePreferences {
        default_view: RequirementWorkspaceView::Board,
        list_status: None,
        list_created_by: None,
        list_sort: RequirementSort::UpdatedDescending,
    };
    store
        .save_requirement_workspace_preferences(&user_id, &replacement)
        .await
        .expect("replace preferences");
    assert_eq!(
        store
            .get_requirement_workspace_preferences(&user_id)
            .await
            .expect("read replaced preferences"),
        replacement
    );

    let invalid_sort = RequirementWorkspacePreferences {
        list_sort: RequirementSort::Board,
        ..replacement.clone()
    };
    assert!(matches!(
        store
            .save_requirement_workspace_preferences(&user_id, &invalid_sort)
            .await,
        Err(PersistenceError::InvalidUserPreferences)
    ));

    for statement in [
        "UPDATE user_requirement_preferences SET default_view = 'cards' WHERE user_id = $1",
        "UPDATE user_requirement_preferences SET list_status = 'pending' WHERE user_id = $1",
        "UPDATE user_requirement_preferences SET list_sort = 'name' WHERE user_id = $1",
    ] {
        assert!(
            support::query(statement)
                .bind(&user_id)
                .execute(&pool)
                .await
                .is_err(),
            "database accepted invalid preference value: {statement}"
        );
    }
    assert_eq!(
        store
            .get_requirement_workspace_preferences(&user_id)
            .await
            .expect("failed updates preserve existing preferences"),
        replacement
    );

    support::query("DELETE FROM users WHERE id = $1")
        .bind(&user_id)
        .execute(&pool)
        .await
        .expect("remove fixture user and owned preferences");
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn requirement_workspace_preferences_http_auth_validation_and_isolation() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for preference HTTP tests");
    let pool = support::TestDatabaseOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");

    let owner_email = setup_http_user(&pool, "preferences-owner", "Owner").await;
    let admin_email = setup_http_user(&pool, "preferences-admin", "Admin").await;
    let requester_email = setup_http_user(&pool, "preferences-requester", "Requester").await;
    let store = AuthStore::new(pool, support::test_otp_key());
    let owner_token = issue_session(&store, &owner_email).await;
    let admin_token = issue_session(&store, &admin_email).await;
    let requester_token = issue_session(&store, &requester_email).await;
    let app = north_server::auth_router(north_server::AuthState::with_log_delivery(store));
    let path = "/auth/me/preferences/requirement-workspace";

    for method in [axum::http::Method::GET, axum::http::Method::PUT] {
        let response = http_request(&app, method, path, None, json!({})).await;
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    let defaults = json!({
        "default_view": "board",
        "list_status": null,
        "list_created_by": null,
        "list_sort": "updated"
    });
    let response = http_request(
        &app,
        axum::http::Method::GET,
        path,
        Some(&requester_token),
        json!({}),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(json_body(response).await, defaults);

    let saved = json!({
        "default_view": "list",
        "list_status": "draft",
        "list_created_by": "creator-1",
        "list_sort": "updated_asc"
    });
    let response = http_request(
        &app,
        axum::http::Method::PUT,
        path,
        Some(&requester_token),
        json!({
            "default_view": "list",
            "list_status": "draft",
            "list_created_by": " creator-1 ",
            "list_sort": "updated_asc"
        }),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(json_body(response).await, saved);

    for token in [&admin_token, &owner_token] {
        let response =
            http_request(&app, axum::http::Method::GET, path, Some(token), json!({})).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(json_body(response).await, defaults);
    }

    let admin_preferences = json!({
        "default_view": "board",
        "list_status": "ready",
        "list_created_by": "admin-only",
        "list_sort": "updated"
    });
    let owner_preferences = json!({
        "default_view": "list",
        "list_status": "accepted",
        "list_created_by": null,
        "list_sort": "updated_asc"
    });
    for (token, expected) in [
        (&admin_token, &admin_preferences),
        (&owner_token, &owner_preferences),
    ] {
        let response = http_request(
            &app,
            axum::http::Method::PUT,
            path,
            Some(token),
            expected.clone(),
        )
        .await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(json_body(response).await, *expected);
    }

    let target_path = format!("{path}/{}", requester_email);
    let response = http_request(
        &app,
        axum::http::Method::GET,
        &target_path,
        Some(&admin_token),
        json!({}),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

    let response = http_request(
        &app,
        axum::http::Method::PUT,
        path,
        Some(&owner_token),
        json!({
            "user_id": requester_email,
            "default_view": "board",
            "list_status": null,
            "list_created_by": null,
            "list_sort": "updated"
        }),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let response = http_request(
        &app,
        axum::http::Method::GET,
        path,
        Some(&owner_token),
        json!({}),
    )
    .await;
    assert_eq!(json_body(response).await, owner_preferences);

    for (case, invalid) in [
        (
            "invalid status",
            json!({
                "default_view": "board",
                "list_status": "unknown",
                "list_created_by": "attacker",
                "list_sort": "updated"
            }),
        ),
        (
            "missing nullable fields",
            json!({"default_view":"board", "list_sort":"updated"}),
        ),
    ] {
        let response = http_request(
            &app,
            axum::http::Method::PUT,
            path,
            Some(&requester_token),
            invalid,
        )
        .await;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::BAD_REQUEST,
            "accepted {case}"
        );
    }

    let response = http_request(
        &app,
        axum::http::Method::GET,
        path,
        Some(&requester_token),
        json!({}),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(json_body(response).await, saved);
}
