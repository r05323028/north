#[allow(dead_code)]
mod support;
use support::TestDatabaseOptions;

use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Method, Request, StatusCode},
};
use futures_util::future::join_all;
use north_persistence::{AuthStore, PersistenceError};
use north_server::{
    auth_router, AuthState, CodeDelivery, DeliveryError, LogCodeDelivery, PublicEndpointConfig,
};
use serde_json::Value;
use std::{
    env,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex as StdMutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower::ServiceExt;

static DATABASE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static KEY_COUNTER: AtomicU64 = AtomicU64::new(1);

async fn database() -> (
    north_persistence::DatabaseConnection,
    tokio::sync::MutexGuard<'static, ()>,
) {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for public endpoint tests");
    let guard = DATABASE_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    let pool = TestDatabaseOptions::new()
        .max_connections(16)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    (pool, guard)
}

fn test_otp_key() -> north_persistence::OtpKey {
    north_persistence::OtpKey::from_hex(
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    )
    .expect("valid test OTP key")
}

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn unique_email(prefix: &str) -> String {
    format!("{}@example.com", unique(prefix))
}

fn unique_peer() -> String {
    let suffix = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos() as u64)
        ^ KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "[2001:db8:{:x}:{:x}::1]:443",
        (suffix >> 16) & 0xffff,
        suffix & 0xffff
    )
}

fn unique_key() -> String {
    let suffix = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos() as u64)
        ^ KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "2001:db8:{:x}:{:x}::/64",
        (suffix >> 16) & 0xffff,
        suffix & 0xffff
    )
}

fn request(peer: &str, method: Method, uri: &str, body: &str) -> Request<Body> {
    request_with_content_type(peer, method, uri, body, Some("application/json"))
}

