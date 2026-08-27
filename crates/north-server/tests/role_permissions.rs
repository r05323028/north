use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Extension,
};
use north_domain::role::Role;
use north_persistence::{PoolOptions, UserRecord};
use north_server::{
    authorize_role_assignment, require_admin, require_review, AuthState, CurrentUser, RoleHttpError,
};
use tower::ServiceExt;

fn user(id: &str, role: Role) -> CurrentUser {
    CurrentUser(UserRecord {
        id: id.into(),
        email: format!("{id}@example.com"),
        role,
    })
}

fn role_router(user: CurrentUser) -> axum::Router {
    let pool = PoolOptions::new()
        .connect_lazy("postgres://north:north@127.0.0.1:1/north")
        .expect("valid lazy pool URL");
    north_server::roles::router()
        .with_state(AuthState::with_log_delivery(
            north_persistence::AuthStore::new(pool),
        ))
        .layer(Extension(user))
}

#[test]
fn requester_policy_is_blocked_from_review_and_administration() {
    let requester = user("requester", Role::Requester);
    assert_eq!(
        require_review(&requester),
        Err(RoleHttpError::PermissionDenied)
    );
    assert_eq!(
        require_admin(&requester),
        Err(RoleHttpError::PermissionDenied)
    );
}

#[test]
fn requester_http_requests_are_refused_at_users_boundary() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let response = role_router(user("requester", Role::Requester))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/users")
                    .body(Body::empty())
                    .expect("users request"),
            )
            .await
            .expect("users response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = role_router(user("requester", Role::Requester))
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/users/target/role")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"role":"Admin"}"#))
                    .expect("role request"),
            )
            .await
            .expect("role response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = role_router(user("admin", Role::Admin))
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/users/admin/role")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"role":"Requester"}"#))
                    .expect("self role request"),
            )
            .await
            .expect("self role response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = role_router(user("admin", Role::Admin))
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/users/owner/role")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"role":"Owner"}"#))
                    .expect("owner role request"),
            )
            .await
            .expect("owner role response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    });
}

#[test]
fn assignment_boundary_preserves_escalation_rules() {
    let admin = user("admin", Role::Admin);
    let owner = user("owner", Role::Owner);

    assert_eq!(
        authorize_role_assignment(&admin, "admin", Role::Requester),
        Err(RoleHttpError::SelfModification)
    );
    assert_eq!(
        authorize_role_assignment(&admin, "owner", Role::Owner),
        Err(RoleHttpError::OwnerGrantRequiresOwner)
    );
    for (index, role) in [
        Role::Owner,
        Role::Admin,
        Role::RequirementManager,
        Role::Requester,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(authorize_role_assignment(&owner, &format!("target-{index}"), role).is_ok());
    }
}
