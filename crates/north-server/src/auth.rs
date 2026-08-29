use axum::{
    extract::{rejection::JsonRejection, Json, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Extension, Router,
};
use north_persistence::{
    AuthStore, AuthenticatedSession, PersistenceError, UserRecord, CODE_REQUEST_COOLDOWN_SECONDS,
    SESSION_TTL_SECONDS,
};
use rand::{rng, Rng};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

pub const SESSION_COOKIE_NAME: &str = "north_session";
pub const VERIFICATION_CODE_LENGTH: usize = 6;
pub const CODE_REQUEST_MIN_INTERVAL_SECONDS: i64 = CODE_REQUEST_COOLDOWN_SECONDS;

/// Delivery boundary for verification codes. HTTP/auth semantics do not depend
/// on whether delivery logs a code or sends it through a provider.
pub trait CodeDelivery: Send + Sync {
    fn send(&self, email: &str, code: &str) -> Result<(), DeliveryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryError;

impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("verification code delivery failed")
    }
}

impl std::error::Error for DeliveryError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct LogCodeDelivery;

impl CodeDelivery for LogCodeDelivery {
    fn send(&self, email: &str, code: &str) -> Result<(), DeliveryError> {
        eprintln!("north verification code email={email} code={code}");
        Ok(())
    }
}

#[derive(Clone)]
pub struct AuthState {
    store: AuthStore,
    delivery: Arc<dyn CodeDelivery>,
    daemon_runtime: crate::daemon::DaemonRuntime,
}

impl AuthState {
    pub fn new(store: AuthStore, delivery: Arc<dyn CodeDelivery>) -> Self {
        Self {
            daemon_runtime: crate::daemon::DaemonRuntime::new(store.clone()),
            store,
            delivery,
        }
    }

    pub fn with_log_delivery(store: AuthStore) -> Self {
        Self::new(store, Arc::new(LogCodeDelivery))
    }

    pub fn store(&self) -> &AuthStore {
        &self.store
    }

    pub fn daemon_runtime(&self) -> &crate::daemon::DaemonRuntime {
        &self.daemon_runtime
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestCodeRequest {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyCodeRequest {
    pub email: String,
    pub code: String,
}

/// User identity extracted by auth middleware and made available to protected
/// handlers through request extensions.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub UserRecord);

impl CurrentUser {
    pub fn user(&self) -> &UserRecord {
        &self.0
    }
}

#[derive(Debug)]
pub enum AuthHttpError {
    BadRequest,
    InvalidCode,
    RateLimited,
    Unauthorized,
    Internal,
}

impl AuthHttpError {
    fn message(&self) -> &'static str {
        match self {
            Self::BadRequest => "invalid authentication request",
            Self::InvalidCode => "invalid or expired verification code",
            Self::RateLimited => "verification code request rate limited",
            Self::Unauthorized => "authentication required",
            Self::Internal => "authentication service unavailable",
        }
    }
}

impl From<PersistenceError> for AuthHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::InvalidCode => Self::InvalidCode,
            PersistenceError::RateLimited => Self::RateLimited,
            PersistenceError::Database(_)
            | PersistenceError::InvalidRole(_)
            | PersistenceError::InvalidDaemonCredential
            | PersistenceError::RevokedDaemon
            | PersistenceError::DaemonNotFound
            | PersistenceError::SetupNotFound
            | PersistenceError::SetupExpired
            | PersistenceError::SetupAlreadyApproved
            | PersistenceError::SetupAlreadyClaimed
            | PersistenceError::InvalidSetup
            | PersistenceError::NoEligibleDaemon
            | PersistenceError::InvalidCapabilities
            | PersistenceError::InvalidCommandPayload
            | PersistenceError::InvalidSessionState
            | PersistenceError::SessionRequirementMismatch
            | PersistenceError::InvalidRepository(_)
            | PersistenceError::RepositoryNotFound
            | PersistenceError::RepositoryNameConflict
            | PersistenceError::RepositoryUrlImmutable
            | PersistenceError::ProtocolIntegrity(_)
            | PersistenceError::EventSequenceGap { .. } => Self::Internal,
        }
    }
}

#[derive(Debug, Serialize)]
struct PublicMessage {
    message: &'static str,
}

impl IntoResponse for AuthHttpError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::InvalidCode => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(PublicMessage {
                message: self.message(),
            }),
        )
            .into_response()
    }
}

