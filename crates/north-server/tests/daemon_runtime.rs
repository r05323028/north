use axum::{
    body::{to_bytes, Body},
    http::{header, HeaderValue, Method, Request, StatusCode},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use north_domain::{requirement::RequirementEdit, role::Role, status::RequirementStatus};
use north_persistence::{
    AuthStore, PersistenceError, PoolOptions, DAEMON_SETUP_CLEANUP_BATCH_SIZE,
};
use north_protocol::{
    encode_daemon_frame, Command, CommandAck, DaemonFrame, Event, EventAckStatus, EventEnvelope,
    Heartbeat, MessageSend, ProtocolErrorFrame, ReadinessVerdictWire, RequirementAssessed,
    RequirementContext, ReviewedRepositoryWire, ServerFrame, SessionStart, SCHEMA_VERSION,
};
use north_server::{
    auth_router, build_app, AuthState, CommandRequest, DaemonResponse, LogCodeDelivery,
    SetupApprovalResponse,
};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    env,
    sync::{Arc, OnceLock},
    time::Duration,
};
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

static DATABASE_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

async fn database_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    DATABASE_TEST_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

async fn response_json<T: DeserializeOwned>(response: Response) -> T {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn response_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    String::from_utf8(bytes.to_vec()).expect("UTF-8 response")
}

fn cookie(token: &str) -> String {
    format!("north_session={token}")
}

fn request(method: Method, uri: &str, session: Option<&str>, body: Body) -> Request<Body> {
    request_with_origin(method, uri, session, body, Some("http://north.test"))
}

fn request_with_accept(
    method: Method,
    uri: &str,
    session: Option<&str>,
    body: Body,
    accept: &str,
) -> Request<Body> {
    let mut request = request(method, uri, session, body);
    request.headers_mut().insert(
        header::ACCEPT,
        HeaderValue::from_str(accept).expect("accept header"),
    );
    request
}

