use axum::{
    extract::{ws::WebSocketUpgrade, Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use north_persistence::{AuthStore, DaemonRegistration, DaemonSetupClaim, PersistenceError};
use north_protocol::{
    DaemonFrame, Heartbeat, ProtocolErrorFrame, ReconcileSnapshot, ServerFrame,
    SessionReconcileState, Welcome, PROTOCOL_VERSION, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

use crate::{
    auth::{AuthState, CurrentUser},
    roles::require_admin,
    transport::{self, DaemonConnection, DaemonTransportState},
};

#[derive(Debug)]
pub enum DaemonHttpError {
    BadRequest,
    Forbidden,
    NotFound,
    Gone,
    Conflict,
    Internal,
}

impl DaemonHttpError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Gone => StatusCode::GONE,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Forbidden => "permission_denied",
            Self::NotFound => "not_found",
            Self::Gone => "expired",
            Self::Conflict => "conflict",
            Self::Internal => "internal_error",
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for DaemonHttpError {
    fn into_response(self) -> Response {
        (self.status(), Json(ErrorBody { error: self.code() })).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupCreatedResponse {
    pub request_token: String,
    pub verification_path: String,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Serialize)]
struct SetupPendingResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct SetupClaimedResponse {
    status: &'static str,
    daemon_id: String,
    credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub daemon_id: String,
    pub label: String,
    pub created_by: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub connected: bool,
    pub protocol_version: String,
    pub capabilities: Vec<String>,
}

impl From<DaemonRegistration> for DaemonResponse {
    fn from(daemon: DaemonRegistration) -> Self {
        Self {
            daemon_id: daemon.daemon_id,
            label: daemon.label,
            created_by: daemon.created_by,
            created_at: daemon.created_at,
            revoked_at: daemon.revoked_at,
            last_seen_at: daemon.last_seen_at,
            connected: daemon.connected,
            protocol_version: daemon.protocol_version,
            capabilities: daemon.capabilities,
        }
    }
}

/// Public device-flow and daemon WebSocket routes. Authentication for the
/// daemon connection is the North credential in `hello`, not a browser session.
pub fn public_router() -> Router<AuthState> {
    Router::new()
        .route("/daemon/setup/request", post(request_setup))
        .route("/daemon/setup/{request_token}", get(poll_setup))
        .route("/daemon/ws", get(daemon_websocket))
}

/// Browser-authenticated setup approval and administrator status/revocation.
pub fn protected_router() -> Router<AuthState> {
    Router::new()
        .route(
            "/daemon/setup/{request_token}/approve",
            get(approve_setup).post(approve_setup),
        )
        .route("/daemons", get(list_daemons))
        .route("/daemons/{daemon_id}/revoke", post(revoke_daemon))
}

pub async fn request_setup(
    State(state): State<AuthState>,
    Json(payload): Json<SetupRequest>,
) -> Result<Json<SetupCreatedResponse>, DaemonHttpError> {
    let label = payload.label.trim();
    if label.is_empty() || label.len() > 100 {
        return Err(DaemonHttpError::BadRequest);
    }
    let request = state
        .store()
        .create_daemon_setup_request(label)
        .await
        .map_err(store_error)?;
    Ok(Json(SetupCreatedResponse {
        verification_path: format!("/daemon/setup/{}/approve", request.request_token),
        request_token: request.request_token,
        expires_in_seconds: north_persistence::DAEMON_SETUP_TTL_SECONDS,
    }))
}

pub async fn poll_setup(
    State(state): State<AuthState>,
    Path(request_token): Path<String>,
) -> Result<Response, DaemonHttpError> {
    match state
        .store()
        .claim_daemon_setup_request(&request_token)
        .await
        .map_err(store_error)?
    {
        DaemonSetupClaim::Pending => Ok((
            StatusCode::ACCEPTED,
            Json(SetupPendingResponse { status: "pending" }),
        )
            .into_response()),
        DaemonSetupClaim::Claimed {
            daemon_id,
            credential,
        } => Ok(Json(SetupClaimedResponse {
            status: "claimed",
            daemon_id,
            credential,
        })
        .into_response()),
    }
}

pub async fn approve_setup(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(request_token): Path<String>,
) -> Result<StatusCode, DaemonHttpError> {
    state
        .store()
        .approve_daemon_setup_request(&request_token, &user.user().id)
        .await
        .map_err(store_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_daemons(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<DaemonResponse>>, DaemonHttpError> {
    let can_view_all = user.user().role.can_administer();
    let user_id = user.user().id.clone();
    let daemons = state
        .store()
        .list_daemons()
        .await
        .map_err(store_error)?
        .into_iter()
        .filter(|daemon| can_view_all || daemon.created_by == user_id)
        .map(Into::into)
        .collect();
    Ok(Json(daemons))
}

pub async fn revoke_daemon(
    State(state): State<AuthState>,
    Extension(user): Extension<CurrentUser>,
    Path(daemon_id): Path<String>,
) -> Result<StatusCode, DaemonHttpError> {
    let daemon = state
        .store()
        .daemon_by_id(&daemon_id)
        .await
        .map_err(store_error)?
        .ok_or(DaemonHttpError::NotFound)?;
    if daemon.created_by != user.user().id {
        require_admin(&user).map_err(|_| DaemonHttpError::Forbidden)?;
    }
    let connection_id = state
        .store()
        .revoke_daemon(&daemon_id)
        .await
        .map_err(store_error)?;
    if let Some(connection_id) = connection_id {
        state
            .daemon_runtime()
            .close_daemon(&daemon_id, &connection_id)
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn daemon_websocket(State(state): State<AuthState>, ws: WebSocketUpgrade) -> Response {
    transport::daemon_websocket_response(ws, state.daemon_runtime().transport())
}

#[derive(Clone)]
pub struct DaemonRuntime {
    inner: Arc<DaemonRuntimeInner>,
}

struct DaemonRuntimeInner {
    store: AuthStore,
    transport: DaemonTransportState,
    receiver: Mutex<Option<mpsc::Receiver<DaemonConnection>>>,
    live: tokio::sync::Mutex<HashMap<String, LiveConnection>>,
}

struct LiveConnection {
    connection_id: String,
    outbound: mpsc::Sender<ServerFrame>,
    close: mpsc::Sender<()>,
}

#[derive(Debug)]
pub enum DaemonDispatchError {
    InvalidCommand,
    SessionNotFound,
    DaemonUnavailable,
    Internal,
}

impl DaemonRuntime {
    pub fn new(store: AuthStore) -> Self {
        let (transport, receiver) = DaemonTransportState::channel();
        Self {
            inner: Arc::new(DaemonRuntimeInner {
                store,
                transport,
                receiver: Mutex::new(Some(receiver)),
                live: tokio::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn transport(&self) -> DaemonTransportState {
        self.inner.transport.clone()
    }

    pub fn start(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let receiver = self
            .inner
            .receiver
            .lock()
            .expect("daemon receiver lock")
            .take();
        let Some(mut receiver) = receiver else {
            return;
        };
        let runtime = self.clone();
        tokio::spawn(async move {
            while let Some(connection) = receiver.recv().await {
                let runtime = runtime.clone();
                tokio::spawn(async move { runtime.handle_connection(connection).await });
            }
        });
    }

    async fn handle_connection(&self, mut connection: DaemonConnection) {
        let Some(DaemonFrame::Hello(hello)) = connection.inbound.recv().await else {
            return;
        };
        if hello.protocol_version != PROTOCOL_VERSION {
            send_protocol_error(
                &connection.outbound,
                "incompatible_protocol",
                "unsupported protocol version",
            )
            .await;
            return;
        }
        let authenticated = match self
            .inner
            .store
            .connect_daemon(
                &hello.daemon_id,
                &hello.credential,
                &hello.protocol_version,
                &hello.capabilities,
            )
            .await
        {
            Ok(authenticated) => authenticated,
            Err(error) => {
                send_protocol_error(
                    &connection.outbound,
                    daemon_error_code(&error),
                    "daemon authentication failed",
                )
                .await;
                return;
            }
        };
        let (close_sender, close_receiver) = mpsc::channel(1);
        let old = self.inner.live.lock().await.insert(
            authenticated.daemon_id.clone(),
            LiveConnection {
                connection_id: authenticated.connection_id.clone(),
                outbound: connection.outbound.clone(),
                close: close_sender,
            },
        );
        if let Some(old) = old {
            let _ = old.close.send(()).await;
        }

        self.serve_authenticated(
            &mut connection,
            &authenticated.daemon_id,
            &authenticated.connection_id,
            close_receiver,
        )
        .await;

        let mut live = self.inner.live.lock().await;
        if live
            .get(&authenticated.daemon_id)
            .is_some_and(|entry| entry.connection_id == authenticated.connection_id)
        {
            live.remove(&authenticated.daemon_id);
        }
        drop(live);
        let _ = self
            .inner
            .store
            .disconnect_daemon(&authenticated.daemon_id, &authenticated.connection_id)
            .await;
    }

    async fn serve_authenticated(
        &self,
        connection: &mut DaemonConnection,
        daemon_id: &str,
        connection_id: &str,
        mut close: mpsc::Receiver<()>,
    ) {
        let welcome = ServerFrame::Welcome(Welcome {
            protocol_version: PROTOCOL_VERSION.into(),
            schema_version: SCHEMA_VERSION,
            daemon_id: daemon_id.into(),
            server_time: server_time(),
        });
        if connection.outbound.send(welcome).await.is_err() {
            return;
        }
        let sessions = match self.inner.store.reconciliation_for_daemon(daemon_id).await {
            Ok(sessions) => sessions
                .into_iter()
                .map(|session| SessionReconcileState {
                    session_id: session.session_id,
                    command_ack_through_seq: session.command_ack_through_seq,
                    event_ack_through_seq: session.event_ack_through_seq,
                    event_ack_sparse: Vec::new(),
                })
                .collect(),
            Err(_) => {
                send_protocol_error(
                    &connection.outbound,
                    "internal_error",
                    "unable to build reconciliation snapshot",
                )
                .await;
                return;
            }
        };
        if connection
            .outbound
            .send(ServerFrame::Reconcile(ReconcileSnapshot {
                schema_version: SCHEMA_VERSION,
                sessions,
            }))
            .await
            .is_err()
        {
            return;
        }

        loop {
            tokio::select! {
                _ = close.recv() => {
                    send_protocol_error(&connection.outbound, "daemon_access_revoked", "daemon credential revoked").await;
                    return;
                }
                frame = connection.inbound.recv() => match frame {
                    Some(DaemonFrame::Heartbeat(Heartbeat { daemon_id: reported_id, .. })) => {
                        if reported_id != daemon_id || self.inner.store.touch_daemon(daemon_id, connection_id).await.is_err() {
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "heartbeat identity rejected").await;
                            return;
                        }
                    }
                    Some(DaemonFrame::Event(event)) => {
                        if self.inner.store.touch_daemon(daemon_id, connection_id).await.is_err()
                            || !self.session_belongs_to_daemon(&event.session_id, daemon_id).await
                        {
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "event session owner rejected").await;
                            return;
                        }
                    }
                    Some(DaemonFrame::CommandAck(ack)) => {
                        if self.inner.store.touch_daemon(daemon_id, connection_id).await.is_err()
                            || !self.session_belongs_to_daemon(&ack.session_id, daemon_id).await
                        {
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "command ACK session owner rejected").await;
                            return;
                        }
                    }
                    Some(DaemonFrame::Hello(_)) => {
                        send_protocol_error(&connection.outbound, "unexpected_hello", "hello is only valid at connection start").await;
                        return;
                    }
                    None => return,
                }
            }
        }
    }

    pub async fn start_session_with_command(
        &self,
        session_id: &str,
        command_id: &str,
        payload: &str,
        required_capabilities: &[String],
    ) -> Result<north_persistence::PinnedCommand, PersistenceError> {
        self.inner
            .store
            .start_session_with_command(session_id, command_id, payload, required_capabilities)
            .await
    }

    pub async fn dispatch_command(&self, command: ServerFrame) -> Result<(), DaemonDispatchError> {
        let session_id = match &command {
            ServerFrame::Command(command) => &command.session_id,
            _ => return Err(DaemonDispatchError::InvalidCommand),
        };
        let daemon_id = self
            .inner
            .store
            .session_owner(session_id)
            .await
            .map_err(|_| DaemonDispatchError::Internal)?
            .ok_or(DaemonDispatchError::SessionNotFound)?;
        let daemon = self
            .inner
            .store
            .daemon_by_id(&daemon_id)
            .await
            .map_err(|_| DaemonDispatchError::Internal)?
            .ok_or(DaemonDispatchError::SessionNotFound)?;
        if daemon.revoked_at.is_some() || !daemon.connected {
            return Err(DaemonDispatchError::DaemonUnavailable);
        }
        let outbound = self
            .inner
            .live
            .lock()
            .await
            .get(&daemon_id)
            .map(|connection| connection.outbound.clone())
            .ok_or(DaemonDispatchError::DaemonUnavailable)?;
        outbound
            .send(command)
            .await
            .map_err(|_| DaemonDispatchError::DaemonUnavailable)
    }

    async fn session_belongs_to_daemon(&self, session_id: &str, daemon_id: &str) -> bool {
        self.inner
            .store
            .session_owner(session_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|owner| owner == daemon_id)
    }

    pub async fn close_daemon(&self, daemon_id: &str, connection_id: &str) {
        let live = {
            let mut connections = self.inner.live.lock().await;
            if connections
                .get(daemon_id)
                .is_some_and(|entry| entry.connection_id == connection_id)
            {
                connections.remove(daemon_id)
            } else {
                None
            }
        };
        if let Some(live) = live {
            let _ = live.close.send(()).await;
        }
    }
}

async fn send_protocol_error(outbound: &mpsc::Sender<ServerFrame>, code: &str, message: &str) {
    let _ = outbound
        .send(ServerFrame::ProtocolError(ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: code.into(),
            message: message.into(),
        }))
        .await;
}

fn daemon_error_code(error: &PersistenceError) -> &'static str {
    match error {
        PersistenceError::RevokedDaemon => "revoked_credential",
        PersistenceError::InvalidDaemonCredential => "invalid_credentials",
        PersistenceError::Database(_) => "internal_error",
        _ => "daemon_authentication_failed",
    }
}

fn store_error(error: PersistenceError) -> DaemonHttpError {
    match error {
        PersistenceError::SetupNotFound | PersistenceError::DaemonNotFound => {
            DaemonHttpError::NotFound
        }
        PersistenceError::SetupExpired => DaemonHttpError::Gone,
        PersistenceError::SetupAlreadyApproved | PersistenceError::SetupAlreadyClaimed => {
            DaemonHttpError::Conflict
        }
        PersistenceError::InvalidDaemonCredential | PersistenceError::RevokedDaemon => {
            DaemonHttpError::Forbidden
        }
        PersistenceError::InvalidCapabilities
        | PersistenceError::InvalidSetup
        | PersistenceError::InvalidSessionState
        | PersistenceError::NoEligibleDaemon
        | PersistenceError::InvalidRole(_)
        | PersistenceError::InvalidCode
        | PersistenceError::RateLimited => DaemonHttpError::BadRequest,
        PersistenceError::Database(_) => DaemonHttpError::Internal,
    }
}

fn server_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
