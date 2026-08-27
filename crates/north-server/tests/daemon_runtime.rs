use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use north_domain::role::Role;
use north_persistence::{AuthStore, PersistenceError, PoolOptions};
use north_protocol::{
    encode_daemon_frame, Command, CommandAck, CommandEnvelope, DaemonFrame, Heartbeat, MessageSend,
    ProtocolErrorFrame, ServerFrame, SCHEMA_VERSION,
};
use north_server::{auth_router, AuthState, DaemonResponse};
use serde::{de::DeserializeOwned, Deserialize};
use std::{env, time::Duration};
use tokio::{
    net::TcpListener,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream};
use tower::ServiceExt;

#[derive(Debug, Deserialize)]
struct SetupClaimed {
    status: String,
    daemon_id: String,
    credential: String,
}

async fn response_json<T: DeserializeOwned>(response: Response) -> T {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

fn cookie(token: &str) -> String {
    format!("north_session={token}")
}

fn request(method: Method, uri: &str, session: Option<&str>, body: Body) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(session) = session {
        builder = builder.header("cookie", cookie(session));
    }
    builder
        .header("content-type", "application/json")
        .body(body)
        .expect("request")
}

fn unique_email(prefix: &str) -> String {
    format!(
        "{prefix}-{}@example.com",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

async fn next_server_frame<S>(socket: &mut WebSocketStream<S>) -> ServerFrame
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("WebSocket frame deadline")
            .expect("WebSocket frame")
            .expect("WebSocket transport");
        match message {
            Message::Text(text) => {
                return ServerFrame::from_json(text.as_ref()).expect("server frame")
            }
            Message::Ping(payload) => {
                socket.send(Message::Pong(payload)).await.expect("pong");
            }
            Message::Close(_) => panic!("server closed before protocol frame"),
            other => panic!("unexpected server message: {other:?}"),
        }
    }
}

fn hello(daemon_id: &str, credential: &str) -> DaemonFrame {
    DaemonFrame::Hello(north_protocol::Hello::new(
        daemon_id,
        credential,
        vec!["agent".into(), format!("test:{daemon_id}")],
    ))
}

