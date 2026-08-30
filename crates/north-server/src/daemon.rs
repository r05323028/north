use axum::{
    extract::{ws::WebSocketUpgrade, Json, Path, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use north_persistence::{
    canonical_payload_digest, AuthStore, DaemonRegistration, DaemonSetupClaim, DaemonSetupState,
    EventReceiptOutcome, EventReceiptRequest, PersistenceError, RepositoryRecord,
};
use north_protocol::{
    Command, CommandEnvelope, DaemonFrame, EventAck, EventAckStatus, EventEnvelope, Heartbeat,
    ProtocolErrorFrame, ReconcileSnapshot, RepositoryContext, ServerFrame, SessionReconcileState,
    SessionStart, Welcome, PROTOCOL_VERSION, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupApprovalResponse {
    pub status: String,
    pub label: String,
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
            get(preview_setup).post(approve_setup),
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

pub async fn preview_setup(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(request_token): Path<String>,
) -> Result<Response, DaemonHttpError> {
    let preview = state
        .store()
        .preview_daemon_setup_request(&request_token)
        .await
        .map_err(store_error)?;
    let status = match preview.state {
        DaemonSetupState::Pending => "pending",
        DaemonSetupState::Approved => "approved",
        DaemonSetupState::Claimed => "claimed",
    };
    let response = SetupApprovalResponse {
        status: status.into(),
        label: preview.label,
    };
    if wants_html(&headers) {
        Ok((
            [(header::CACHE_CONTROL, "no-store")],
            Html(render_setup_approval_page(&request_token, &response)),
        )
            .into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

pub async fn approve_setup(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Extension(user): Extension<CurrentUser>,
    Path(request_token): Path<String>,
) -> Result<Response, DaemonHttpError> {
    if !same_origin(&headers) {
        return Err(DaemonHttpError::Forbidden);
    }
    state
        .store()
        .approve_daemon_setup_request(&request_token, &user.user().id)
        .await
        .map_err(store_error)?;
    if wants_html(&headers) {
        Ok((
            [(header::CACHE_CONTROL, "no-store")],
            Html(render_setup_approved_page()),
        )
            .into_response())
    } else {
        Ok(StatusCode::NO_CONTENT.into_response())
    }
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
    ready: bool,
}

#[derive(Debug)]
pub enum DaemonDispatchError {
    InvalidCommand,
    SessionNotFound,
    DaemonUnavailable,
    Internal,
}

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub command_id: String,
    pub session_id: String,
    pub command: Command,
}

enum EventHandleError {
    Gap,
    Integrity(String),
    RequirementMismatch,
    Internal,
}

fn map_persistence_event_error(error: PersistenceError) -> EventHandleError {
    match error {
        PersistenceError::EventSequenceGap { .. } => EventHandleError::Gap,
        PersistenceError::ProtocolIntegrity(reason) => EventHandleError::Integrity(reason),
        _ => EventHandleError::Internal,
    }
}

fn map_assessment_event_error(error: crate::assessment::AssessmentError) -> EventHandleError {
    match error {
        crate::assessment::AssessmentError::Persistence(
            north_persistence::ReadinessError::SequenceGap { .. },
        ) => EventHandleError::Gap,
        crate::assessment::AssessmentError::InvalidPayload(error) => {
            EventHandleError::Integrity(error.to_string())
        }
        crate::assessment::AssessmentError::Persistence(
            north_persistence::ReadinessError::SequenceConflict
            | north_persistence::ReadinessError::EventIdentityConflict,
        ) => EventHandleError::Integrity("event identity conflict".into()),
        crate::assessment::AssessmentError::Persistence(
            north_persistence::ReadinessError::SessionRequirementMismatch,
        ) => EventHandleError::RequirementMismatch,
        crate::assessment::AssessmentError::Persistence(_)
        | crate::assessment::AssessmentError::NotAssessmentEvent => EventHandleError::Internal,
    }
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
            .ok()
            .and_then(|mut receiver| receiver.take());
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
            Err(PersistenceError::Database(_)) => {
                // Close without a protocol error; the daemon may reconnect and
                // retry authentication once persistence is available.
                return;
            }
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
                ready: false,
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
                    event_ack_sparse: session.event_ack_sparse,
                })
                .collect(),
            Err(PersistenceError::Database(_)) => {
                return;
            }
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

        let pending_commands = match self.inner.store.unacknowledged_commands(daemon_id).await {
            Ok(commands) => commands,
            Err(PersistenceError::Database(_)) => {
                return;
            }
            Err(_) => {
                send_protocol_error(
                    &connection.outbound,
                    "internal_error",
                    "unable to load unacknowledged commands",
                )
                .await;
                return;
            }
        };
        for pending in pending_commands {
            let frame = match ServerFrame::from_json(&pending.payload) {
                Ok(ServerFrame::Command(command))
                    if command.command_id == pending.command_id
                        && command.session_id == pending.session_id
                        && command.server_command_seq == pending.server_command_seq =>
                {
                    ServerFrame::Command(command)
                }
                _ => {
                    send_protocol_error(
                        &connection.outbound,
                        "invalid_outbox_payload",
                        "durable command payload identity does not match its outbox row",
                    )
                    .await;
                    return;
                }
            };
            if connection.outbound.send(frame).await.is_err() {
                return;
            }
        }
        self.mark_live_ready(daemon_id, connection_id).await;

        loop {
            tokio::select! {
                _ = close.recv() => {
                    send_protocol_error(&connection.outbound, "daemon_access_revoked", "daemon credential revoked").await;
                    return;
                }
                frame = connection.inbound.recv() => match frame {
                    Some(DaemonFrame::Heartbeat(Heartbeat { daemon_id: reported_id, .. })) => {
                        if reported_id != daemon_id {
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "heartbeat identity rejected").await;
                            return;
                        }
                        if let Err(error) = self.inner.store.touch_daemon(daemon_id, connection_id).await {
                            if matches!(error, PersistenceError::Database(_)) {
                                return;
                            }
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "heartbeat identity rejected").await;
                            return;
                        }
                    }
                    Some(DaemonFrame::Event(event)) => {
                        if let Err(error) = self
                            .validate_daemon_session(&event.session_id, daemon_id, connection_id)
                            .await
                        {
                            if matches!(error, PersistenceError::Database(_)) {
                                return;
                            }
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "event session owner rejected").await;
                            return;
                        }
                        match self.handle_event(&event).await {
                            Ok(ack) => {
                                if connection.outbound.send(ServerFrame::EventAck(ack)).await.is_err() {
                                    return;
                                }
                            }
                            Err(EventHandleError::Gap) => {
                                // A valid frame above the next sequence remains in the
                                // daemon journal; close without a protocol error so replay
                                // can fill the gap on the next connection.
                                return;
                            }
                            Err(EventHandleError::RequirementMismatch) => {
                                send_protocol_error(
                                    &connection.outbound,
                                    "assessment_requirement_mismatch",
                                    "assessment requirement is not bound to event session",
                                )
                                .await;
                                return;
                            }
                            Err(EventHandleError::Integrity(reason)) => {
                                send_protocol_error(&connection.outbound, "event_identity_conflict", &reason).await;
                                return;
                            }
                            Err(EventHandleError::Internal) => {
                                // No ACK and no protocol error: the journaled event
                                // remains replay-eligible after this connection closes.
                                return;
                            }
                        }
                    }
                    Some(DaemonFrame::CommandAck(ack)) => {
                        if let Err(error) = self
                            .validate_daemon_session(&ack.session_id, daemon_id, connection_id)
                            .await
                        {
                            if matches!(error, PersistenceError::Database(_)) {
                                return;
                            }
                            send_protocol_error(&connection.outbound, "daemon_identity_mismatch", "command ACK session owner rejected").await;
                            return;
                        }
                        match self.inner.store.acknowledge_command(
                            &ack.command_id,
                            &ack.session_id,
                            ack.server_command_seq,
                        ).await {
                            Ok(_) => {}
                            Err(PersistenceError::ProtocolIntegrity(reason)) => {
                                send_protocol_error(&connection.outbound, "command_ack_conflict", &reason).await;
                                return;
                            }
                            Err(_) => {
                                // Do not turn a transient database failure into a
                                // terminal protocol violation; the daemon will resend.
                                return;
                            }
                        }
                    }
                    Some(DaemonFrame::ProtocolError(_)) => {
                        // The daemon reports a terminal protocol violation;
                        // close only this authenticated connection.
                        return;
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

    async fn handle_event(&self, event: &EventEnvelope) -> Result<EventAck, EventHandleError> {
        if matches!(event.event, north_protocol::Event::RequirementAssessed(_)) {
            return crate::assessment::handle_requirement_assessed(&self.inner.store, event)
                .await
                .map_err(map_assessment_event_error);
        }

        let value = serde_json::to_value(event)
            .map_err(|error| EventHandleError::Integrity(error.to_string()))?;
        let digest = canonical_payload_digest(&value);
        let payload = serde_json::to_string(event)
            .map_err(|error| EventHandleError::Integrity(error.to_string()))?;
        let receipt = self
            .inner
            .store
            .record_event_receipt_with_payload(EventReceiptRequest {
                event_id: &event.event_id,
                session_id: &event.session_id,
                daemon_event_seq: event.daemon_event_seq,
                payload_digest: &digest,
                payload: &payload,
                outcome: EventReceiptOutcome::Rejected,
                rejection_reason: Some("event_handler_not_implemented"),
            })
            .await
            .map_err(map_persistence_event_error)?;
        let (status, reason) = match receipt.outcome {
            EventReceiptOutcome::Accepted => (EventAckStatus::Accepted, None),
            EventReceiptOutcome::Rejected => {
                (EventAckStatus::Rejected, receipt.rejection_reason.clone())
            }
        };
        Ok(EventAck {
            event_id: receipt.event_id,
            session_id: receipt.session_id,
            daemon_event_seq: receipt.daemon_event_seq,
            schema_version: SCHEMA_VERSION,
            status,
            reason,
        })
    }

    pub async fn persist_and_dispatch_command(
        &self,
        request: CommandRequest,
        required_capabilities: &[String],
    ) -> Result<north_persistence::PinnedCommand, DaemonDispatchError> {
        let command_id = request.command_id.clone();
        let session_id = request.session_id.clone();
        if let Some(pinned) = self
            .inner
            .store
            .command_by_id(&command_id)
            .await
            .map_err(|_| DaemonDispatchError::Internal)?
        {
            if pinned.session_id != session_id {
                return Err(DaemonDispatchError::InvalidCommand);
            }
            if pinned.compacted {
                let matches = if pinned.payload.is_empty() {
                    let candidate = ServerFrame::Command(CommandEnvelope {
                        command_id: pinned.command_id.clone(),
                        session_id: pinned.session_id.clone(),
                        server_command_seq: pinned.server_command_seq,
                        sent_at: String::new(),
                        schema_version: SCHEMA_VERSION,
                        command: request.command.clone(),
                    })
                    .to_json()
                    .map_err(|_| DaemonDispatchError::InvalidCommand)?;
                    north_persistence::command_identity_digest(&candidate)
                        == pinned.command_identity_digest
                } else {
                    let persisted = ServerFrame::from_json(&pinned.payload)
                        .map_err(|_| DaemonDispatchError::InvalidCommand)?;
                    let ServerFrame::Command(envelope) = persisted else {
                        return Err(DaemonDispatchError::InvalidCommand);
                    };
                    same_retry_command(&request.command, &envelope.command)
                };
                if !matches {
                    return Err(DaemonDispatchError::InvalidCommand);
                }
                return Ok(pinned);
            }
            let persisted = ServerFrame::from_json(&pinned.payload)
                .map_err(|_| DaemonDispatchError::InvalidCommand)?;
            let ServerFrame::Command(envelope) = &persisted else {
                return Err(DaemonDispatchError::InvalidCommand);
            };
            if !same_retry_command(&request.command, &envelope.command) {
                return Err(DaemonDispatchError::InvalidCommand);
            }
            self.dispatch_persisted_command(&pinned, persisted).await?;
            return Ok(pinned);
        }
        let command = self.assemble_session_command(request.command).await?;
        let repository_ids = match &command {
            Command::SessionStart(start) => Some(
                start
                    .repositories
                    .iter()
                    .map(|repository| repository.repository_id.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let envelope_command_id = command_id.clone();
        let envelope_session_id = session_id.clone();
        let requirement_id = match &command {
            Command::SessionStart(start) => Some(start.requirement.id.clone()),
            _ => None,
        };
        let pinned = self
            .inner
            .store
            .start_session_with_command_for_requirement_and_repositories(
                &session_id,
                &command_id,
                required_capabilities,
                requirement_id.as_deref(),
                repository_ids.as_deref(),
                move |_daemon_id, server_command_seq| {
                    ServerFrame::Command(CommandEnvelope {
                        command_id: envelope_command_id.clone(),
                        session_id: envelope_session_id.clone(),
                        server_command_seq,
                        sent_at: server_time(),
                        schema_version: SCHEMA_VERSION,
                        command: command.clone(),
                    })
                    .to_json()
                    .map_err(|_| PersistenceError::InvalidCommandPayload)
                },
            )
            .await
            .map_err(|error| match error {
                PersistenceError::NoEligibleDaemon => DaemonDispatchError::DaemonUnavailable,
                PersistenceError::InvalidCommandPayload => DaemonDispatchError::InvalidCommand,
                _ => DaemonDispatchError::Internal,
            })?;
        if pinned.compacted {
            return Ok(pinned);
        }
        let persisted = ServerFrame::from_json(&pinned.payload)
            .map_err(|_| DaemonDispatchError::InvalidCommand)?;
        let ServerFrame::Command(envelope) = &persisted else {
            return Err(DaemonDispatchError::InvalidCommand);
        };
        if envelope.command_id != pinned.command_id
            || envelope.session_id != pinned.session_id
            || envelope.server_command_seq != pinned.server_command_seq
        {
            return Err(DaemonDispatchError::InvalidCommand);
        }
        self.dispatch_persisted_command(&pinned, persisted).await?;
        Ok(pinned)
    }

    async fn assemble_session_command(
        &self,
        command: Command,
    ) -> Result<Command, DaemonDispatchError> {
        let Command::SessionStart(start) = command else {
            return Ok(command);
        };
        let repositories = self
            .inner
            .store
            .active_repositories()
            .await
            .map_err(|_| DaemonDispatchError::Internal)?;
        Self::assemble_session_repositories(start, repositories)
    }

    fn assemble_session_repositories(
        mut start: SessionStart,
        repositories: Vec<RepositoryRecord>,
    ) -> Result<Command, DaemonDispatchError> {
        let requested_ids = start
            .repositories
            .iter()
            .map(|repository| repository.repository_id.as_str())
            .collect::<Vec<_>>();
        let mut unique_ids = HashSet::with_capacity(requested_ids.len());
        if requested_ids
            .iter()
            .any(|repository_id| !unique_ids.insert(*repository_id))
            || requested_ids.iter().any(|repository_id| {
                !repositories
                    .iter()
                    .any(|repository| repository.id == *repository_id)
            })
        {
            return Err(DaemonDispatchError::InvalidCommand);
        }
        start.repositories = repositories
            .into_iter()
            .filter(|repository| {
                requested_ids.is_empty()
                    || requested_ids
                        .iter()
                        .any(|repository_id| repository.id == *repository_id)
            })
            .map(|repository| RepositoryContext {
                repository_id: repository.id,
                name: repository.name,
                url: repository.url,
                description: repository.description,
            })
            .collect();
        Ok(Command::SessionStart(start))
    }

    async fn dispatch_persisted_command(
        &self,
        pinned: &north_persistence::PinnedCommand,
        command: ServerFrame,
    ) -> Result<(), DaemonDispatchError> {
        let ServerFrame::Command(envelope) = &command else {
            return Err(DaemonDispatchError::InvalidCommand);
        };
        if envelope.command_id != pinned.command_id
            || envelope.session_id != pinned.session_id
            || envelope.server_command_seq != pinned.server_command_seq
        {
            return Err(DaemonDispatchError::InvalidCommand);
        }
        let daemon = self
            .inner
            .store
            .daemon_by_id(&pinned.daemon_id)
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
            .get(&pinned.daemon_id)
            .filter(|connection| connection.ready)
            .map(|connection| connection.outbound.clone())
            .ok_or(DaemonDispatchError::DaemonUnavailable)?;
        outbound
            .send(command)
            .await
            .map_err(|_| DaemonDispatchError::DaemonUnavailable)
    }

    async fn mark_live_ready(&self, daemon_id: &str, connection_id: &str) {
        let mut live = self.inner.live.lock().await;
        if let Some(connection) = live.get_mut(daemon_id) {
            if connection.connection_id == connection_id {
                connection.ready = true;
            }
        }
    }

    async fn validate_daemon_session(
        &self,
        session_id: &str,
        daemon_id: &str,
        connection_id: &str,
    ) -> Result<(), PersistenceError> {
        self.inner
            .store
            .touch_daemon(daemon_id, connection_id)
            .await?;
        let owner = self.inner.store.session_owner(session_id).await?;
        if owner.as_deref() == Some(daemon_id) {
            Ok(())
        } else {
            Err(PersistenceError::ProtocolIntegrity(
                "daemon session owner rejected".into(),
            ))
        }
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

fn same_retry_command(requested: &Command, persisted: &Command) -> bool {
    let mut requested = requested.clone();
    if let (Command::SessionStart(requested), Command::SessionStart(persisted)) =
        (&mut requested, persisted)
    {
        requested.repositories = persisted.repositories.clone();
    }
    requested == *persisted
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
        | PersistenceError::InvalidCommandPayload
        | PersistenceError::InvalidSessionState
        | PersistenceError::SessionRequirementMismatch
        | PersistenceError::NoEligibleDaemon
        | PersistenceError::InvalidRole(_)
        | PersistenceError::InvalidCode
        | PersistenceError::RateLimited
        | PersistenceError::ProtocolIntegrity(_)
        | PersistenceError::EventSequenceGap { .. } => DaemonHttpError::BadRequest,
        PersistenceError::Database(_) => DaemonHttpError::Internal,
        PersistenceError::InvalidRepository(_) => DaemonHttpError::BadRequest,
        PersistenceError::RepositoryNotFound => DaemonHttpError::NotFound,
        PersistenceError::RepositoryNameConflict | PersistenceError::RepositoryUrlImmutable => {
            DaemonHttpError::Conflict
        }
    }
}

fn wants_html(headers: &HeaderMap) -> bool {
    let Some(accept) = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let mut html = false;
    let mut json = false;
    for media_range in accept.split(',') {
        let media_type = media_range.split(';').next().unwrap_or("").trim();
        if media_type.eq_ignore_ascii_case("text/html") {
            html = true;
        }
        if media_type.eq_ignore_ascii_case("application/json") || media_type.ends_with("+json") {
            json = true;
        }
    }
    html && !json
}

fn render_setup_approval_page(request_token: &str, response: &SetupApprovalResponse) -> String {
    let action = escape_html(&format!("/daemon/setup/{request_token}/approve"));
    let label = escape_html(&response.label);
    let status = escape_html(&response.status);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Approve daemon connection</title>
</head>
<body>
<main>
<h1>Connect daemon to North</h1>
<p>North is asking to connect a daemon.</p>
<dl>
<dt>Daemon</dt><dd>{label}</dd>
<dt>Setup state</dt><dd>{status}</dd>
</dl>
<form method="POST" action="{action}">
<button type="submit">Approve</button>
</form>
<p><a href="/">Cancel / back</a></p>
</main>
</body>
</html>
"#
    )
}

fn render_setup_approved_page() -> &'static str {
    "<!doctype html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>Daemon approved</title></head>\n<body><main><h1>Daemon approved</h1><p>You may return to the terminal.</p></main></body>\n</html>\n"
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(origin_uri) = origin.parse::<Uri>() else {
        return false;
    };
    if !matches!(origin_uri.scheme_str(), Some("http" | "https"))
        || !(origin_uri.path().is_empty() || origin_uri.path() == "/")
        || origin_uri.query().is_some()
    {
        return false;
    }
    origin_uri
        .authority()
        .is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
}

fn server_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn session_repository_selection_validates_and_filters_ids() {
        let make_start = |repositories| SessionStart {
            requirement: north_protocol::RequirementContext {
                id: "requirement".into(),
                revision: 1,
                title: "Title".into(),
                description: "Description".into(),
                summary: "Summary".into(),
                acceptance_criteria: vec![],
                assumptions: vec![],
                open_questions: vec![],
            },
            conversation: north_protocol::ConversationContext { excerpt: vec![] },
            repositories,
        };
        let repository = |id: &str, name: &str| RepositoryRecord {
            id: id.into(),
            name: name.into(),
            name_normalized: name.to_lowercase(),
            url: format!("https://example.test/{id}.git"),
            description: format!("{name} description"),
            created_at: "created".into(),
            updated_at: "updated".into(),
            disabled_at: None,
        };
        let first = repository("repo-1", "First");
        let second = repository("repo-2", "Second");

        let command = DaemonRuntime::assemble_session_repositories(
            make_start(vec![RepositoryContext {
                repository_id: "repo-1".into(),
                name: "stale name".into(),
                url: "https://stale.example/repo.git".into(),
                description: "stale description".into(),
            }]),
            vec![first.clone(), second.clone()],
        )
        .expect("known repository selection");
        let Command::SessionStart(start) = command else {
            panic!("expected session start");
        };
        assert_eq!(start.repositories.len(), 1);
        assert_eq!(start.repositories[0].repository_id, "repo-1");
        assert_eq!(start.repositories[0].name, "First");
        assert_eq!(start.repositories[0].url, "https://example.test/repo-1.git");

        let command = DaemonRuntime::assemble_session_repositories(
            make_start(vec![]),
            vec![first.clone(), second.clone()],
        )
        .expect("empty selection means all active repositories");
        let Command::SessionStart(start) = command else {
            panic!("expected session start");
        };
        assert_eq!(start.repositories.len(), 2);

        assert!(matches!(
            DaemonRuntime::assemble_session_repositories(
                make_start(vec![
                    RepositoryContext {
                        repository_id: "repo-1".into(),
                        name: "First".into(),
                        url: "https://example.test/repo-1.git".into(),
                        description: "First description".into(),
                    },
                    RepositoryContext {
                        repository_id: "repo-1".into(),
                        name: "First".into(),
                        url: "https://example.test/repo-1.git".into(),
                        description: "First description".into(),
                    },
                ]),
                vec![first.clone(), second.clone()],
            ),
            Err(DaemonDispatchError::InvalidCommand)
        ));
        assert!(matches!(
            DaemonRuntime::assemble_session_repositories(
                make_start(vec![RepositoryContext {
                    repository_id: "missing".into(),
                    name: "Missing".into(),
                    url: "https://example.test/missing.git".into(),
                    description: "Missing description".into(),
                }]),
                vec![first, second],
            ),
            Err(DaemonDispatchError::InvalidCommand)
        ));
    }

    #[test]
    fn approval_content_negotiation_prefers_explicit_json() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml"),
        );
        assert!(wants_html(&headers));
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        assert!(!wants_html(&headers));
    }

    #[test]
    fn approval_html_escapes_label_and_token() {
        let html = render_setup_approval_page(
            "token\"&",
            &SetupApprovalResponse {
                status: "pending".into(),
                label: "<daemon>".into(),
            },
        );
        assert!(html.contains("&lt;daemon&gt;"));
        assert!(html.contains(r#"token&quot;&amp;"#));
        assert!(!html.contains("<daemon>"));
        assert!(!html.contains("credential"));
    }

    #[test]
    fn same_origin_requires_matching_request_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("north.test"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://north.test"),
        );
        assert!(same_origin(&headers));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!same_origin(&headers));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://north.test/setup"),
        );
        assert!(!same_origin(&headers));
        headers.remove(header::ORIGIN);
        assert!(!same_origin(&headers));
    }
}
