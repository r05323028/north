use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Extension, Router,
};
use north_domain::role::Role;
use north_persistence::{AuthStore, PoolOptions, RequirementTransition};
use north_protocol::{ReadinessVerdictWire, RequirementAssessed, ReviewedRepositoryWire};
use north_server::{
    assessment::process_requirement_assessed, repositories, AuthState, CurrentUser,
};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn app(pool: north_persistence::PgPool, id: &str, role: Role) -> Router {
    repositories::router()
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
async fn repository_management_preserves_identity_and_lifecycle() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for repository integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let owner = unique("repository-owner");
    let requester = unique("repository-requester");
    let manager = unique("repository-manager");
    let name = unique("North Repository");
    let url = "https://example.test/north.git";

    let invalid = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        "/repositories",
        json!({"name": "invalid", "url": "https://token@example.test/repo.git"}),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let created = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        "/repositories",
        json!({"name": format!("  {name}  "), "url": format!(" {url} "), "description":" desc "}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    let repository_id = created["id"].as_str().expect("repository ID").to_owned();
    assert_eq!(created["name"], name);
    assert_eq!(created["url"], url);
    assert!(created["created_at"].as_str().is_some());
    assert_eq!(created["created_at"], created["updated_at"]);
    assert!(created["disabled_at"].is_null());
    assert_eq!(created_id_version(&repository_id), Some(4));

    let denied_list = request(
        app(pool.clone(), &requester, Role::Requester),
        Method::GET,
        "/repositories",
        json!({}),
    )
    .await;
    assert_eq!(denied_list.status(), StatusCode::FORBIDDEN);
    let denied_create = request(
        app(pool.clone(), &manager, Role::RequirementManager),
        Method::POST,
        "/repositories",
        json!({"name":"not allowed", "url":url}),
    )
    .await;
    assert_eq!(denied_create.status(), StatusCode::FORBIDDEN);

    tokio::time::sleep(Duration::from_millis(2)).await;
    let updated = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::PATCH,
        &format!("/repositories/{repository_id}"),
        json!({"name":format!("{name} renamed"), "description":"updated", "url":url}),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json_body(updated).await;
    assert_eq!(updated["id"], repository_id);
    assert_eq!(updated["url"], url);
    assert_ne!(updated["created_at"], Value::Null);
    assert_ne!(updated["updated_at"], created["updated_at"]);

    let url_change = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::PATCH,
        &format!("/repositories/{repository_id}"),
        json!({"url":"https://other.example/repo.git"}),
    )
    .await;
    assert_eq!(url_change.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(url_change).await["action"],
        "disable_old_create_new"
    );

    let disabled = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        &format!("/repositories/{repository_id}/disable"),
        json!({}),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::OK);
    let disabled = json_body(disabled).await;
    let disabled_at = disabled["disabled_at"].clone();
    let disabled_updated_at = disabled["updated_at"].clone();
    assert!(!disabled["enabled"].as_bool().expect("enabled flag"));

    let repeated_disable = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        &format!("/repositories/{repository_id}/disable"),
        json!({}),
    )
    .await;
    assert_eq!(repeated_disable.status(), StatusCode::OK);
    let repeated_disable = json_body(repeated_disable).await;
    assert_eq!(repeated_disable["disabled_at"], disabled_at);
    assert_eq!(repeated_disable["updated_at"], disabled_updated_at);

    let duplicate = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        "/repositories",
        json!({"name":updated["name"], "url":url}),
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    let duplicate = json_body(duplicate).await;
    assert_eq!(duplicate["repository_id"], repository_id);
    assert_eq!(duplicate["action"], "re_enable");

    let active = AuthStore::new(pool.clone())
        .active_repositories()
        .await
        .expect("active catalog")
        .into_iter()
        .all(|repository| repository.id != repository_id);
    assert!(active);

    let reenabled = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        &format!("/repositories/{repository_id}/re-enable"),
        json!({}),
    )
    .await;
    assert_eq!(reenabled.status(), StatusCode::OK);
    let reenabled = json_body(reenabled).await;
    assert_eq!(reenabled["id"], repository_id);
    assert!(reenabled["disabled_at"].is_null());
    assert_ne!(reenabled["updated_at"], disabled_updated_at);
    let repeated_reenable = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::POST,
        &format!("/repositories/{repository_id}/re-enable"),
        json!({}),
    )
    .await;
    assert_eq!(repeated_reenable.status(), StatusCode::OK);
    assert_eq!(
        json_body(repeated_reenable).await["updated_at"],
        reenabled["updated_at"]
    );

    let no_delete = request(
        app(pool.clone(), &owner, Role::Owner),
        Method::DELETE,
        &format!("/repositories/{repository_id}"),
        json!({}),
    )
    .await;
    assert!(!no_delete.status().is_success());
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn repository_citations_require_identity_but_survive_disable() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for repository integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let user_id = unique("citation-user");
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, 'Requester')")
        .bind(&user_id)
        .bind(format!("{user_id}@example.com"))
        .execute(&pool)
        .await
        .expect("insert user");
    let store = AuthStore::new(pool.clone());
    let repository = store
        .create_repository(
            "Citation Repository",
            "https://example.test/citation.git",
            "",
        )
        .await
        .expect("create repository");
    let requirement = store
        .create_requirement(
            "Citation requirement",
            "A requirement with source evidence",
            &user_id,
        )
        .await
        .expect("create requirement");
    let requirement = store
        .transition_requirement(
            &requirement.id,
            1,
            &user_id,
            RequirementTransition::BeginDiscussion,
        )
        .await
        .expect("begin discussion");
    let requirement = store
        .edit_requirement(
            &requirement.id,
            requirement.state_version,
            &north_domain::requirement::RequirementEdit {
                acceptance_criteria: Some(vec!["Evidence is exact".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("add criteria");
    let unknown_session = unique("unknown-citation-session");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&unknown_session)
        .bind(&requirement.id)
        .execute(&pool)
        .await
        .expect("bind unknown citation session");
    let unknown = process_requirement_assessed(
        &store,
        &unique("unknown-citation-event"),
        &unknown_session,
        1,
        &RequirementAssessed {
            requirement_id: requirement.id.clone(),
            requirement_revision: requirement.revision,
            verdict: ReadinessVerdictWire::Ready,
            blockers: Vec::new(),
            assumptions: vec!["Unknown citation is rejected".into()],
            repositories_reviewed: vec![ReviewedRepositoryWire {
                repository_id: "missing-repository".into(),
                commit_sha: "abc123".into(),
            }],
        },
    )
    .await
    .expect("unknown citation rejection");
    assert_eq!(unknown.status, north_protocol::EventAckStatus::Rejected);
    assert_eq!(unknown.reason.as_deref(), Some("unknown_repository"));

    let outside_session = unique("outside-citation-session");
    sqlx::query("INSERT INTO execution_sessions (id, requirement_id) VALUES ($1, $2)")
        .bind(&outside_session)
        .bind(&requirement.id)
        .execute(&pool)
        .await
        .expect("bind outside citation session");
    let outside = process_requirement_assessed(
        &store,
        &unique("outside-citation-event"),
        &outside_session,
        1,
        &RequirementAssessed {
            requirement_id: requirement.id.clone(),
            requirement_revision: requirement.revision,
            verdict: ReadinessVerdictWire::Ready,
            blockers: Vec::new(),
            assumptions: vec!["Context membership is required".into()],
            repositories_reviewed: vec![ReviewedRepositoryWire {
                repository_id: repository.id.clone(),
                commit_sha: "abc123".into(),
            }],
        },
    )
    .await
    .expect("outside context rejection");
    assert_eq!(outside.status, north_protocol::EventAckStatus::Rejected);
    assert_eq!(outside.reason.as_deref(), Some("unknown_repository"));

    let valid_session = unique("valid-citation-session");
    sqlx::query(
        "INSERT INTO execution_sessions (id, requirement_id, repository_ids)
         VALUES ($1, $2, $3)",
    )
    .bind(&valid_session)
    .bind(&requirement.id)
    .bind(vec![repository.id.clone()])
    .execute(&pool)
    .await
    .expect("bind valid citation session");
    let valid = process_requirement_assessed(
        &store,
        &unique("valid-citation-event"),
        &valid_session,
        1,
        &RequirementAssessed {
            requirement_id: requirement.id.clone(),
            requirement_revision: requirement.revision,
            verdict: ReadinessVerdictWire::Ready,
            blockers: Vec::new(),
            assumptions: vec!["Retained citation".into()],
            repositories_reviewed: vec![ReviewedRepositoryWire {
                repository_id: repository.id.clone(),
                commit_sha: "abcdef0123456789".into(),
            }],
        },
    )
    .await
    .expect("valid citation");
    assert_eq!(valid.status, north_protocol::EventAckStatus::Accepted);
    store
        .disable_repository(&repository.id)
        .await
        .expect("disable after evidence");
    let retained = store
        .repository_by_id(&repository.id)
        .await
        .expect("read retained repository")
        .expect("retained repository");
    assert!(!retained.enabled());
    let historical: (String, String) = sqlx::query_as(
        "SELECT repositories_reviewed->0->>\'repository_id\', repositories_reviewed->0->>\'commit_sha\'\n         FROM readiness_assessments WHERE session_id = $1",
    )
    .bind(&valid_session)
    .fetch_one(&pool)
    .await
    .expect("read historical citation");
    assert_eq!(historical.0, repository.id);
    assert_eq!(historical.1, "abcdef0123456789");
}

fn created_id_version(id: &str) -> Option<u8> {
    let bytes = id.as_bytes();
    (bytes.len() == 36 && bytes.get(14) == Some(&b'4')).then_some(4)
}
