use axum::{
    body::Body,
    extract::Extension,
    http::{Method, Request, StatusCode},
    Router,
};
use north_domain::role::Role;
use north_persistence::{AuthStore, PoolOptions, UserRecord};
use north_server::{requirements, AuthState, CurrentUser};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tower::ServiceExt;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{prefix}-{nanos}")
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn committed_requirement_creation_publishes_one_lightweight_hint(
) -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")?;
    let pool = PoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    north_persistence::run_migrations(&pool).await?;

    let user_id = unique("event-user");
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
        .bind(&user_id)
        .bind(format!("{user_id}@example.com"))
        .bind("Requester")
        .execute(&pool)
        .await?;

    let state = AuthState::with_log_delivery(AuthStore::new(pool));
    let mut receiver = state.events().subscribe();
    let app: Router = requirements::router()
        .with_state(state)
        .layer(Extension(CurrentUser(UserRecord {
            id: user_id,
            email: "event@example.com".into(),
            role: Role::Requester,
        })));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/requirements")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Event requirement",
                        "description": "A committed requirement"
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::CREATED);
    let notification = timeout(Duration::from_secs(1), receiver.recv()).await??;
    assert_eq!(notification.category, "requirement.changed");
    assert!(!notification.requirement_id.is_empty());
    Ok(())
}