fn request_with_origin(
    method: Method,
    uri: &str,
    session: Option<&str>,
    body: Body,
    origin: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "north.test");
    if let Some(session) = session {
        builder = builder.header("cookie", cookie(session));
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
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
    let _database_test_guard = database_test_lock().await;
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
        .oneshot(request_with_accept(
            Method::GET,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
            "text/html",
        ))
        .await
        .expect("browser approval page response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    let html = response_text(response).await;
    assert!(html.contains("Connect daemon to North"));
    assert!(html.contains("North is asking to connect a daemon."));
    assert!(html.contains("integration daemon"));
    assert!(html.contains("Setup state</dt><dd>pending"));
    assert!(html.contains(&format!(
        "<form method=\"POST\" action=\"{}\">",
        created.verification_path
    )));
    assert!(html.contains(">Approve</button>"));
    assert!(html.contains("Cancel / back"));
    assert!(!html.contains("credential"));

    let response = app
        .clone()
        .oneshot(request_with_accept(
            Method::GET,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
            "application/json",
        ))
        .await
        .expect("API approval preview response");
    assert_eq!(response.status(), StatusCode::OK);
    let preview: SetupApprovalResponse = response_json(response).await;
    assert_eq!(preview.status, "pending");
    assert_eq!(preview.label, "integration daemon");

    let mut cross_site_get = request_with_origin(
        Method::GET,
        &created.verification_path,
        Some(&admin.token),
        Body::empty(),
        Some("https://evil.example"),
    );
    cross_site_get
        .headers_mut()
        .insert(header::ACCEPT, HeaderValue::from_static("text/html"));
    let response = app
        .clone()
        .oneshot(cross_site_get)
        .await
        .expect("cross-site approval preview response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = response_text(response).await;
    assert!(html.contains("integration daemon"));
    assert!(html.contains("Setup state</dt><dd>pending"));
    assert!(!html.contains("credential"));

    let response = app
        .clone()
        .oneshot(request_with_accept(
            Method::GET,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
            "application/json",
        ))
        .await
        .expect("read-only state response");
    let preview: SetupApprovalResponse = response_json(response).await;
    assert_eq!(preview.status, "pending");

    let response = app
        .clone()
        .oneshot(request_with_origin(
            Method::POST,
            &created.verification_path,
            None,
            Body::empty(),
            Some("http://north.test"),
        ))
        .await
        .expect("unauthenticated approval response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(request_with_origin(
            Method::POST,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
            Some("https://evil.example"),
        ))
        .await
        .expect("cross-site approval response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/not-a-real-token/approve",
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("invalid approval response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let expired = store
        .create_daemon_setup_request("expired integration daemon")
        .await
        .expect("create expired setup request");
    sqlx::query(
        "UPDATE daemon_setup_requests\n         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute'\n         WHERE label = $1",
    )
    .bind(&expired.label)
    .execute(&pool)
    .await
    .expect("expire setup request");
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!("/daemon/setup/{}/approve", expired.request_token),
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("expired approval response");
    assert_eq!(response.status(), StatusCode::GONE);

    let response = app
        .clone()
        .oneshot(request_with_accept(
            Method::POST,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
            "text/html",
        ))
        .await
        .expect("browser approval response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    let html = response_text(response).await;
    assert!(html.contains("Daemon approved"));
    assert!(html.contains("You may return to the terminal."));
    assert!(!html.contains("credential"));

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("already approved response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

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
            Method::POST,
            &created.verification_path,
            Some(&admin.token),
            Body::empty(),
        ))
        .await
        .expect("already claimed approval response");
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
    let command = CommandRequest {
        command_id: command_id.clone(),
        session_id: session_id.clone(),
        command: Command::MessageSend(MessageSend {
            message_id: "message-1".into(),
            content: "hello daemon".into(),
        }),
    };
    let pinned = runtime
        .persist_and_dispatch_command(command, std::slice::from_ref(&required_capability))
        .await
        .expect("pin, persist, and dispatch first command");
    assert_eq!(pinned.daemon_id, claimed.daemon_id);
    let owner_before_revoke = store
        .session_owner(&session_id)
        .await
        .expect("session owner")
        .expect("pinned session");
    let received = next_server_frame(&mut socket).await;
    let stored_payload: String =
        sqlx::query_scalar("SELECT payload FROM server_command_outbox WHERE command_id = $1")
            .bind(&command_id)
            .fetch_one(&pool)
            .await
            .expect("read persisted command");
    let persisted = ServerFrame::from_json(&stored_payload).expect("decode persisted command");
    assert_eq!(persisted, received);
    let ServerFrame::Command(envelope) = received else {
        panic!("expected dispatched command");
    };
    assert_eq!(envelope.command_id, command_id);
    assert_eq!(envelope.session_id, session_id);
    assert_eq!(envelope.server_command_seq, pinned.server_command_seq);

    let assessment_requirement = store
        .create_requirement(
            "Daemon assessment",
            "Only its bound daemon session may assess this requirement",
            &requester.user.id,
        )
        .await
        .expect("create assessment requirement");
    let assessment_requirement_id = assessment_requirement.id.clone();
    store
        .transition_requirement(
            &assessment_requirement_id,
            1,
            &requester.user.id,
            north_persistence::RequirementTransition::BeginDiscussion,
        )
        .await
        .expect("begin assessment requirement");
    let assessment_requirement = store
        .edit_requirement(
            &assessment_requirement_id,
            1,
            &RequirementEdit {
                acceptance_criteria: Some(vec!["The assessment is session-bound".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("add assessment criteria");
    assert_eq!(assessment_requirement.revision, 2);

    let assessment_session_id = format!("assessment-session-{}", claimed.daemon_id);
    let assessment_command_id = format!("assessment-command-{}", claimed.daemon_id);
    let assessment_command = CommandRequest {
        command_id: assessment_command_id,
        session_id: assessment_session_id.clone(),
        command: Command::SessionStart(SessionStart {
            requirement: RequirementContext {
                id: assessment_requirement.id.clone(),
                revision: assessment_requirement.revision,
                title: assessment_requirement.title.clone(),
                description: assessment_requirement.description.clone(),
                summary: assessment_requirement.summary.clone(),
                acceptance_criteria: assessment_requirement.acceptance_criteria.clone(),
                assumptions: assessment_requirement.assumptions.clone(),
                open_questions: assessment_requirement.open_questions.clone(),
            },
            conversation: north_protocol::ConversationContext { excerpt: vec![] },
            repositories: vec![],
        }),
    };
    runtime
        .persist_and_dispatch_command(
            assessment_command,
            std::slice::from_ref(&required_capability),
        )
        .await
        .expect("bind assessment session");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Command(_)
    ));
    assert_eq!(
        store
            .session_requirement(&assessment_session_id)
            .await
            .expect("assessment session binding"),
        Some(assessment_requirement_id.clone())
    );

    let assessment_event_id = format!("assessment-event-{}", claimed.daemon_id);
    let assessment_event = DaemonFrame::Event(EventEnvelope {
        event_id: assessment_event_id.clone(),
        session_id: assessment_session_id.clone(),
        daemon_event_seq: 1,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::RequirementAssessed(RequirementAssessed {
            requirement_id: assessment_requirement_id.clone(),
            requirement_revision: 2,
            verdict: ReadinessVerdictWire::Ready,
            blockers: vec![],
            assumptions: vec!["Daemon owns session".into()],
            repositories_reviewed: vec![ReviewedRepositoryWire {
                repository_id: "north".into(),
                commit_sha: "abc123".into(),
            }],
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&assessment_event)
                .expect("encode assessment event")
                .into(),
        ))
        .await
        .expect("send assessment event");
    let ServerFrame::EventAck(ack) = next_server_frame(&mut socket).await else {
        panic!("expected accepted assessment ACK");
    };
    assert_eq!(ack.status, EventAckStatus::Accepted);
    let assessed = store
        .requirement_by_id(&assessment_requirement_id)
        .await
        .expect("read assessed requirement")
        .expect("assessed requirement");
    assert_eq!(assessed.status, RequirementStatus::Ready);

    socket
        .send(Message::Text(
            encode_daemon_frame(&assessment_event)
                .expect("encode duplicate assessment event")
                .into(),
        ))
        .await
        .expect("send duplicate assessment event");
    let ServerFrame::EventAck(duplicate_ack) = next_server_frame(&mut socket).await else {
        panic!("expected duplicate assessment ACK");
    };
    assert_eq!(duplicate_ack.status, EventAckStatus::Accepted);
    let assessment_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments WHERE event_id = $1")
            .bind(&assessment_event_id)
            .fetch_one(&pool)
            .await
            .expect("count daemon assessments");
    assert_eq!(assessment_rows, 1);

    store
        .edit_requirement(
            &assessment_requirement_id,
            2,
            &RequirementEdit {
                summary: Some("Changed after daemon assessment".into()),
                ..Default::default()
            },
        )
        .await
        .expect("demote assessed requirement");
    let stale_event = DaemonFrame::Event(EventEnvelope {
        event_id: format!("{assessment_event_id}-stale"),
        session_id: assessment_session_id.clone(),
        daemon_event_seq: 2,
        sent_at: "2026-01-01T00:00:01Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::RequirementAssessed(RequirementAssessed {
            requirement_id: assessment_requirement_id.clone(),
            requirement_revision: 2,
            verdict: ReadinessVerdictWire::Ready,
            blockers: vec![],
            assumptions: vec![],
            repositories_reviewed: vec![],
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&stale_event)
                .expect("encode stale assessment event")
                .into(),
        ))
        .await
        .expect("send stale assessment event");
    let ServerFrame::EventAck(stale_ack) = next_server_frame(&mut socket).await else {
        panic!("expected stale assessment ACK");
    };
    assert_eq!(stale_ack.status, EventAckStatus::Rejected);
    let assessed = store
        .requirement_by_id(&assessment_requirement_id)
        .await
        .expect("read stale assessed requirement")
        .expect("stale assessed requirement");
    assert_eq!(assessed.status, RequirementStatus::Discussing);
    assert_eq!(assessed.revision, 3);

    let foreign_requirement = store
        .create_requirement(
            "Foreign assessment",
            "Session binding must reject this target",
            &requester.user.id,
        )
        .await
        .expect("create foreign requirement");
    store
        .transition_requirement(
            &foreign_requirement.id,
            1,
            &requester.user.id,
            north_persistence::RequirementTransition::BeginDiscussion,
        )
        .await
        .expect("begin foreign requirement");
    let foreign_requirement = store
        .edit_requirement(
            &foreign_requirement.id,
            1,
            &RequirementEdit {
                acceptance_criteria: Some(vec!["Foreign criterion".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("add foreign criteria");
    let foreign_event = DaemonFrame::Event(EventEnvelope {
        event_id: format!("{assessment_event_id}-foreign"),
        session_id: assessment_session_id,
        daemon_event_seq: 3,
        sent_at: "2026-01-01T00:00:02Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::RequirementAssessed(RequirementAssessed {
            requirement_id: foreign_requirement.id.clone(),
            requirement_revision: foreign_requirement.revision,
            verdict: ReadinessVerdictWire::Ready,
            blockers: vec![],
            assumptions: vec![],
            repositories_reviewed: vec![],
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&foreign_event)
                .expect("encode foreign assessment event")
                .into(),
        ))
        .await
        .expect("send foreign assessment event");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::ProtocolError(ProtocolErrorFrame { code, .. })
            if code == "assessment_requirement_mismatch"
    ));
    let foreign_state = store
        .requirement_by_id(&foreign_requirement.id)
        .await
        .expect("read foreign requirement")
        .expect("foreign requirement");
    assert_eq!(foreign_state.status, RequirementStatus::Discussing);
    drop(socket);
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("reconnect after assessment rejection");
    socket
        .send(Message::Text(
            encode_daemon_frame(&hello(&claimed.daemon_id, &claimed.credential))
                .expect("encode post-assessment hello")
                .into(),
        ))
        .await
        .expect("send post-assessment hello");
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Welcome(_)
    ));
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Reconcile(_)
    ));

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
    let second_capability = format!("test:{}", second_claimed.daemon_id);
    let manual_connection = store
        .connect_daemon(
            &second_claimed.daemon_id,
            &second_claimed.credential,
            "0.1",
            std::slice::from_ref(&second_capability),
        )
        .await
        .expect("mark second daemon connected");
    let failing_runtime = north_server::DaemonRuntime::new(store.clone());
    let failed_command = Command::MessageSend(MessageSend {
        message_id: "dispatch-failure-message".into(),
        content: "must remain durable".into(),
    });
    let failed_request = CommandRequest {
        command_id: format!("dispatch-failure-command-{}", second_claimed.daemon_id),
        session_id: format!("dispatch-failure-session-{}", second_claimed.daemon_id),
        command: failed_command.clone(),
    };
    let failure = failing_runtime
        .persist_and_dispatch_command(failed_request, std::slice::from_ref(&second_capability))
        .await;
    assert!(matches!(
        failure,
        Err(north_server::DaemonDispatchError::DaemonUnavailable)
    ));
    let (stored_daemon_id, stored_seq, stored_payload): (String, i64, String) = sqlx::query_as(
        "SELECT daemon_id, server_command_seq, payload\n         FROM server_command_outbox WHERE command_id = $1",
    )
    .bind(format!("dispatch-failure-command-{}", second_claimed.daemon_id))
    .fetch_one(&pool)
    .await
    .expect("read command persisted before failed dispatch");
    let persisted = ServerFrame::from_json(&stored_payload).expect("decode failed command");
    let ServerFrame::Command(envelope) = persisted else {
        panic!("expected persisted command envelope");
    };
    assert_eq!(stored_daemon_id, second_claimed.daemon_id);
    assert_eq!(
        u64::try_from(stored_seq).expect("positive sequence"),
        envelope.server_command_seq
    );
    assert_eq!(envelope.command, failed_command);
    store
        .disconnect_daemon(&second_claimed.daemon_id, &manual_connection.connection_id)
        .await
        .expect("clear manually connected daemon");

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
            std::slice::from_ref(&required_capability),
            |_, _| Ok("{}".into()),
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

    sqlx::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(format!("assessment-session-{}", claimed.daemon_id))
        .execute(&pool)
        .await
        .expect("cleanup assessment session");
    sqlx::query("DELETE FROM requirements WHERE id IN ($1, $2)")
        .bind(&assessment_requirement_id)
        .bind(&foreign_requirement.id)
        .execute(&pool)
        .await
        .expect("cleanup assessment requirements");

    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn server_restart_invalidates_stale_daemon_lease_and_cleans_setup_rows() {
    let database_url = match env::var("NORTH_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => panic!("NORTH_TEST_DATABASE_URL is required for daemon integration tests"),
    };
    let _database_test_guard = database_test_lock().await;
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_server::run_migrations(&pool)
        .await
        .expect("run migrations");
    let store = AuthStore::new(pool.clone());

    let email = unique_email("restart-admin");
    store
        .issue_code(&email, "333333")
        .await
        .expect("issue admin code");
    let admin = store
        .verify_code(&email, "333333")
        .await
        .expect("verify admin code");
    store
        .update_user_role(&admin.user.id, Role::Admin)
        .await
        .expect("promote admin")
        .expect("admin exists");

    let app = auth_router(AuthState::with_log_delivery(store.clone()));
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/request",
            None,
            Body::from(r#"{"label":"restart daemon"}"#),
        ))
        .await
        .expect("setup request response");
    let created: north_server::daemon::SetupCreatedResponse = response_json(response).await;
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
    let claimed: SetupClaimed = response_json(response).await;

    let recent_label = unique_email("recent-cleanup");
    let recent = store
        .create_daemon_setup_request(&recent_label)
        .await
        .expect("create recent setup request");
    let expired_label = unique_email("expired-cleanup");
    let expired = store
        .create_daemon_setup_request(&expired_label)
        .await
        .expect("create expired setup request");
    sqlx::query(
        "UPDATE daemon_setup_requests\n         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '2 days'\n         WHERE label = $1",
    )
    .bind(&expired.label)
    .execute(&pool)
    .await
    .expect("age setup request");
    let deleted = store
        .cleanup_expired_daemon_setup_requests()
        .await
        .expect("cleanup expired setup requests");
    assert!(deleted >= 1);
    let expired_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM daemon_setup_requests WHERE label = $1")
            .bind(&expired.label)
            .fetch_one(&pool)
            .await
            .expect("count expired rows");
    let recent_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM daemon_setup_requests WHERE label = $1")
            .bind(&recent.label)
            .fetch_one(&pool)
            .await
            .expect("count recent rows");
    assert_eq!(expired_count, 0);
    assert_eq!(recent_count, 1);

    let batch_prefix = unique_email("expired-cleanup-batch");
    for index in 0..=DAEMON_SETUP_CLEANUP_BATCH_SIZE {
        let label = format!("{batch_prefix}-{index}");
        store
            .create_daemon_setup_request(&label)
            .await
            .expect("create batch cleanup setup request");
    }
    sqlx::query(
        "UPDATE daemon_setup_requests
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '2 days'
         WHERE label LIKE $1",
    )
    .bind(format!("{batch_prefix}-%"))
    .execute(&pool)
    .await
    .expect("age batch cleanup setup requests");
    let first_batch_deleted = store
        .cleanup_expired_daemon_setup_requests()
        .await
        .expect("run bounded cleanup");
    assert_eq!(
        first_batch_deleted,
        u64::try_from(DAEMON_SETUP_CLEANUP_BATCH_SIZE).expect("positive cleanup batch")
    );
    let remaining_after_first: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM daemon_setup_requests WHERE label LIKE $1")
            .bind(format!("{batch_prefix}-%"))
            .fetch_one(&pool)
            .await
            .expect("count remaining batch rows");
    assert_eq!(remaining_after_first, 1);
    let second_batch_deleted = store
        .cleanup_expired_daemon_setup_requests()
        .await
        .expect("finish bounded cleanup");
    assert_eq!(second_batch_deleted, 1);
    let remaining_after_second: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM daemon_setup_requests WHERE label LIKE $1")
            .bind(format!("{batch_prefix}-%"))
            .fetch_one(&pool)
            .await
            .expect("count cleaned batch rows");
    assert_eq!(remaining_after_second, 0);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve first runtime");
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("connect first daemon");
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
    assert!(
        store
            .daemon_by_id(&claimed.daemon_id)
            .await
            .expect("daemon lookup")
            .expect("daemon record")
            .connected
    );

    server.abort();
    drop(socket);
    let restarted_app = build_app(pool.clone(), Arc::new(LogCodeDelivery))
        .await
        .expect("build restarted app");
    let restart_only_capability = format!("restart-only:{}", claimed.daemon_id);
    let stale = store
        .start_session_with_command(
            &format!("restart-stale-session-{}", claimed.daemon_id),
            &format!("restart-stale-command-{}", claimed.daemon_id),
            std::slice::from_ref(&restart_only_capability),
            |_, _| Ok("{}".into()),
        )
        .await;
    assert!(matches!(stale, Err(PersistenceError::NoEligibleDaemon)));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind restarted server");
    let address = listener.local_addr().expect("restarted server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, restarted_app)
            .await
            .expect("serve restarted runtime");
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/daemon/ws"))
        .await
        .expect("connect reconnected daemon");
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
    assert!(matches!(
        next_server_frame(&mut socket).await,
        ServerFrame::Reconcile(_)
    ));
    let mut connected = false;
    for _ in 0..20 {
        if store
            .daemon_by_id(&claimed.daemon_id)
            .await
            .expect("daemon lookup")
            .expect("daemon record")
            .connected
        {
            connected = true;
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(connected, "reconnect did not restore daemon eligibility");
    let pinned = store
        .start_session_with_command(
            &format!("restart-live-session-{}", claimed.daemon_id),
            &format!("restart-live-command-{}", claimed.daemon_id),
            std::slice::from_ref(&format!("test:{}", claimed.daemon_id)),
            |_, _| Ok(r#"{"stored":"after-reconnect"}"#.into()),
        )
        .await
        .expect("place session after reconnect");
    assert_eq!(pinned.daemon_id, claimed.daemon_id);

    drop(socket);
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