/// Auth routes. Public issuance/verification stay outside the auth middleware;
/// logout is protected and receives CurrentUser through request extensions.
pub fn router(state: AuthState) -> Router {
    state.daemon_runtime().start();
    let public = Router::new()
        .route("/auth/request-code", post(request_code))
        .route("/auth/verify", post(verify_code))
        .merge(crate::daemon::public_router());
    let protected = Router::new()
        .route("/auth/logout", post(logout))
        .merge(crate::roles::router())
        .merge(crate::requirements::router())
        .merge(crate::repositories::router())
        .merge(crate::daemon::protected_router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    public.merge(protected).with_state(state)
}

pub async fn request_code(
    State(state): State<AuthState>,
    payload: Result<Json<RequestCodeRequest>, JsonRejection>,
) -> Result<Response, AuthHttpError> {
    let Json(payload) = payload.map_err(|_| AuthHttpError::BadRequest)?;
    let email = normalize_email(&payload.email).ok_or(AuthHttpError::BadRequest)?;
    let code = generate_code();

    state.store.issue_code(&email, &code).await?;
    state
        .delivery
        .send(&email, &code)
        .map_err(|_| AuthHttpError::Internal)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(PublicMessage {
            message: "verification code requested",
        }),
    )
        .into_response())
}

pub async fn verify_code(
    State(state): State<AuthState>,
    payload: Result<Json<VerifyCodeRequest>, JsonRejection>,
) -> Result<Response, AuthHttpError> {
    let Json(payload) = payload.map_err(|_| AuthHttpError::BadRequest)?;
    let email = normalize_email(&payload.email).ok_or(AuthHttpError::BadRequest)?;
    if !valid_code(&payload.code) {
        return Err(AuthHttpError::InvalidCode);
    }

    let session = state.store.verify_code(&email, &payload.code).await?;
    let cookie = session_cookie(&session)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}

pub async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Extension(_current_user): Extension<CurrentUser>,
) -> Result<Response, AuthHttpError> {
    let token = session_token(&headers).ok_or(AuthHttpError::Unauthorized)?;
    state.store.invalidate_session(&token).await?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_session_cookie()?);
    Ok(response)
}

/// Extract and validate server-side session before entering protected handlers.
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = session_token(request.headers()).ok_or(StatusCode::UNAUTHORIZED)?;
    let user = state
        .store
        .user_for_session(&token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    request.extensions_mut().insert(CurrentUser(user));
    Ok(next.run(request).await)
}

fn normalize_email(email: &str) -> Option<String> {
    let email = email.trim().to_ascii_lowercase();
    let (local, domain) = email.split_once('@')?;
    if email.len() > 254
        || local.is_empty()
        || domain.is_empty()
        || domain.starts_with('.')
        || domain.ends_with('.')
        || !domain.contains('.')
    {
        return None;
    }
    Some(email)
}

fn generate_code() -> String {
    format!(
        "{:0width$}",
        rng().random_range(0..1_000_000_u32),
        width = VERIFICATION_CODE_LENGTH
    )
}

fn valid_code(code: &str) -> bool {
    code.len() == VERIFICATION_CODE_LENGTH && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE_NAME && !value.is_empty()).then(|| value.to_string())
    })
}

fn session_cookie(session: &AuthenticatedSession) -> Result<HeaderValue, AuthHttpError> {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}",
        session.token
    ))
    .map_err(|_| AuthHttpError::Internal)
}

fn clear_session_cookie() -> Result<HeaderValue, AuthHttpError> {
    HeaderValue::from_str("north_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0")
        .map_err(|_| AuthHttpError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_six_digits() {
        for _ in 0..32 {
            let code = generate_code();
            assert_eq!(code.len(), VERIFICATION_CODE_LENGTH);
            assert!(valid_code(&code));
        }
    }

    #[test]
    fn public_payloads_contain_no_secret_material() {
        let secret_code = "123456";
        let secret_token = "deadbeefdeadbeef";
        for error in [
            AuthHttpError::BadRequest,
            AuthHttpError::InvalidCode,
            AuthHttpError::RateLimited,
            AuthHttpError::Unauthorized,
            AuthHttpError::Internal,
        ] {
            let payload = serde_json::to_string(&PublicMessage {
                message: error.message(),
            })
            .expect("serialize public error");
            assert!(!payload.contains(secret_code));
            assert!(!payload.contains(secret_token));
            assert!(!payload.contains("hash"));
        }
        let acknowledgement = serde_json::to_string(&PublicMessage {
            message: "verification code requested",
        })
        .expect("serialize public success");
        assert!(!acknowledgement.contains(secret_code));
        assert!(!acknowledgement.contains(secret_token));
    }

    #[test]
    fn session_cookie_is_http_only_and_secure() {
        let cookie = session_cookie(&AuthenticatedSession {
            user: UserRecord {
                id: "user-1".into(),
                email: "user@example.com".into(),
                role: north_domain::role::Role::Requester,
            },
            token: "deadbeef".into(),
        })
        .expect("cookie header");
        let cookie = cookie.to_str().expect("valid cookie");
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(!cookie.contains("token_hash"));
    }

    #[test]
    fn email_normalization_is_stable() {
        assert_eq!(
            normalize_email(" User@Example.COM "),
            Some("user@example.com".into())
        );
        assert!(normalize_email("not-an-email").is_none());
        assert_eq!(CODE_REQUEST_COOLDOWN_SECONDS, 60);
    }
}