fn request_with_content_type(
    peer: &str,
    method: Method,
    uri: &str,
    body: &str,
    content_type: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let mut request = builder.body(Body::from(body.to_owned())).expect("request");
    request.extensions_mut().insert(ConnectInfo(
        peer.parse::<SocketAddr>().expect("peer socket address"),
    ));
    request
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn setup_quota_is_keyed_and_concurrency_safe() {
    let (pool, _database_test_guard) = database().await;
    let store = AuthStore::new(pool.clone(), test_otp_key());
    let key = unique_key();
    let label_prefix = unique("setup-label");
    let labels: Vec<_> = (0..8)
        .map(|index| format!("{label_prefix}-{index}"))
        .collect();
    let results = join_all(labels.iter().map(|label| {
        let store = store.clone();
        let label = label.clone();
        let key = key.clone();
        tokio::spawn(async move { store.create_daemon_setup_request(&label, &key).await })
    }))
    .await;
    let successes: Vec<_> = results
        .into_iter()
        .filter_map(|result| result.ok()?.ok())
        .collect();
    assert_eq!(
        successes.len(),
        3,
        "pending quota must serialize count-and-insert"
    );
    support::query(
        "UPDATE daemon_setup_requests
         SET approved_at = CURRENT_TIMESTAMP
         WHERE label = $1",
    )
    .bind(&successes[2].label)
    .execute(&pool)
    .await
    .expect("mark setup approved");

    let before_rejection: i64 = support::query_scalar(
        "SELECT COUNT(*) FROM daemon_setup_requests
         WHERE client_network_key = $1::cidr
           AND claimed_at IS NULL
           AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(&key)
    .fetch_one(&pool)
    .await
    .expect("count pending setup rows");
    assert_eq!(before_rejection, 3);
    assert!(matches!(
        store
            .create_daemon_setup_request(&unique("different-label"), &key)
            .await,
        Err(PersistenceError::RateLimited)
    ));
    let after_rejection: i64 = support::query_scalar(
        "SELECT COUNT(*) FROM daemon_setup_requests
         WHERE client_network_key = $1::cidr
           AND claimed_at IS NULL
           AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(&key)
    .fetch_one(&pool)
    .await
    .expect("recount pending setup rows");
    assert_eq!(after_rejection, before_rejection);

    let other_key = unique_key();
    store
        .create_daemon_setup_request(&unique("other-network"), &other_key)
        .await
        .expect("different CIDR must have an independent quota");

    support::query(
        "UPDATE daemon_setup_requests
         SET claimed_at = CURRENT_TIMESTAMP
         WHERE label = $1",
    )
    .bind(&successes[0].label)
    .execute(&pool)
    .await
    .expect("mark setup claimed");
    store
        .create_daemon_setup_request(&unique("after-claim"), &key)
        .await
        .expect("claimed setup must not count");

    support::query(
        "UPDATE daemon_setup_requests
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
         WHERE label = $1",
    )
    .bind(&successes[1].label)
    .execute(&pool)
    .await
    .expect("expire setup row");
    store
        .create_daemon_setup_request(&unique("after-expiry"), &key)
        .await
        .expect("expired setup must not count");

    let non_null_keys: i64 = support::query_scalar(
        "SELECT COUNT(*) FROM daemon_setup_requests
         WHERE client_network_key IS NOT NULL
           AND label LIKE $1",
    )
    .bind(format!("{label_prefix}-%"))
    .fetch_one(&pool)
    .await
    .expect("count keyed setup rows");
    assert_eq!(non_null_keys, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn public_http_429_is_generic_and_endpoint_buckets_are_isolated() {
    let (pool, _database_test_guard) = database().await;
    let peer = unique_peer();
    let config = PublicEndpointConfig {
        bucket_capacity: 1,
        bucket_refill_interval: Duration::from_secs(120),
        ..PublicEndpointConfig::default()
    };
    let app = auth_router(AuthState::with_public_endpoint_config(
        AuthStore::new(pool.clone(), test_otp_key()),
        Arc::new(LogCodeDelivery),
        config,
    ));
    let first_email = unique_email("client-limit-first");
    let response = app
        .clone()
        .oneshot(request(
            &peer,
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{first_email}"}}"#),
        ))
        .await
        .expect("first request-code response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"message": "verification code requested"})
    );

    let rejected_email = unique_email("client-limit-rejected");
    let response = app
        .clone()
        .oneshot(request(
            &peer,
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{rejected_email}"}}"#),
        ))
        .await
        .expect("rate-limited request-code response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .expect("retry-after")
        .to_str()
        .expect("retry-after value")
        .parse::<u64>()
        .expect("positive retry-after");
    assert!(retry_after > 0);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"error": "rate_limited"})
    );
    let rejected_count: i64 =
        support::query_scalar("SELECT COUNT(*) FROM verification_codes WHERE email = $1")
            .bind(&rejected_email)
            .fetch_one(&pool)
            .await
            .expect("count rejected verification codes");
    assert_eq!(
        rejected_count, 0,
        "client rejection must precede durable creation"
    );

    let response = app
        .clone()
        .oneshot(request(
            &peer,
            Method::POST,
            "/daemon/setup/request",
            r#"{"label":"isolated endpoint"}"#,
        ))
        .await
        .expect("isolated setup response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn email_cooldown_consumes_client_bucket() {
    let (pool, _database_test_guard) = database().await;
    let app = auth_router(AuthState::with_log_delivery(AuthStore::new(
        pool.clone(),
        test_otp_key(),
    )));
    let email = unique_email("cooldown");
    let response = app
        .clone()
        .oneshot(request(
            "127.0.0.2:443",
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{email}"}}"#),
        ))
        .await
        .expect("initial request-code response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(request(
                "127.0.0.2:443",
                Method::POST,
                "/auth/request-code",
                &format!(r#"{{"email":"{email}"}}"#),
            ))
            .await
            .expect("cooldown response");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("60")
        );
        assert_eq!(
            json_body(response).await,
            serde_json::json!({"error": "rate_limited"})
        );
    }

    let response = app
        .oneshot(request(
            "127.0.0.2:443",
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{}"}}"#, unique_email("after-cooldown")),
        ))
        .await
        .expect("client bucket response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("120")
    );
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"error": "rate_limited"})
    );

    let issued: i64 =
        support::query_scalar("SELECT COUNT(*) FROM verification_codes WHERE email = $1")
            .bind(&email)
            .fetch_one(&pool)
            .await
            .expect("count issued verification codes");
    assert_eq!(issued, 1, "cooldown rejections must create no code");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn pending_setup_quota_consumes_client_bucket() {
    let (pool, _database_test_guard) = database().await;
    let app = auth_router(AuthState::with_log_delivery(AuthStore::new(
        pool.clone(),
        test_otp_key(),
    )));
    let peer = "127.0.0.3:443";
    let label_prefix = unique("pending-client");

    for index in 0..3 {
        let response = app
            .clone()
            .oneshot(request(
                peer,
                Method::POST,
                "/daemon/setup/request",
                &format!(r#"{{"label":"{label_prefix}-accepted-{index}"}}"#),
            ))
            .await
            .expect("accepted setup response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    for index in 0..2 {
        let response = app
            .clone()
            .oneshot(request(
                peer,
                Method::POST,
                "/daemon/setup/request",
                &format!(r#"{{"label":"{label_prefix}-quota-rejected-{index}"}}"#),
            ))
            .await
            .expect("pending quota response");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("600")
        );
        assert_eq!(
            json_body(response).await,
            serde_json::json!({"error": "rate_limited"})
        );
    }

    let response = app
        .oneshot(request(
            peer,
            Method::POST,
            "/daemon/setup/request",
            &format!(r#"{{"label":"{label_prefix}-client-rejected"}}"#),
        ))
        .await
        .expect("client bucket response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("120")
    );
    assert_eq!(
        json_body(response).await,
        serde_json::json!({"error": "rate_limited"})
    );

    let created: i64 = support::query_scalar(
        "SELECT COUNT(*)
         FROM daemon_setup_requests
         WHERE label LIKE $1
           AND client_network_key = '127.0.0.3/32'::cidr",
    )
    .bind(format!("{label_prefix}-%"))
    .fetch_one(&pool)
    .await
    .expect("count setup rows");
    assert_eq!(
        created, 3,
        "quota and client rejections create no setup row"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn malformed_setup_requests_are_generic_and_do_not_consume_client_bucket() {
    let (pool, _database_test_guard) = database().await;
    let config = PublicEndpointConfig {
        bucket_capacity: 1,
        bucket_refill_interval: Duration::from_secs(120),
        ..PublicEndpointConfig::default()
    };
    let app = auth_router(AuthState::with_public_endpoint_config(
        AuthStore::new(pool.clone(), test_otp_key()),
        Arc::new(LogCodeDelivery),
        config,
    ));
    let peer = "127.0.0.4:443";
    let label_prefix = unique("extractor");
    let malformed = vec![
        (
            "malformed JSON",
            Some("application/json"),
            "{\"label\":\"".to_owned(),
        ),
        ("missing label", Some("application/json"), "{}".to_owned()),
        (
            "wrong label type",
            Some("application/json"),
            r#"{"label":42}"#.to_owned(),
        ),
        (
            "missing content type",
            None,
            format!(r#"{{"label":"{label_prefix}-missing-content-type"}}"#),
        ),
        (
            "unsupported content type",
            Some("text/plain"),
            format!(r#"{{"label":"{label_prefix}-unsupported-content-type"}}"#),
        ),
    ];

    for (description, content_type, body) in malformed {
        let response = app
            .clone()
            .oneshot(request_with_content_type(
                peer,
                Method::POST,
                "/daemon/setup/request",
                &body,
                content_type,
            ))
            .await
            .unwrap_or_else(|_| panic!("{description} response"));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{description}");
        assert_eq!(
            json_body(response).await,
            serde_json::json!({"error": "bad_request"}),
            "{description}"
        );
    }

    let valid_label = format!("{label_prefix}-valid");
    let response = app
        .oneshot(request(
            peer,
            Method::POST,
            "/daemon/setup/request",
            &format!(r#"{{"label":"{valid_label}"}}"#),
        ))
        .await
        .expect("valid setup response");
    assert_eq!(response.status(), StatusCode::OK);

    let created: i64 =
        support::query_scalar("SELECT COUNT(*) FROM daemon_setup_requests WHERE label LIKE $1")
            .bind(format!("{label_prefix}-%"))
            .fetch_one(&pool)
            .await
            .expect("count setup rows");
    assert_eq!(created, 1, "malformed requests must create no setup row");
}

#[derive(Default)]
struct RecordingCodeDelivery(StdMutex<Vec<(String, String)>>);

impl CodeDelivery for RecordingCodeDelivery {
    fn send(&self, email: &str, code: &str) -> Result<(), DeliveryError> {
        self.0
            .lock()
            .expect("recording delivery lock")
            .push((email.to_owned(), code.to_owned()));
        Ok(())
    }
}

fn cookie_request(method: Method, uri: &str, cookie: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("cookie request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn otp_auth_http_lifecycle_preserves_identity_and_invalidates_session() {
    let (admin_pool, _database_test_guard) = database().await;
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for public endpoint tests");
    let schema = format!(
        "north_auth_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    support::query(format!(r#"CREATE SCHEMA "{schema}""#))
        .execute(&admin_pool)
        .await
        .expect("create isolated auth schema");
    let separator = if database_url.contains('?') { '&' } else { '?' };
    let scoped_url = format!("{database_url}{separator}options=-c%20search_path%3D{schema}");
    let pool = support::database(&scoped_url, 1)
        .await
        .expect("connect isolated auth schema");
    let current_schema: String = support::query_scalar("SELECT current_schema()")
        .fetch_one(&pool)
        .await
        .expect("read isolated auth schema");
    assert_eq!(current_schema, schema);
    north_persistence::run_migrations(&pool)
        .await
        .expect("run isolated auth migrations");
    let store = AuthStore::new(pool.clone(), test_otp_key());
    let delivery = Arc::new(RecordingCodeDelivery::default());
    let app = auth_router(AuthState::new(store.clone(), delivery.clone()));
    let email = unique_email("auth-lifecycle");

    let unauthenticated = app
        .clone()
        .oneshot(request(&unique_peer(), Method::GET, "/auth/me", ""))
        .await
        .expect("unauthenticated profile response");
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let protected = app
        .clone()
        .oneshot(request(&unique_peer(), Method::GET, "/requirements", ""))
        .await
        .expect("unauthenticated requirements response");
    assert_eq!(protected.status(), StatusCode::UNAUTHORIZED);

    let issued = app
        .clone()
        .oneshot(request(
            &unique_peer(),
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{email}"}}"#),
        ))
        .await
        .expect("request-code response");
    assert_eq!(issued.status(), StatusCode::ACCEPTED);
    let issued_body = json_body(issued).await;
    assert_eq!(issued_body["message"], "verification code requested");
    let code = delivery
        .0
        .lock()
        .expect("recording delivery lock")
        .last()
        .expect("delivered verification code")
        .1
        .clone();
    assert_eq!(code.len(), 6);
    assert!(!issued_body.to_string().contains(&code));

    let mut invalid_bytes = code.into_bytes();
    invalid_bytes[0] = if invalid_bytes[0] == b'0' { b'1' } else { b'0' };
    let invalid_code = String::from_utf8(invalid_bytes).expect("ASCII code");
    let invalid = app
        .clone()
        .oneshot(request(
            &unique_peer(),
            Method::POST,
            "/auth/verify",
            &format!(r#"{{"email":"{email}","code":"{invalid_code}"}}"#),
        ))
        .await
        .expect("invalid-code response");
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

    let expired_email = unique_email("auth-expired");
    let expired_request = app
        .clone()
        .oneshot(request(
            &unique_peer(),
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{expired_email}"}}"#),
        ))
        .await
        .expect("expired-code request response");
    assert_eq!(expired_request.status(), StatusCode::ACCEPTED);
    let expired_code = delivery
        .0
        .lock()
        .expect("recording delivery lock")
        .last()
        .expect("delivered expired-test code")
        .1
        .clone();
    support::query(
        "UPDATE verification_codes
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
         WHERE email = $1 AND used_at IS NULL",
    )
    .bind(&expired_email)
    .execute(&pool)
    .await
    .expect("expire verification code");
    let expired = app
        .clone()
        .oneshot(request(
            &unique_peer(),
            Method::POST,
            "/auth/verify",
            &format!(r#"{{"email":"{expired_email}","code":"{expired_code}"}}"#),
        ))
        .await
        .expect("expired-code response");
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);

    let verified = app
        .clone()
        .oneshot(request(
            &unique_peer(),
            Method::POST,
            "/auth/verify",
            &format!(
                r#"{{"email":"{email}","code":"{}"}}"#,
                delivery.0.lock().expect("delivery lock")[0].1
            ),
        ))
        .await
        .expect("verification response");
    assert_eq!(verified.status(), StatusCode::NO_CONTENT);
    let set_cookie = verified
        .headers()
        .get(header::SET_COOKIE)
        .expect("session cookie")
        .to_str()
        .expect("cookie header")
        .to_owned();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let session_cookie = set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned();
    let (cookie_name, token) = session_cookie.split_once('=').expect("cookie token");
    assert_eq!(cookie_name, "north_session");
    assert!(!token.is_empty());

    let profile_response = app
        .clone()
        .oneshot(cookie_request(Method::GET, "/auth/me", &session_cookie, ""))
        .await
        .expect("authenticated profile response");
    assert_eq!(profile_response.status(), StatusCode::OK);
    let profile = json_body(profile_response).await;
    assert_eq!(profile["email"], email);
    assert_eq!(profile["role"], "Owner");
    assert!(!profile["id"].as_str().unwrap_or_default().is_empty());
    assert!(!profile["created_at"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
    for secret in [
        "session_id",
        "session_token",
        "token_hash",
        "verification_code",
    ] {
        assert!(profile.get(secret).is_none(), "profile exposed {secret}");
    }

    let role_mutation = app
        .clone()
        .oneshot(cookie_request(
            Method::PATCH,
            "/auth/me",
            &session_cookie,
            r#"{"role":"Admin"}"#,
        ))
        .await
        .expect("profile role mutation response");
    assert_eq!(role_mutation.status(), StatusCode::METHOD_NOT_ALLOWED);
    let stable_profile = app
        .clone()
        .oneshot(cookie_request(Method::GET, "/auth/me", &session_cookie, ""))
        .await
        .expect("stable profile response");
    assert_eq!(json_body(stable_profile).await, profile);

    let logout = app
        .clone()
        .oneshot(cookie_request(
            Method::POST,
            "/auth/logout",
            &session_cookie,
            "",
        ))
        .await
        .expect("logout response");
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let cleared_cookie = logout
        .headers()
        .get(header::SET_COOKIE)
        .expect("cleared session cookie")
        .to_str()
        .expect("clear cookie header");
    assert!(cleared_cookie.starts_with("north_session=;"));
    assert!(cleared_cookie.contains("Max-Age=0"));
    assert!(store
        .user_for_session(token)
        .await
        .expect("session lookup")
        .is_none());

    let after_logout = app
        .clone()
        .oneshot(cookie_request(Method::GET, "/auth/me", &session_cookie, ""))
        .await
        .expect("post-logout profile response");
    assert_eq!(after_logout.status(), StatusCode::UNAUTHORIZED);
    let protected_after_logout = app
        .oneshot(cookie_request(
            Method::GET,
            "/requirements",
            &session_cookie,
            "",
        ))
        .await
        .expect("post-logout protected response");
    assert_eq!(protected_after_logout.status(), StatusCode::UNAUTHORIZED);

    drop(store);
    let _ = pool.close().await;
    support::query(format!(r#"DROP SCHEMA "{schema}" CASCADE"#))
        .execute(&admin_pool)
        .await
        .expect("drop isolated auth schema");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn malformed_request_does_not_consume_client_bucket() {
    let (pool, _database_test_guard) = database().await;
    let config = PublicEndpointConfig {
        bucket_capacity: 1,
        bucket_refill_interval: Duration::from_secs(120),
        ..PublicEndpointConfig::default()
    };
    let app = auth_router(AuthState::with_public_endpoint_config(
        AuthStore::new(pool, test_otp_key()),
        Arc::new(LogCodeDelivery),
        config,
    ));
    let peer = "127.0.0.4:443";
    let response = app
        .clone()
        .oneshot(request(peer, Method::POST, "/auth/request-code", r#"{}"#))
        .await
        .expect("malformed request response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = app
        .oneshot(request(
            peer,
            Method::POST,
            "/auth/request-code",
            &format!(r#"{{"email":"{}"}}"#, unique_email("after-malformed")),
        ))
        .await
        .expect("valid request response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}
