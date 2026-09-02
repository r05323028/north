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
    Heartbeat, MessageSend, ProtocolErrorFrame, ReadinessVerdictWire, RepositoryContext,
    RequirementAssessed, RequirementContext, ReviewedRepositoryWire, ServerFrame, SessionStart,
    SCHEMA_VERSION,
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
    sqlx::query(
        "INSERT INTO repositories (id, name, name_normalized, url, description)
         VALUES ('00000000-0000-4000-8000-000000000001', 'North', 'north', 'https://example.test/north.git', '')
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("seed configured repository");

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
            2,
            &RequirementEdit {
                acceptance_criteria: Some(vec!["The assessment is session-bound".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("add assessment criteria");
    assert_eq!(assessment_requirement.revision, 2);
    assert_eq!(assessment_requirement.state_version, 3);

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
            repositories: vec![RepositoryContext {
                repository_id: "00000000-0000-4000-8000-000000000001".into(),
                name: "requested metadata is replaced".into(),
                url: "https://example.test/north.git".into(),
                description: "requested metadata is replaced".into(),
            }],
        }),
    };
    runtime
        .persist_and_dispatch_command(
            assessment_command,
            std::slice::from_ref(&required_capability),
        )
        .await
        .expect("bind assessment session");
    let ServerFrame::Command(envelope) = next_server_frame(&mut socket).await else {
        panic!("expected assembled session start");
    };
    let Command::SessionStart(start) = envelope.command else {
        panic!("expected session start command");
    };
    assert_eq!(start.repositories.len(), 1);
    assert_eq!(
        start.repositories[0].repository_id,
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(start.repositories[0].url, "https://example.test/north.git");
    assert_eq!(
        store
            .session_requirement(&assessment_session_id)
            .await
            .expect("assessment session binding"),
        Some(assessment_requirement_id.clone())
    );
    let follow_up_command_id = format!("assessment-follow-up-{}", claimed.daemon_id);
    let follow_up = CommandRequest {
        command_id: follow_up_command_id.clone(),
        session_id: assessment_session_id.clone(),
        command: Command::MessageSend(MessageSend {
            message_id: "assessment-follow-up-message".into(),
            content: "repository binding remains stable".into(),
        }),
    };
    runtime
        .persist_and_dispatch_command(follow_up, std::slice::from_ref(&required_capability))
        .await
        .expect("dispatch follow-up without retargeting repository context");
    let ServerFrame::Command(follow_up_envelope) = next_server_frame(&mut socket).await else {
        panic!("expected follow-up command");
    };
    assert_eq!(follow_up_envelope.command_id, follow_up_command_id);
    socket
        .send(Message::Text(
            encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                command_id: follow_up_envelope.command_id,
                session_id: follow_up_envelope.session_id,
                server_command_seq: follow_up_envelope.server_command_seq,
                schema_version: SCHEMA_VERSION,
            }))
            .expect("encode follow-up ACK")
            .into(),
        ))
        .await
        .expect("send follow-up ACK");

    store
        .disable_repository("00000000-0000-4000-8000-000000000001")
        .await
        .expect("disable in-flight repository");

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
                repository_id: "00000000-0000-4000-8000-000000000001".into(),
                commit_sha: "abcdef0123456789abcdef0123456789abcdef01".into(),
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
    assert_eq!(assessed.revision, 2);
    assert_eq!(assessed.state_version, 4);

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
    let duplicate_state = store
        .requirement_by_id(&assessment_requirement_id)
        .await
        .expect("read duplicate assessment requirement")
        .expect("duplicate assessment requirement");
    assert_eq!(duplicate_state.state_version, 4);

    store
        .edit_requirement(
            &assessment_requirement_id,
            4,
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
    assert_eq!(assessed.state_version, 5);

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
            2,
            &RequirementEdit {
                acceptance_criteria: Some(vec!["Foreign criterion".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("add foreign criteria");
    let foreign_event_id = format!("{assessment_event_id}-foreign");
    let foreign_event = DaemonFrame::Event(EventEnvelope {
        event_id: foreign_event_id.clone(),
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
    assert_eq!(foreign_state.revision, 2);
    assert_eq!(foreign_state.state_version, 3);
    let foreign_assessments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM readiness_assessments WHERE event_id = $1")
            .bind(&foreign_event_id)
            .fetch_one(&pool)
            .await
            .expect("count foreign assessments");
    assert_eq!(foreign_assessments, 0);
    let foreign_ready_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transition_audit
         WHERE requirement_id = $1 AND transition = 'mark_ready'",
    )
    .bind(&foreign_requirement.id)
    .fetch_one(&pool)
    .await
    .expect("count foreign readiness audits");
    assert_eq!(foreign_ready_audits, 0);
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
    for _ in 0..2 {
        let ServerFrame::Command(resent) = next_server_frame(&mut socket).await else {
            panic!("expected unacknowledged command replay");
        };
        socket
            .send(Message::Text(
                encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                    command_id: resent.command_id,
                    session_id: resent.session_id,
                    server_command_seq: resent.server_command_seq,
                    schema_version: SCHEMA_VERSION,
                }))
                .expect("encode replay ACK")
                .into(),
            ))
            .await
            .expect("send replay ACK");
    }
    for _ in 0..100 {
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM server_command_outbox
             WHERE daemon_id = $1 AND acknowledged_at IS NULL",
        )
        .bind(&claimed.daemon_id)
        .fetch_one(&pool)
        .await
        .expect("count replay work");
        if pending == 0 {
            break;
        }
        sleep(Duration::from_millis(5)).await;
    }
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM server_command_outbox
         WHERE daemon_id = $1 AND acknowledged_at IS NULL",
    )
    .bind(&claimed.daemon_id)
    .fetch_one(&pool)
    .await
    .expect("final count replay work");
    assert_eq!(pending, 0);

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

    sqlx::query(
        "DELETE FROM server_command_tombstones
         WHERE session_id = $1",
    )
    .bind(format!("assessment-session-{}", claimed.daemon_id))
    .execute(&pool)
    .await
    .expect("cleanup command tombstones");
    sqlx::query("DELETE FROM server_event_dedupe WHERE session_id = $1")
        .bind(format!("assessment-session-{}", claimed.daemon_id))
        .execute(&pool)
        .await
        .expect("cleanup event tombstones");
    sqlx::query("DELETE FROM server_message_command_map WHERE session_id = $1")
        .bind(format!("assessment-session-{}", claimed.daemon_id))
        .execute(&pool)
        .await
        .expect("cleanup message command map");
    sqlx::query("DELETE FROM execution_sessions WHERE id = $1")
        .bind(format!("assessment-session-{}", claimed.daemon_id))
        .execute(&pool)
        .await
        .expect("cleanup assessment session");
    sqlx::query("DELETE FROM requirements WHERE id = $1")
        .bind(&foreign_requirement.id)
        .execute(&pool)
        .await
        .expect("cleanup foreign requirement");

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn clarification_start_reuse_cancel_and_terminal_slot_release() {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for clarification integration tests");
    let _database_test_guard = database_test_lock().await;
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_server::run_migrations(&pool)
        .await
        .expect("run migrations");
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL",
    )
    .execute(&pool)
    .await
    .expect("clear daemon leases");

    let store = AuthStore::new(pool.clone());
    let email = unique_email("clarification-requester");
    store
        .issue_code(&email, "444444")
        .await
        .expect("issue requester code");
    let requester = store
        .verify_code(&email, "444444")
        .await
        .expect("verify requester code");
    let requirement = store
        .create_requirement(
            "Clarification lifecycle",
            "Need clarification",
            &requester.user.id,
        )
        .await
        .expect("create requirement");
    let first = store
        .post_requester_message(&requirement.id, &requester.user.id, "first question")
        .await
        .expect("persist first message");
    let app = auth_router(AuthState::with_log_delivery(store.clone()));
    let start_uri = format!("/requirements/{}/clarification/start", requirement.id);
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &start_uri,
            Some(&requester.token),
            Body::from(
                serde_json::json!({
                    "message_id": first.id,
                    "expected_state_version": 1
                })
                .to_string(),
            ),
        ))
        .await
        .expect("first clarification start response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let first_body: serde_json::Value = response_json(response).await;
    assert_eq!(first_body["error"], "clarification_unavailable");
    assert_eq!(first_body["session"]["phase"], "awaiting_assignment");
    assert_eq!(first_body["session"]["status"], "unavailable");
    let first_run_id = first_body["session"]["run_id"]
        .as_str()
        .expect("first run ID")
        .to_owned();

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &start_uri,
            Some(&requester.token),
            Body::from(
                serde_json::json!({
                    "message_id": first.id,
                    "expected_state_version": 1
                })
                .to_string(),
            ),
        ))
        .await
        .expect("same-message retry response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let retry_body: serde_json::Value = response_json(response).await;
    assert_eq!(retry_body["session"]["run_id"], first_run_id);

    let second = store
        .post_requester_message(&requirement.id, &requester.user.id, "second question")
        .await
        .expect("persist second message");
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &start_uri,
            Some(&requester.token),
            Body::from(
                serde_json::json!({
                    "message_id": second.id,
                    "expected_state_version": 2
                })
                .to_string(),
            ),
        ))
        .await
        .expect("different-message conflict response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let current = store
        .requirement_by_id(&requirement.id)
        .await
        .expect("read requirement")
        .expect("requirement");
    assert_eq!(current.state_version, 2);

    let other_requirement = store
        .create_requirement(
            "Other clarification requirement",
            "Must not share run identity",
            &requester.user.id,
        )
        .await
        .expect("create second requirement");
    let other_message = store
        .post_requester_message(
            &other_requirement.id,
            &requester.user.id,
            "Other requirement message",
        )
        .await
        .expect("persist second requirement message");

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/missing-run/messages/{}/dispatch",
                requirement.id, first.id
            ),
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("unknown-run dispatch response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["error"], "not_found");

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/{}/cancel",
                other_requirement.id, first_run_id
            ),
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("cross-requirement cancel response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["error"], "not_found");

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/missing-run/cancel",
                requirement.id
            ),
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("unknown-run cancel response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["error"], "not_found");

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/{}/messages/{}/dispatch",
                other_requirement.id, first_run_id, other_message.id
            ),
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("cross-requirement dispatch response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = response_json(response).await;
    assert_eq!(body["error"], "not_found");

    let cancel_uri = format!(
        "/requirements/{}/clarification/runs/{}/cancel",
        requirement.id, first_run_id
    );
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &cancel_uri,
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("cancel response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let cancelled: serde_json::Value = response_json(response).await;
    assert_eq!(cancelled["session"]["phase"], "terminal");
    assert_eq!(cancelled["session"]["status"], "unavailable");
    assert_eq!(cancelled["session"]["cancel_requested"], true);

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &cancel_uri,
            Some(&requester.token),
            Body::empty(),
        ))
        .await
        .expect("repeat cancelled run response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let repeated_cancel: serde_json::Value = response_json(response).await;
    assert_eq!(repeated_cancel["session"]["phase"], "terminal");
    assert_eq!(repeated_cancel["session"]["cancel_requested"], true);

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &start_uri,
            Some(&requester.token),
            Body::from(
                serde_json::json!({
                    "message_id": first.id,
                    "expected_state_version": 2
                })
                .to_string(),
            ),
        ))
        .await
        .expect("terminal same-message response");
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let third = store
        .post_requester_message(&requirement.id, &requester.user.id, "third question")
        .await
        .expect("persist third message");
    let response = app
        .oneshot(request(
            Method::POST,
            &start_uri,
            Some(&requester.token),
            Body::from(
                serde_json::json!({
                    "message_id": third.id,
                    "expected_state_version": 2
                })
                .to_string(),
            ),
        ))
        .await
        .expect("new terminal-slot start response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let next: serde_json::Value = response_json(response).await;
    assert_ne!(next["session"]["run_id"], first_run_id);
    assert_eq!(next["session"]["phase"], "awaiting_assignment");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn clarification_runtime_projects_existing_events_and_releases_slot() {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for clarification integration tests");
    let _database_test_guard = database_test_lock().await;
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_server::run_migrations(&pool)
        .await
        .expect("run migrations");
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL",
    )
    .execute(&pool)
    .await
    .expect("clear daemon leases");
    let store = AuthStore::new(pool.clone());
    let email = unique_email("clarification-runtime");
    store
        .issue_code(&email, "555555")
        .await
        .expect("issue user code");
    let user = store
        .verify_code(&email, "555555")
        .await
        .expect("verify user code");
    store
        .update_user_role(&user.user.id, Role::Admin)
        .await
        .expect("promote test user")
        .expect("test user exists");
    let requirement = store
        .create_requirement("Runtime projection", "Runtime output", &user.user.id)
        .await
        .expect("create requirement");
    let first = store
        .post_requester_message(&requirement.id, &user.user.id, "start clarification")
        .await
        .expect("persist start message");
    let app = auth_router(AuthState::with_log_delivery(store.clone()));

    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/daemon/setup/request",
            None,
            Body::from(r#"{"label":"clarification runtime daemon"}"#),
        ))
        .await
        .expect("setup request response");
    let created: north_server::daemon::SetupCreatedResponse = response_json(response).await;
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &created.verification_path,
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("approve daemon response");
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
        .expect("claim daemon response");
    let claimed: SetupClaimed = response_json(response).await;
    assert_eq!(claimed.status, "claimed");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let address = listener.local_addr().expect("server address");
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, server_app)
            .await
            .expect("serve clarification runtime");
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

    let start_uri = format!("/requirements/{}/clarification/start", requirement.id);
    let response = app_for_request_body(
        &app,
        Method::POST,
        &start_uri,
        &user.token,
        serde_json::json!({
            "message_id": first.id,
            "expected_state_version": 1
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let start_body: serde_json::Value = response_json(response).await;
    let run_id = start_body["session"]["run_id"]
        .as_str()
        .expect("run ID")
        .to_owned();
    assert_eq!(start_body["session"]["phase"], "active");
    assert_eq!(start_body["session"]["status"], "starting");
    let ServerFrame::Command(start_command) = next_server_frame(&mut socket).await else {
        panic!("expected session.start command");
    };
    assert_eq!(start_command.session_id, run_id);
    let Command::SessionStart(start_payload) = &start_command.command else {
        panic!("expected session.start payload");
    };
    assert!(start_payload
        .conversation
        .excerpt
        .iter()
        .any(|message| message.message_id == first.id && message.content == "start clarification"));
    socket
        .send(Message::Text(
            encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                command_id: start_command.command_id,
                session_id: start_command.session_id,
                server_command_seq: start_command.server_command_seq,
                schema_version: SCHEMA_VERSION,
            }))
            .expect("encode start ACK")
            .into(),
        ))
        .await
        .expect("send start ACK");

    let follow_up = store
        .post_requester_message(&requirement.id, &user.user.id, "follow-up clarification")
        .await
        .expect("persist follow-up message");
    let dispatch_uri = format!(
        "/requirements/{}/clarification/runs/{}/messages/{}/dispatch",
        requirement.id, run_id, follow_up.id
    );
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &dispatch_uri,
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("dispatch follow-up response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let ServerFrame::Command(message_command) = next_server_frame(&mut socket).await else {
        panic!("expected message.send command");
    };
    let Command::MessageSend(message_send) = &message_command.command else {
        panic!("expected message.send payload");
    };
    assert_eq!(message_send.message_id, follow_up.id);
    assert_eq!(message_send.content, "follow-up clarification");
    socket
        .send(Message::Text(
            encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                command_id: message_command.command_id.clone(),
                session_id: message_command.session_id.clone(),
                server_command_seq: message_command.server_command_seq,
                schema_version: SCHEMA_VERSION,
            }))
            .expect("encode message ACK")
            .into(),
        ))
        .await
        .expect("send message ACK");
    let replay = app
        .clone()
        .oneshot(request(
            Method::POST,
            &dispatch_uri,
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("replay follow-up response");
    assert_eq!(replay.status(), StatusCode::ACCEPTED);
    if let Ok(ServerFrame::Command(replayed_command)) =
        timeout(Duration::from_millis(100), next_server_frame(&mut socket)).await
    {
        assert_eq!(replayed_command.command_id, message_command.command_id);
        assert!(matches!(replayed_command.command, Command::MessageSend(_)));
    }

    let started = DaemonFrame::Event(EventEnvelope {
        event_id: "clarification-started-event".into(),
        session_id: run_id.clone(),
        daemon_event_seq: 1,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionStarted(north_protocol::SessionStarted {
            runtime_id: "pi-runtime-1".into(),
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&started)
                .expect("encode started event")
                .into(),
        ))
        .await
        .expect("send started event");
    let ServerFrame::EventAck(started_ack) = next_server_frame(&mut socket).await else {
        panic!("expected started ACK");
    };
    assert_eq!(started_ack.status, EventAckStatus::Accepted);

    let message_event = DaemonFrame::Event(EventEnvelope {
        event_id: "clarification-message-event".into(),
        session_id: run_id.clone(),
        daemon_event_seq: 2,
        sent_at: "2026-01-01T00:00:01Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::AgentMessage(north_protocol::AgentMessage {
            message_id: "agent-message-1".into(),
            content: "Please clarify scope.".into(),
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&message_event)
                .expect("encode agent message event")
                .into(),
        ))
        .await
        .expect("send agent message event");
    let ServerFrame::EventAck(message_ack) = next_server_frame(&mut socket).await else {
        panic!("expected agent message ACK");
    };
    assert_eq!(message_ack.status, EventAckStatus::Accepted);

    let activity_event = DaemonFrame::Event(EventEnvelope {
        event_id: "clarification-activity-event".into(),
        session_id: run_id.clone(),
        daemon_event_seq: 3,
        sent_at: "2026-01-01T00:00:02Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::AgentActivity(north_protocol::AgentActivity {
            activity: "Pi reviewed authorized context".into(),
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&activity_event)
                .expect("encode activity event")
                .into(),
        ))
        .await
        .expect("send activity event");
    let ServerFrame::EventAck(activity_ack) = next_server_frame(&mut socket).await else {
        panic!("expected activity ACK");
    };
    assert_eq!(activity_ack.status, EventAckStatus::Accepted);

    let cancel_uri = format!(
        "/requirements/{}/clarification/runs/{}/cancel",
        requirement.id, run_id
    );
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &cancel_uri,
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("cancel assigned run response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let cancel_body: serde_json::Value = response_json(response).await;
    assert_eq!(cancel_body["session"]["phase"], "active");
    assert_eq!(cancel_body["session"]["cancel_requested"], true);
    let ServerFrame::Command(cancel_command) = next_server_frame(&mut socket).await else {
        panic!("expected session.cancel command");
    };
    assert!(matches!(cancel_command.command, Command::SessionCancel(_)));
    socket
        .send(Message::Text(
            encode_daemon_frame(&DaemonFrame::CommandAck(CommandAck {
                command_id: cancel_command.command_id.clone(),
                session_id: cancel_command.session_id.clone(),
                server_command_seq: cancel_command.server_command_seq,
                schema_version: SCHEMA_VERSION,
            }))
            .expect("encode cancel ACK")
            .into(),
        ))
        .await
        .expect("send cancel ACK");
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/session", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("active cancelled session response");
    let active_cancel: serde_json::Value = response_json(response).await;
    assert_eq!(active_cancel["session"]["phase"], "active");
    assert_eq!(active_cancel["session"]["cancel_requested"], true);

    let blocked_message = store
        .post_requester_message(&requirement.id, &user.user.id, "blocked after cancel")
        .await
        .expect("persist blocked message");
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/{}/messages/{}/dispatch",
                requirement.id, run_id, blocked_message.id
            ),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("dispatch after cancellation response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let blocked_body: serde_json::Value = response_json(response).await;
    assert_eq!(blocked_body["error"], "conflict");

    let completed = DaemonFrame::Event(EventEnvelope {
        event_id: "clarification-completed-event".into(),
        session_id: run_id.clone(),
        daemon_event_seq: 4,
        sent_at: "2026-01-01T00:00:03Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionCompleted(north_protocol::SessionCompleted {
            summary: "Clarification completed".into(),
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&completed)
                .expect("encode completion event")
                .into(),
        ))
        .await
        .expect("send completion event");
    let ServerFrame::EventAck(completed_ack) = next_server_frame(&mut socket).await else {
        panic!("expected completion ACK");
    };
    assert_eq!(completed_ack.status, EventAckStatus::Accepted);
    socket
        .send(Message::Text(
            encode_daemon_frame(&completed)
                .expect("encode duplicate completion event")
                .into(),
        ))
        .await
        .expect("send duplicate completion event");
    let ServerFrame::EventAck(duplicate_completed_ack) = next_server_frame(&mut socket).await
    else {
        panic!("expected duplicate completion ACK");
    };
    assert_eq!(duplicate_completed_ack.status, EventAckStatus::Accepted);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/session", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("session read response");
    let session: serde_json::Value = response_json(response).await;
    assert_eq!(session["session"]["run_id"], run_id);
    assert_eq!(session["session"]["phase"], "terminal");
    assert_eq!(session["session"]["status"], "completed");
    let messages = store
        .conversation_messages(&requirement.id)
        .await
        .expect("read projected conversation");
    assert!(messages.iter().any(|message| {
        message.id == "agent-message-1" && message.body == "Please clarify scope."
    }));
    let activities = store
        .clarification_activities(&requirement.id, 0, 50)
        .await
        .expect("read projected activity")
        .0;
    assert!(activities
        .iter()
        .any(|activity| activity.activity == "Pi reviewed authorized context"));
    assert!(store
        .latest_readiness(&requirement.id)
        .await
        .expect("read readiness")
        .is_none());
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/readiness", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("readiness endpoint response");
    let readiness: serde_json::Value = response_json(response).await;
    assert!(readiness["assessment"].is_null());
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/activity", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("activity endpoint response");
    let activity_read: serde_json::Value = response_json(response).await;
    assert!(activity_read["activities"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["activity"] == "Pi reviewed authorized context")
    }));

    let second = store
        .post_requester_message(&requirement.id, &user.user.id, "new clarification")
        .await
        .expect("persist second start message");
    let response = app_for_request_body(
        &app,
        Method::POST,
        &start_uri,
        &user.token,
        serde_json::json!({
            "message_id": second.id,
            "expected_state_version": 2
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let next_body: serde_json::Value = response_json(response).await;
    let next_run_id = next_body["session"]["run_id"]
        .as_str()
        .expect("second run ID")
        .to_owned();
    assert_ne!(next_run_id, run_id);
    assert_eq!(next_body["session"]["phase"], "active");
    let ServerFrame::Command(next_command) = next_server_frame(&mut socket).await else {
        panic!("expected second session.start command");
    };
    assert!(matches!(next_command.command, Command::SessionStart(_)));
    let failure = DaemonFrame::Event(EventEnvelope {
        event_id: "clarification-failed-event".into(),
        session_id: next_run_id.clone(),
        daemon_event_seq: 1,
        sent_at: "2026-01-01T00:00:04Z".into(),
        schema_version: SCHEMA_VERSION,
        event: Event::SessionFailed(north_protocol::SessionFailed {
            recoverable: false,
            reason: "Pi clarification failed before assessment".into(),
        }),
    });
    socket
        .send(Message::Text(
            encode_daemon_frame(&failure)
                .expect("encode failure event")
                .into(),
        ))
        .await
        .expect("send failure event");
    let ServerFrame::EventAck(failure_ack) = next_server_frame(&mut socket).await else {
        panic!("expected failure ACK");
    };
    assert_eq!(failure_ack.status, EventAckStatus::Accepted);
    socket
        .send(Message::Text(
            encode_daemon_frame(&failure)
                .expect("encode duplicate failure event")
                .into(),
        ))
        .await
        .expect("send duplicate failure event");
    let ServerFrame::EventAck(duplicate_failure_ack) = next_server_frame(&mut socket).await else {
        panic!("expected duplicate failure ACK");
    };
    assert_eq!(duplicate_failure_ack.status, EventAckStatus::Accepted);
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/session", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("failed session read response");
    let failed_session: serde_json::Value = response_json(response).await;
    assert_eq!(failed_session["session"]["run_id"], next_run_id);
    assert_eq!(failed_session["session"]["phase"], "terminal");
    assert_eq!(failed_session["session"]["status"], "unavailable");
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/{}/cancel",
                requirement.id, next_run_id
            ),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("completed failed-run cancellation response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let failed_cancel: serde_json::Value = response_json(response).await;
    assert_eq!(failed_cancel["error"], "conflict");

    let stale_message = store
        .post_requester_message(&requirement.id, &user.user.id, "stale run message")
        .await
        .expect("persist stale-run message");
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!(
                "/requirements/{}/clarification/runs/{}/messages/{}/dispatch",
                requirement.id, run_id, stale_message.id
            ),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("stale run dispatch response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let stale_body: serde_json::Value = response_json(response).await;
    assert_eq!(stale_body["error"], "conflict");
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/requirements/{}/session", requirement.id),
            Some(&user.token),
            Body::empty(),
        ))
        .await
        .expect("latest session after stale dispatch response");
    let latest: serde_json::Value = response_json(response).await;
    assert_eq!(latest["session"]["run_id"], next_run_id);
    drop(socket);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn clarification_postgres_concurrency_preserves_single_slot_and_command_identity() {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for clarification integration tests");
    let _database_test_guard = database_test_lock().await;
    let pool = PoolOptions::new()
        .max_connections(16)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_server::run_migrations(&pool)
        .await
        .expect("run migrations");
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, connection_id = NULL",
    )
    .execute(&pool)
    .await
    .expect("clear daemon leases");
    let store = AuthStore::new(pool.clone());
    let email = unique_email("clarification-concurrency");
    store
        .issue_code(&email, "666666")
        .await
        .expect("issue user code");
    let user = store
        .verify_code(&email, "666666")
        .await
        .expect("verify user code");
    store
        .update_user_role(&user.user.id, Role::Admin)
        .await
        .expect("promote test user")
        .expect("test user exists");
    let daemon_id = unique_email("clarification-daemon").replace(['@', '.'], "-");
    sqlx::query(
        "INSERT INTO daemon_registrations
            (daemon_id, credential_hash, label, created_by, protocol_version,
             connected_at, last_seen_at, capabilities)
         VALUES ($1, $2, $3, $4, '0.1', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, $5)",
    )
    .bind(&daemon_id)
    .bind(vec![7_u8; 32])
    .bind("clarification concurrency daemon")
    .bind(&user.user.id)
    .bind("[\"agent\"]")
    .execute(&pool)
    .await
    .expect("insert eligible daemon");
    let context = |requirement_id: &str, revision: u64, message_id: &str, content: &str| {
        serde_json::json!({
            "requirement": { "id": requirement_id, "revision": revision },
            "conversation": {
                "excerpt": [{
                    "message_id": message_id,
                    "role": "requester",
                    "content": content
                }]
            },
            "repositories": []
        })
    };
    let start = |store: AuthStore,
                 requirement_id: String,
                 message_id: String,
                 expected_state_version: u64,
                 start_context: serde_json::Value,
                 capability: String| async move {
        let capabilities = vec![capability];
        store
            .start_clarification(
                north_persistence::ClarificationStartInput {
                    requirement_id: &requirement_id,
                    start_message_id: &message_id,
                    expected_state_version,
                    context_requirement_revision: start_context["requirement"]["revision"]
                        .as_u64()
                        .expect("context revision"),
                    context: &start_context,
                    repository_ids: &[],
                    required_capabilities: &capabilities,
                },
                |_daemon_id, _run_id, _command_id, _sequence, _context| {
                    Ok::<_, north_persistence::PersistenceError>("session.start".into())
                },
            )
            .await
    };

    let assigned_requirement = store
        .create_requirement("Concurrent assigned start", "Description", &user.user.id)
        .await
        .expect("create assigned requirement");
    let assigned_message = store
        .post_requester_message(
            &assigned_requirement.id,
            &user.user.id,
            "same assigned question",
        )
        .await
        .expect("persist assigned message");
    let assigned_context = context(
        &assigned_requirement.id,
        assigned_requirement.revision,
        &assigned_message.id,
        &assigned_message.body,
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let left_barrier = barrier.clone();
    let left_pool = pool.clone();
    let left_requirement_id = assigned_requirement.id.clone();
    let left_message_id = assigned_message.id.clone();
    let left_expected_state_version = assigned_requirement.state_version;
    let left_context = assigned_context.clone();
    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        start(
            AuthStore::new(left_pool),
            left_requirement_id,
            left_message_id,
            left_expected_state_version,
            left_context,
            "agent".into(),
        )
        .await
    });
    let right_barrier = barrier.clone();
    let right_pool = pool.clone();
    let right_requirement_id = assigned_requirement.id.clone();
    let right_message_id = assigned_message.id.clone();
    let right_expected_state_version = assigned_requirement.state_version;
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        start(
            AuthStore::new(right_pool),
            right_requirement_id,
            right_message_id,
            right_expected_state_version,
            assigned_context,
            "agent".into(),
        )
        .await
    });
    barrier.wait().await;
    let (left, right) = tokio::join!(left, right);
    let left = left
        .expect("left concurrent start task")
        .expect("left start");
    let right = right
        .expect("right concurrent start task")
        .expect("right start");
    assert_eq!(left.run.run_id, right.run.run_id);
    assert_ne!(left.reused, right.reused);
    assert_eq!(left.command_id, right.command_id);
    let assigned_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_sessions WHERE requirement_id = $1")
            .bind(&assigned_requirement.id)
            .fetch_one(&pool)
            .await
            .expect("count assigned sessions");
    let assigned_commands: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&left.run.run_id)
            .fetch_one(&pool)
            .await
            .expect("count assigned commands");
    assert_eq!(assigned_sessions, 1);
    assert_eq!(assigned_commands, 1);
    assert_eq!(
        store
            .requirement_by_id(&assigned_requirement.id)
            .await
            .expect("read assigned requirement")
            .expect("assigned requirement")
            .state_version,
        2
    );

    let different_requirement = store
        .create_requirement("Concurrent different start", "Description", &user.user.id)
        .await
        .expect("create different requirement");
    let different_left = store
        .post_requester_message(
            &different_requirement.id,
            &user.user.id,
            "winning candidate",
        )
        .await
        .expect("persist winning candidate");
    let different_right = store
        .post_requester_message(&different_requirement.id, &user.user.id, "losing candidate")
        .await
        .expect("persist losing candidate");
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let left_barrier = barrier.clone();
    let left_requirement_id = different_requirement.id.clone();
    let left_message_id = different_left.id.clone();
    let left_context = context(
        &different_requirement.id,
        different_requirement.revision,
        &different_left.id,
        &different_left.body,
    );
    let left_pool = pool.clone();
    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        start(
            AuthStore::new(left_pool),
            left_requirement_id,
            left_message_id,
            1,
            left_context,
            "agent".into(),
        )
        .await
    });
    let right_barrier = barrier.clone();
    let right_requirement_id = different_requirement.id.clone();
    let right_message_id = different_right.id.clone();
    let right_context = context(
        &different_requirement.id,
        different_requirement.revision,
        &different_right.id,
        &different_right.body,
    );
    let right_pool = pool.clone();
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        start(
            AuthStore::new(right_pool),
            right_requirement_id,
            right_message_id,
            1,
            right_context,
            "agent".into(),
        )
        .await
    });
    barrier.wait().await;
    let (left, right) = tokio::join!(left, right);
    let left = left.expect("left different start task");
    let right = right.expect("right different start task");
    let error = match (left, right) {
        (Ok(_), Err(error)) | (Err(error), Ok(_)) => error,
        _ => panic!("different concurrent starts must have one winner"),
    };
    assert!(matches!(
        error,
        north_persistence::ClarificationError::ExistingRunDifferentStart
    ));
    let different_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_sessions WHERE requirement_id = $1")
            .bind(&different_requirement.id)
            .fetch_one(&pool)
            .await
            .expect("count different sessions");
    let different_messages = store
        .conversation_messages(&different_requirement.id)
        .await
        .expect("read different conversation");
    assert_eq!(different_sessions, 1);
    assert_eq!(different_messages.len(), 2);

    let stale_requirement = store
        .create_requirement("Stale clarification start", "Description", &user.user.id)
        .await
        .expect("create stale requirement");
    let stale_message = store
        .post_requester_message(&stale_requirement.id, &user.user.id, "stale start")
        .await
        .expect("persist stale start message");
    let stale = start(
        AuthStore::new(pool.clone()),
        stale_requirement.id.clone(),
        stale_message.id.clone(),
        99,
        context(
            &stale_requirement.id,
            stale_requirement.revision,
            &stale_message.id,
            &stale_message.body,
        ),
        "agent".into(),
    )
    .await;
    assert!(matches!(
        stale,
        Err(north_persistence::ClarificationError::StateVersionConflict)
    ));
    let stale_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_sessions WHERE requirement_id = $1")
            .bind(&stale_requirement.id)
            .fetch_one(&pool)
            .await
            .expect("count stale sessions");
    assert_eq!(stale_sessions, 0);
    assert_eq!(
        store
            .conversation_messages(&stale_requirement.id)
            .await
            .expect("read stale conversation")
            .len(),
        1
    );

    let awaiting_requirement = store
        .create_requirement("Concurrent awaiting start", "Description", &user.user.id)
        .await
        .expect("create awaiting requirement");
    let awaiting_message = store
        .post_requester_message(
            &awaiting_requirement.id,
            &user.user.id,
            "same awaiting question",
        )
        .await
        .expect("persist awaiting message");
    let awaiting_context = context(
        &awaiting_requirement.id,
        awaiting_requirement.revision,
        &awaiting_message.id,
        &awaiting_message.body,
    );
    let initial = start(
        AuthStore::new(pool.clone()),
        awaiting_requirement.id.clone(),
        awaiting_message.id.clone(),
        1,
        awaiting_context.clone(),
        "missing-agent".into(),
    )
    .await
    .expect("create awaiting run");
    assert_eq!(
        initial.run.phase,
        north_persistence::ClarificationPhase::AwaitingAssignment
    );
    let retry_barrier = Arc::new(tokio::sync::Barrier::new(3));
    let same_barrier = retry_barrier.clone();
    let same_pool = pool.clone();
    let same_requirement_id = awaiting_requirement.id.clone();
    let same_message_id = awaiting_message.id.clone();
    let same_context = awaiting_context.clone();
    let same = tokio::spawn(async move {
        same_barrier.wait().await;
        start(
            AuthStore::new(same_pool),
            same_requirement_id,
            same_message_id,
            999,
            same_context,
            "missing-agent".into(),
        )
        .await
    });
    let retry_barrier = retry_barrier.clone();
    let retry_release = retry_barrier.clone();
    let retry_pool = pool.clone();
    let retry_requirement_id = awaiting_requirement.id.clone();
    let retry_message_id = awaiting_message.id.clone();
    let retry = tokio::spawn(async move {
        retry_barrier.wait().await;
        start(
            AuthStore::new(retry_pool),
            retry_requirement_id,
            retry_message_id,
            999,
            awaiting_context,
            "missing-agent".into(),
        )
        .await
    });
    retry_release.wait().await;
    let (same, retry) = tokio::join!(same, retry);
    let same = same
        .expect("same-message awaiting retry task")
        .expect("same-message awaiting retry");
    let retry = retry
        .expect("second awaiting retry task")
        .expect("second awaiting retry");
    assert_eq!(same.run.run_id, initial.run.run_id);
    assert_eq!(retry.run.run_id, initial.run.run_id);
    assert!(same.reused && retry.reused);

    let competing_message = store
        .post_requester_message(
            &awaiting_requirement.id,
            &user.user.id,
            "different awaiting question",
        )
        .await
        .expect("persist competing awaiting message");
    let competing_context = context(
        &awaiting_requirement.id,
        awaiting_requirement.revision,
        &competing_message.id,
        &competing_message.body,
    );
    let race_barrier = Arc::new(tokio::sync::Barrier::new(3));
    let same_race_barrier = race_barrier.clone();
    let same_race_pool = pool.clone();
    let same_race_requirement_id = awaiting_requirement.id.clone();
    let same_race_message_id = awaiting_message.id.clone();
    let same_race_context = context(
        &awaiting_requirement.id,
        awaiting_requirement.revision,
        &awaiting_message.id,
        &awaiting_message.body,
    );
    let same_race = tokio::spawn(async move {
        same_race_barrier.wait().await;
        start(
            AuthStore::new(same_race_pool),
            same_race_requirement_id,
            same_race_message_id,
            999,
            same_race_context,
            "missing-agent".into(),
        )
        .await
    });
    let different_race_barrier = race_barrier.clone();
    let different_race_pool = pool.clone();
    let different_race_requirement_id = awaiting_requirement.id.clone();
    let different_race_message_id = competing_message.id.clone();
    let different_race = tokio::spawn(async move {
        different_race_barrier.wait().await;
        start(
            AuthStore::new(different_race_pool),
            different_race_requirement_id,
            different_race_message_id,
            2,
            competing_context,
            "missing-agent".into(),
        )
        .await
    });
    race_barrier.wait().await;
    let (same_race, different_race) = tokio::join!(same_race, different_race);
    let same_race = same_race
        .expect("same-message race task")
        .expect("same-message race");
    let different_race = different_race
        .expect("different-message race task")
        .expect_err("different message must lose occupied slot");
    assert_eq!(same_race.run.run_id, initial.run.run_id);
    assert!(same_race.reused);
    assert!(matches!(
        different_race,
        north_persistence::ClarificationError::ExistingRunDifferentStart
    ));
    let awaiting_sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_sessions WHERE requirement_id = $1")
            .bind(&awaiting_requirement.id)
            .fetch_one(&pool)
            .await
            .expect("count awaiting sessions");
    assert_eq!(awaiting_sessions, 1);

    let cancellation_requirement = store
        .create_requirement("Cancellation arbitration", "Description", &user.user.id)
        .await
        .expect("create cancellation requirement");
    let cancellation_message = store
        .post_requester_message(
            &cancellation_requirement.id,
            &user.user.id,
            "cancel before assignment",
        )
        .await
        .expect("persist cancellation message");
    let cancellation_context = context(
        &cancellation_requirement.id,
        cancellation_requirement.revision,
        &cancellation_message.id,
        &cancellation_message.body,
    );
    let awaiting = start(
        AuthStore::new(pool.clone()),
        cancellation_requirement.id.clone(),
        cancellation_message.id.clone(),
        1,
        cancellation_context.clone(),
        "missing-agent".into(),
    )
    .await
    .expect("create cancellable awaiting run");
    let cancelled = store
        .cancel_clarification(
            &cancellation_requirement.id,
            &awaiting.run.run_id,
            |_daemon_id, _run_id, _command_id, _sequence| {
                Ok::<_, north_persistence::PersistenceError>("session.cancel".into())
            },
        )
        .await
        .expect("cancel awaiting run");
    assert_eq!(
        cancelled.run.phase,
        north_persistence::ClarificationPhase::Terminal
    );
    assert!(cancelled.command_id.is_empty());
    let new_message = store
        .post_requester_message(
            &cancellation_requirement.id,
            &user.user.id,
            "after cancellation",
        )
        .await
        .expect("persist post-cancellation message");
    let new_run = start(
        AuthStore::new(pool.clone()),
        cancellation_requirement.id.clone(),
        new_message.id.clone(),
        2,
        context(
            &cancellation_requirement.id,
            cancellation_requirement.revision,
            &new_message.id,
            &new_message.body,
        ),
        "missing-agent".into(),
    )
    .await
    .expect("start after cancellation");
    assert_ne!(new_run.run.run_id, awaiting.run.run_id);

    let assigned_cancel_requirement = store
        .create_requirement("Assigned cancellation", "Description", &user.user.id)
        .await
        .expect("create assigned cancellation requirement");
    let assigned_cancel_message = store
        .post_requester_message(
            &assigned_cancel_requirement.id,
            &user.user.id,
            "cancel after assignment",
        )
        .await
        .expect("persist assigned cancellation message");
    let assigned_cancel = start(
        AuthStore::new(pool.clone()),
        assigned_cancel_requirement.id.clone(),
        assigned_cancel_message.id.clone(),
        1,
        context(
            &assigned_cancel_requirement.id,
            assigned_cancel_requirement.revision,
            &assigned_cancel_message.id,
            &assigned_cancel_message.body,
        ),
        "agent".into(),
    )
    .await
    .expect("start assigned cancellation run");
    let first_cancel = store
        .cancel_clarification(
            &assigned_cancel_requirement.id,
            &assigned_cancel.run.run_id,
            |_daemon_id, _run_id, _command_id, _sequence| {
                Ok::<_, north_persistence::PersistenceError>("session.cancel".into())
            },
        )
        .await
        .expect("first assigned cancellation");
    let second_cancel = store
        .cancel_clarification(
            &assigned_cancel_requirement.id,
            &assigned_cancel.run.run_id,
            |_daemon_id, _run_id, _command_id, _sequence| {
                Ok::<_, north_persistence::PersistenceError>("session.cancel".into())
            },
        )
        .await
        .expect("replayed assigned cancellation");
    assert_eq!(
        first_cancel.run.phase,
        north_persistence::ClarificationPhase::Active
    );
    assert_eq!(first_cancel.command_id, second_cancel.command_id);
    let assigned_cancel_commands: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&assigned_cancel.run.run_id)
            .fetch_one(&pool)
            .await
            .expect("count cancellation commands");
    assert_eq!(assigned_cancel_commands, 2);
    let later = store
        .post_requester_message(
            &assigned_cancel_requirement.id,
            &user.user.id,
            "must remain history",
        )
        .await
        .expect("persist message after cancellation intent");
    let dispatch = store
        .dispatch_clarification_message(
            &assigned_cancel_requirement.id,
            &assigned_cancel.run.run_id,
            &later.id,
            |_daemon_id, _run_id, _command_id, _sequence, _message_id, _body| {
                Ok::<_, north_persistence::PersistenceError>("message.send".into())
            },
        )
        .await;
    assert!(matches!(
        dispatch,
        Err(north_persistence::ClarificationError::RunNotEligible)
    ));

    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = NULL, last_seen_at = NULL",
    )
    .execute(&pool)
    .await
    .expect("make daemon unavailable for race setup");
    let race_requirement = store
        .create_requirement("Assignment cancellation race", "Description", &user.user.id)
        .await
        .expect("create race requirement");
    let race_message = store
        .post_requester_message(&race_requirement.id, &user.user.id, "race assignment")
        .await
        .expect("persist race message");
    let race_context = context(
        &race_requirement.id,
        race_requirement.revision,
        &race_message.id,
        &race_message.body,
    );
    let race_run = start(
        AuthStore::new(pool.clone()),
        race_requirement.id.clone(),
        race_message.id.clone(),
        1,
        race_context.clone(),
        "agent".into(),
    )
    .await
    .expect("create race awaiting run");
    assert_eq!(
        race_run.run.phase,
        north_persistence::ClarificationPhase::AwaitingAssignment
    );
    sqlx::query(
        "UPDATE daemon_registrations
         SET connected_at = CURRENT_TIMESTAMP, last_seen_at = CURRENT_TIMESTAMP",
    )
    .execute(&pool)
    .await
    .expect("make daemon available for race");
    let race_barrier = Arc::new(tokio::sync::Barrier::new(3));
    let assignment_barrier = race_barrier.clone();
    let assignment_pool = pool.clone();
    let assignment_requirement_id = race_requirement.id.clone();
    let assignment_message_id = race_message.id.clone();
    let assignment = tokio::spawn(async move {
        assignment_barrier.wait().await;
        start(
            AuthStore::new(assignment_pool),
            assignment_requirement_id,
            assignment_message_id,
            999,
            race_context,
            "agent".into(),
        )
        .await
    });
    let cancellation_barrier = race_barrier.clone();
    let cancellation_pool = pool.clone();
    let cancellation_requirement_id = race_requirement.id.clone();
    let cancellation_run_id = race_run.run.run_id.clone();
    let cancellation = tokio::spawn(async move {
        cancellation_barrier.wait().await;
        AuthStore::new(cancellation_pool)
            .cancel_clarification(
                &cancellation_requirement_id,
                &cancellation_run_id,
                |_daemon_id, _run_id, _command_id, _sequence| {
                    Ok::<_, north_persistence::PersistenceError>("session.cancel".into())
                },
            )
            .await
    });
    race_barrier.wait().await;
    let (assignment, cancellation) = tokio::join!(assignment, cancellation);
    let assignment = assignment.expect("assignment race task");
    let cancellation = cancellation
        .expect("cancellation race task")
        .expect("cancellation race");
    let raced = store
        .clarification_run(&race_requirement.id, &race_run.run.run_id)
        .await
        .expect("read raced run");
    let raced_daemon: Option<String> =
        sqlx::query_scalar("SELECT daemon_id FROM execution_sessions WHERE id = $1")
            .bind(&race_run.run.run_id)
            .fetch_one(&pool)
            .await
            .expect("read raced daemon");
    let raced_commands: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM server_command_outbox WHERE session_id = $1")
            .bind(&race_run.run.run_id)
            .fetch_one(&pool)
            .await
            .expect("count raced commands");
    match raced.phase {
        north_persistence::ClarificationPhase::Terminal => {
            assert!(raced_daemon.is_none());
            assert_eq!(raced_commands, 0);
            assert!(matches!(
                assignment,
                Err(north_persistence::ClarificationError::RunNotEligible)
            ));
            assert!(cancellation.command_id.is_empty());
        }
        north_persistence::ClarificationPhase::Active => {
            assert!(raced_daemon.is_some());
            assert!(raced.cancel_requested);
            assert_eq!(raced_commands, 2);
            assert!(assignment.is_ok());
            assert!(!cancellation.command_id.is_empty());
        }
        north_persistence::ClarificationPhase::AwaitingAssignment => {
            panic!("assignment/cancellation race left run awaiting");
        }
    }
}

async fn app_for_request_body(
    app: &axum::Router,
    method: Method,
    uri: &str,
    token: &str,
    body: serde_json::Value,
) -> Response {
    app.clone()
        .oneshot(request(
            method,
            uri,
            Some(token),
            Body::from(body.to_string()),
        ))
        .await
        .expect("clarification request response")
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
