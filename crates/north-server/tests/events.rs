use axum::{
    body::Body,
    extract::Extension,
    http::{Method, Request, StatusCode},
    Router,
};
use north_domain::{requirement::RequirementEdit, role::Role, status::RequirementStatus};
use north_persistence::{AuthStore, PoolOptions, RequirementTransition, UserRecord};
use north_protocol::{
    Event, EventAckStatus, EventEnvelope, ReadinessVerdictWire, RequirementAssessed, SCHEMA_VERSION,
};
use north_server::{
    assessment::handle_requirement_assessed_with_events, requirements, AuthState, CurrentUser,
};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{sync::broadcast, time::timeout};
use tower::ServiceExt;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{prefix}-{nanos}")
}

async fn test_pool() -> Result<north_persistence::PgPool, Box<dyn std::error::Error>> {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")?;
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await?;
    north_persistence::run_migrations(&pool).await?;
    Ok(pool)
}

async fn setup_user(
    pool: &north_persistence::PgPool,
    prefix: &str,
) -> Result<UserRecord, Box<dyn std::error::Error>> {
    let id = unique(prefix);
    let email = format!("{id}@example.com");
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, $3)")
        .bind(&id)
        .bind(&email)
        .bind("Requester")
        .execute(pool)
        .await?;
    Ok(UserRecord {
        id,
        email,
        role: Role::Requester,
    })
}

fn requirements_app(state: AuthState, user: UserRecord) -> Router {
    requirements::router()
        .with_state(state)
        .layer(Extension(CurrentUser(user)))
}

async fn request(
    app: Router,
    method: Method,
    uri: &str,
    body: serde_json::Value,
) -> Result<axum::response::Response, Box<dyn std::error::Error>> {
    Ok(app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))?,
        )
        .await?)
}

async fn next_notification(
    receiver: &mut broadcast::Receiver<north_server::events::BrowserNotification>,
) -> Result<north_server::events::BrowserNotification, Box<dyn std::error::Error>> {
    Ok(timeout(Duration::from_secs(1), receiver.recv()).await??)
}