fn heartbeat(daemon_id: &str) -> DaemonFrame {
    DaemonFrame::Heartbeat(Heartbeat {
        schema_version: SCHEMA_VERSION,
        daemon_id: daemon_id.into(),
        sent_at: format!("{:?}", std::time::SystemTime::now()),
        application_state: "connected".into(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn daemon_setup_connection_liveness_and_revocation_are_server_owned() {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for daemon integration tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_server::run_migrations(&pool)
        .await
        .expect("run migrations");
    let store = AuthStore::new(pool.clone());

    let admin_email = unique_email("daemon-admin");
    store
        .issue_code(&admin_email, "111111")
        .await
        .expect("issue admin code");
    let admin = store
        .verify_code(&admin_email, "111111")
        .await
        .expect("verify admin code");
    store
        .update_user_role(&admin.user.id, Role::Admin)
        .await
        .expect("promote admin")
        .expect("admin exists");

    let requester_email = unique_email("daemon-requester");
    store
        .issue_code(&requester_email, "222222")
        .await
        .expect("issue requester code");
    let requester = store
        .verify_code(&requester_email, "222222")
        .await
        .expect("verify requester code");
    store
        .update_user_role(&requester.user.id, Role::Requester)
        .await
        .expect("set requester role")
        .expect("requester exists");

    let state = AuthState::with_log_delivery(store.clone());
    let runtime = state.daemon_runtime().clone();
    let app = auth_router(state);
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/request",
            None,
            Body::from(r#"{"label":"integration daemon"}"#),
        ))
        .await
        .expect("setup request response");
    assert_eq!(response.status(), StatusCode::OK);
    let created: north_server::daemon::SetupCreatedResponse = response_json(response).await;

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/daemon/setup/{}", created.request_token),
            None,
            Body::empty(),
        ))
        .await
        .expect("pending response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("approval response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/daemon/setup/{}", created.request_token),
            None,
            Body::empty(),
        ))
        .await
        .expect("claim response");
    assert_eq!(response.status(), StatusCode::OK);
    let claimed: SetupClaimed = response_json(response).await;
    assert_eq!(claimed.status, "claimed");
    assert_ne!(claimed.credential, created.request_token);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/daemon/setup/{}", created.request_token),
            None,
            Body::empty(),
        ))
        .await
        .expect("second claim response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/daemons",
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("daemon list response");
    assert_eq!(response.status(), StatusCode::OK);
    let daemons: Vec<DaemonResponse> = response_json(response).await;
    assert!(daemons
        .iter()
        .any(|daemon| daemon.daemon_id == claimed.daemon_id));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, server_app)
            .await
            .expect("serve daemon runtime");
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("connect daemon");
    socket
        .send(Message::Text(
            encode_daemon_frame(&hello(&claimed.daemon_id, &claimed.credential))
                .expect("encode hello")
                .into(),
        ))
        .await
        .expect("send hello");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Welcome(_)
    ));
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Reconcile(_)
    ));

    let session_id = format!("daemon-session-{}", claimed.daemon_id);
    let command_id = format!("daemon-command-{}", claimed.daemon_id);
    let required_capability = format!("test:{}", claimed.daemon_id);
    let pinned = store
        .start_session_with_command(
            &session_id,
            &command_id,
            "{}",
            std::slice::from_ref(&required_capability),
        )
        .await
        .expect("pin session and persist first command");
    assert_eq!(pinned.daemon_id, claimed.daemon_id);
    let owner_before_revoke = store
        .session_owner(&session_id)
        .await
        .expect("session owner")
        .expect("pinned session");
    let command = ServerFrame::Command(CommandEnvelope {
        command_id: command_id.clone(),
        session_id: session_id.clone(),
        server_command_seq: pinned.server_command_seq,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        command: Command::MessageSend(MessageSend {
            message_id: "message-1".into(),
            content: "hello daemon".into(),
        }),
    });
    runtime
        .dispatch_command(command.clone())
        .await
        .expect("dispatch through pinned daemon");
    assert_eq!(next_server_frame(&mut socket).await, command);
    let second_response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/request",
            None,
            Body::from(r#"{"label":"second integration daemon"}"#),
        ))
        .await
        .expect("second setup request response");
    let second_created: north_server::daemon::SetupCreatedResponse =
        response_json(second_response).await;
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &second_created.verification_path,
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("second approval response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/daemon/setup/{}", second_created.request_token),
            None,
            Body::empty(),
        ))
        .await
        .expect("second claim response");
    let second_claimed: SetupClaimed = response_json(response).await;

    let (mut second_socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("connect second daemon");
    second_socket
        .send(Message::Text(
            encode_daemon_frame(&hello(
                &second_claimed.daemon_id,
                &second_claimed.credential,
            ))
            .expect("encode second hello")
            .into(),
        ))
        .await
        .expect("send second hello");
    assert!(matches!(
        next_server_frame(&mut second_socket).await,
        ServerFrame::Welcome(_)
    ));
    assert!(matches!(
        next_server_frame(&mut second_socket).await,
        ServerFrame::Reconcile(_)
    ));
    second_socket
        .send(Message::Text(
            encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                command_id: command_id.clone(),
                session_id: session_id.clone(),
                server_command_seq: pinned.server_command_seq,
                schema_version: SCHEMA_VERSION,
            }))
            .expect("encode foreign ACK")
            .into(),
        ))
        .await
        .expect("send foreign ACK");
    let frame = next_server_frame(&mut second_socket).await;
    assert!(matches!(
        frame,
        ServerFrame::ProtocolError(ProtocolErrorFrame { code, .. }) if code == "daemon_identity_mismatch"
    ));
    drop(second_socket);

    let last_seen_before = store
        .daemon_by_id(&claimed.daemon_id)
        .await
        .expect("daemon lookup")
        .expect("daemon record")
        .last_seen_at
        .expect("initial liveness");
    socket
        .send(Message::Text(
            encode_daemon_frame(&heartbeat(&claimed.daemon_id))
                .expect("encode heartbeat")
                .into(),
        ))
        .await
        .expect("send heartbeat");
    let mut heartbeat_seen = false;
    for _ in 0..20 {
        sleep(Duration::from_millis(10)).await;
        let current = store
            .daemon_by_id(&claimed.daemon_id)
            .await
            .expect("daemon lookup")
            .expect("daemon record");
        if current.last_seen_at.as_deref() != Some(last_seen_before.as_str()) {
            heartbeat_seen = true;
            break;
        }
    }
    assert!(heartbeat_seen, "heartbeat did not update last_seen_at");

    sqlx::query(
        "UPDATE daemon_registrations
         SET last_seen_at = CURRENT_TIMESTAMP - INTERVAL '1 minute'
         WHERE daemon_id = $1",
    )
    .bind(&claimed.daemon_id)
    .execute(&pool)
    .await
    .expect("age heartbeat");
    assert!(
        !store
            .daemon_by_id(&claimed.daemon_id)
            .await
            .expect("stale daemon lookup")
            .expect("stale daemon")
            .connected,
        "stale heartbeat must report offline"
    );
    let stale_session = store
        .start_session_with_command(
            &format!("stale-session-{}", claimed.daemon_id),
            &format!("stale-command-{}", claimed.daemon_id),
            "{}",
            std::slice::from_ref(&required_capability),
        )
        .await;
    assert!(matches!(
        stale_session,
        Err(PersistenceError::NoEligibleDaemon)
    ));

    drop(socket);
    for _ in 0..100 {
        sleep(Duration::from_millis(10)).await;
        if !store
            .daemon_by_id(&claimed.daemon_id)
            .await
            .expect("daemon lookup")
            .expect("daemon record")
            .connected
        {
            break;
        }
    }
    assert_eq!(
        store
            .session_owner(&session_id)
            .await
            .expect("owner while offline"),
        Some(owner_before_revoke.clone())
    );
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("reconnect daemon");
    socket
        .send(Message::Text(
            encode_daemon_frame(&hello(&claimed.daemon_id, &claimed.credential))
                .expect("encode reconnect hello")
                .into(),
        ))
        .await
        .expect("send reconnect hello");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Welcome(_)
    ));
    let frame = next_server_frame(&mut socket).await;
    let ServerFrame::Reconcile(snapshot) = frame else {
        panic!("expected reconnect reconciliation");
    };
    assert!(snapshot
        .sessions
        .iter()
        .any(|session| session.session_id == session_id));

    let response = app_for_request(&app, Method::POST, &claimed.daemon_id, &requester.token).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app_for_request(&app, Method::POST, &claimed.daemon_id, &admin.token).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        store
            .session_owner(&session_id)
            .await
            .expect("owner after revoke"),
        Some(owner_before_revoke)
    );
    let revoked = store
        .daemon_by_id(&claimed.daemon_id)
        .await
        .expect("revoked daemon lookup")
        .expect("revoked daemon");
    assert!(revoked.revoked_at.is_some());
    assert!(!revoked.connected);

    let frame = next_server_frame(&mut socket).await;
    assert!(matches!(
        frame,
        ServerFrame::ProtocolError(ProtocolErrorFrame { code, .. }) if code == "daemon_access_revoked"
    ));

    let (mut rejected, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("reconnect socket");
    rejected
        .send(Message::Text(
            encode_daemon_frame(&hello(&claimed.daemon_id, &claimed.credential))
                .expect("encode rejected hello")
                .into(),
        ))
        .await
        .expect("send rejected hello");
    let frame = next_server_frame(&mut rejected).await;
    assert!(matches!(
        frame,
        ServerFrame::ProtocolError(ProtocolErrorFrame { code, .. }) if code == "revoked_credential"
    ));

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/request",
            None,
            Body::from(r#"{"label":"owner integration daemon"}"#),
        ))
        .await
        .expect("owner setup request response");
    assert_eq!(response.status(), StatusCode::OK);
    let owner_created: north_server::daemon::SetupCreatedResponse = response_json(response).await;

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &owner_created.verification_path,
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("owner approval response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/daemon/setup/{}", owner_created.request_token),
            None,
            Body::empty(),
        ))
        .await
        .expect("owner claim response");
    assert_eq!(response.status(), StatusCode::OK);
    let owner_claimed: SetupClaimed = response_json(response).await;

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/daemons",
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("owner daemon list response");
    assert_eq!(response.status(), StatusCode::OK);
    let owned_daemons: Vec<DaemonResponse> = response_json(response).await;
    assert!(owned_daemons
        .iter()
        .any(|daemon| daemon.daemon_id == owner_claimed.daemon_id));
    assert!(owned_daemons
        .iter()
        .all(|daemon| daemon.created_by == requester.user.id));

    let response = app_for_request(
        &app,
        Method::POST,
        &owner_claimed.daemon_id,
        &requester.token,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    server.abort();
}

async fn app_for_request(
    app: &axum::Router,
    method: Method,
    daemon_id: &str,
    token: &str,
) -> Response {
    app.clone()
        .oneshot(request(
            method,
            &format!("/daemons/{daemon_id}/revoke"),
            Some(token),
            Body::empty(),
        ))
        .await
        .expect("revoke response")
}