fn assert_no_notification(
    receiver: &mut broadcast::Receiver<north_server::events::BrowserNotification>,
) {
    match receiver.try_recv() {
        Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {}
        result => panic!("unexpected browser notification: {result:?}"),
    }
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn committed_requirement_creation_publishes_one_lightweight_hint(
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = test_pool().await?;
    let user = setup_user(&pool, "event-create-user").await?;
    let state = AuthState::with_log_delivery(AuthStore::new(pool));
    let mut receiver = state.events().subscribe();
    let app = requirements_app(state, user);
    let response = request(
        app,
        Method::POST,
        "/requirements",
        json!({
            "title": "Event requirement",
            "description": "A committed requirement"
        }),
    )
    .await?;

    assert_eq!(response.status(), StatusCode::CREATED);
    let notification = next_notification(&mut receiver).await?;
    assert_eq!(notification.category, "requirement.changed");
    assert!(!notification.requirement_id.is_empty());
    assert_no_notification(&mut receiver);
    Ok(())
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn real_requirement_edit_publishes_once_and_noop_is_silent(
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = test_pool().await?;
    let user = setup_user(&pool, "event-edit-user").await?;
    let store = AuthStore::new(pool.clone());
    let requirement = store
        .create_requirement("Edit requirement", "Edit coverage", &user.id)
        .await?;
    let state = AuthState::with_log_delivery(store);
    let mut receiver = state.events().subscribe();
    let uri = format!("/requirements/{}", requirement.id);

    let edited = request(
        requirements_app(state.clone(), user.clone()),
        Method::PATCH,
        &uri,
        json!({"expected_state_version":1,"summary":"Changed"}),
    )
    .await?;
    assert_eq!(edited.status(), StatusCode::OK);
    let notification = next_notification(&mut receiver).await?;
    assert_eq!(notification.requirement_id, requirement.id);
    assert_no_notification(&mut receiver);

    let noop = request(
        requirements_app(state.clone(), user),
        Method::PATCH,
        &uri,
        json!({"expected_state_version":2,"summary":"Changed"}),
    )
    .await?;
    assert_eq!(noop.status(), StatusCode::OK);
    assert_no_notification(&mut receiver);
    Ok(())
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn committed_lifecycle_transition_publishes_once() -> Result<(), Box<dyn std::error::Error>> {
    let pool = test_pool().await?;
    let user = setup_user(&pool, "event-lifecycle-user").await?;
    let store = AuthStore::new(pool.clone());
    let requirement = store
        .create_requirement("Lifecycle requirement", "Transition coverage", &user.id)
        .await?;
    let state = AuthState::with_log_delivery(store);
    let mut receiver = state.events().subscribe();
    let uri = format!("/requirements/{}/begin-discussion", requirement.id);

    let transitioned = request(
        requirements_app(state, user),
        Method::POST,
        &uri,
        json!({"expected_state_version":1}),
    )
    .await?;
    assert_eq!(transitioned.status(), StatusCode::OK);
    let notification = next_notification(&mut receiver).await?;
    assert_eq!(notification.requirement_id, requirement.id);
    assert_no_notification(&mut receiver);
    Ok(())
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn accepted_readiness_publishes_once_and_duplicate_or_rejected_events_are_silent(
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = test_pool().await?;
    let user = setup_user(&pool, "event-readiness-user").await?;
    let store = AuthStore::new(pool.clone());
    let requirement = store
        .create_requirement("Readiness requirement", "Readiness coverage", &user.id)
        .await?;
    store
        .transition_requirement_with_feedback(
            &requirement.id,
            1,
            &user.id,
            RequirementTransition::BeginDiscussion,
            None,
            None,
        )
        .await?;
    store
        .edit_requirement_with_actor(
            &requirement.id,
            2,
            &user.id,
            &RequirementEdit {
                title: None,
                description: None,
                summary: None,
                acceptance_criteria: Some(vec!["The requirement is testable".into()]),
                assumptions: None,
                open_questions: None,
            },
        )
        .await?;
    let session_id = unique("event-readiness-session");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&session_id)
        .bind(&requirement.id)
        .execute(&pool)
        .await?;

    let state = AuthState::with_log_delivery(store);
    let mut receiver = state.events().subscribe();
    let assessment = RequirementAssessed {
        requirement_id: requirement.id.clone(),
        requirement_revision: 2,
        verdict: ReadinessVerdictWire::Ready,
        blockers: Vec::new(),
        assumptions: vec!["Current evidence".into()],
        repositories_reviewed: Vec::new(),
    };
    let envelope = EventEnvelope {
        event_id: unique("event-readiness"),
        session_id: session_id.clone(),
        daemon_event_seq: 1,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::RequirementAssessed(assessment.clone()),
    };

    let accepted =
        handle_requirement_assessed_with_events(state.store(), &envelope, state.events()).await?;
    assert_eq!(accepted.status, EventAckStatus::Accepted);
    let notification = next_notification(&mut receiver).await?;
    assert_eq!(notification.requirement_id, requirement.id);
    assert_no_notification(&mut receiver);

    let duplicate =
        handle_requirement_assessed_with_events(state.store(), &envelope, state.events()).await?;
    assert_eq!(duplicate.status, EventAckStatus::Accepted);
    assert_no_notification(&mut receiver);

    let rejected_envelope = EventEnvelope {
        event_id: unique("event-rejected-readiness"),
        session_id: session_id.clone(),
        daemon_event_seq: 2,
        sent_at: "2026-01-01T00:00:01Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::RequirementAssessed(assessment),
    };
    let rejected =
        handle_requirement_assessed_with_events(state.store(), &rejected_envelope, state.events())
            .await?;
    assert_eq!(rejected.status, EventAckStatus::Rejected);
    assert_no_notification(&mut receiver);

    let current = state
        .store()
        .requirement_by_id(&requirement.id)
        .await?
        .ok_or_else(|| std::io::Error::other("readiness requirement"))?;
    assert_eq!(current.status, RequirementStatus::Ready);
    assert_eq!(current.state_version, 4);
    let assessment_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments WHERE event_id = $1")
            .bind(&envelope.event_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(assessment_count, 1);
    let ready_transition_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transition_audit
         WHERE requirement_id = $1 AND transition = 'mark_ready'",
    )
    .bind(&requirement.id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(ready_transition_count, 1);
    Ok(())
}
