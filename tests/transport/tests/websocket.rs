use futures_util::{SinkExt, StreamExt};
use north_daemon::transport::{
    ConnectionConfig, ConnectionError, ConnectionEvent, ConnectionSupervisor,
    HandshakeTimeouts as DaemonHandshakeTimeouts,
};
use north_protocol::{
    encode_daemon_frame, DaemonFrame, EventAck, EventAckStatus, Hello, ProtocolErrorFrame,
    ReconcileSnapshot, ServerFrame, SessionReconcileState, Welcome, PROTOCOL_VERSION,
    SCHEMA_VERSION,
};
use north_server::transport::{
    daemon_router, DaemonConnection, DaemonTransportState,
    HandshakeTimeouts as ServerHandshakeTimeouts,
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    net::TcpListener,
    sync::mpsc,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn spawn_server(
    handshake_timeouts: ServerHandshakeTimeouts,
    capacity: usize,
) -> (SocketAddr, mpsc::Receiver<DaemonConnection>, JoinHandle<()>) {
    let (connection_sender, connection_receiver) = mpsc::channel(capacity);
    let state =
        DaemonTransportState::with_handshake_timeouts(connection_sender, handshake_timeouts);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        axum::serve(listener, daemon_router(state))
            .await
            .expect("serve test WebSocket endpoint");
    });
    (address, connection_receiver, task)
}

fn server_timeouts() -> ServerHandshakeTimeouts {
    ServerHandshakeTimeouts {
        hello: Duration::from_secs(1),
        admission: Duration::from_secs(1),
        welcome: Duration::from_secs(1),
        reconcile: Duration::from_secs(1),
    }
}

fn daemon_config(address: SocketAddr) -> ConnectionConfig {
    let mut config = ConnectionConfig::new(
        format!("ws://{address}/daemon/ws"),
        Hello::new("daemon-1", "credential", vec!["agent".into()]),
    );
    config.handshake = DaemonHandshakeTimeouts {
        hello: Duration::from_secs(1),
        welcome: Duration::from_secs(1),
        reconcile: Duration::from_secs(1),
        coordination: Duration::from_secs(1),
    };
    config
}

fn welcome() -> ServerFrame {
    ServerFrame::Welcome(Welcome {
        protocol_version: PROTOCOL_VERSION.into(),
        schema_version: SCHEMA_VERSION,
        daemon_id: "daemon-1".into(),
        server_time: "2026-01-01T00:00:00Z".into(),
    })
}

fn reconciliation(sessions: Vec<SessionReconcileState>) -> ServerFrame {
    ServerFrame::Reconcile(ReconcileSnapshot {
        schema_version: SCHEMA_VERSION,
        sessions,
    })
}

fn event_ack() -> ServerFrame {
    ServerFrame::EventAck(EventAck {
        event_id: "event-1".into(),
        session_id: "session-1".into(),
        daemon_event_seq: 1,
        schema_version: SCHEMA_VERSION,
        status: EventAckStatus::Accepted,
        reason: None,
    })
}

async fn receive_hello(inbound: &mut mpsc::Receiver<DaemonFrame>) {
    assert!(matches!(
        timeout(Duration::from_secs(1), inbound.recv())
            .await
            .expect("hello deadline")
            .expect("hello frame"),
        DaemonFrame::Hello(_)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_axum_tungstenite_empty_reconciliation_reaches_active() {
    let (address, mut connections, server) = spawn_server(server_timeouts(), 4).await;
    let (outbound_sender, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let (events_sender, mut events_receiver) = mpsc::channel(8);
    let supervisor = ConnectionSupervisor::new(daemon_config(address));
    let daemon = tokio::spawn(async move {
        supervisor
            .run(outbound_receiver, events_sender)
            .await
            .expect_err("test daemon remains active until aborted")
    });

    let connection = timeout(Duration::from_secs(1), connections.recv())
        .await
        .expect("connection admission deadline")
        .expect("connection");
    let mut inbound = connection.inbound;
    let outbound = connection.outbound;
    receive_hello(&mut inbound).await;
    outbound.send(welcome()).await.expect("welcome");
    outbound
        .send(reconciliation(Vec::new()))
        .await
        .expect("empty reconciliation");

    let event = timeout(Duration::from_secs(1), events_receiver.recv())
        .await
        .expect("handshake result deadline")
        .expect("handshake result");
    let ConnectionEvent::HandshakeComplete { result, ready } = event else {
        panic!("expected handshake result");
    };
    assert_eq!(result.welcome.daemon_id, "daemon-1");
    assert!(result.reconciliation.sessions.is_empty());
    ready.send(()).expect("activate daemon");

    outbound.send(event_ack()).await.expect("application frame");
    let event = timeout(Duration::from_secs(1), events_receiver.recv())
        .await
        .expect("application frame deadline")
        .expect("application frame");
    assert!(matches!(
        event,
        ConnectionEvent::Frame(ServerFrame::EventAck(_))
    ));

    daemon.abort();
    server.abort();
    let _ = outbound_sender;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_application_traffic_waits_for_reconciliation_activation() {
    let (address, mut connections, server) = spawn_server(server_timeouts(), 4).await;
    let (outbound_sender, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let heartbeat = DaemonFrame::Heartbeat(north_protocol::Heartbeat {
        schema_version: SCHEMA_VERSION,
        daemon_id: "daemon-1".into(),
        sent_at: "2026-01-01T00:00:00Z".into(),
        application_state: "connected".into(),
    });
    outbound_sender
        .send(heartbeat.clone())
        .await
        .expect("queue heartbeat before connect");
    let (events_sender, mut events_receiver) = mpsc::channel(8);
    let supervisor = ConnectionSupervisor::new(daemon_config(address));
    let daemon =
        tokio::spawn(async move { supervisor.run(outbound_receiver, events_sender).await });

    let connection = timeout(Duration::from_secs(1), connections.recv())
        .await
        .expect("connection admission deadline")
        .expect("connection");
    let mut inbound = connection.inbound;
    let outbound = connection.outbound;
    receive_hello(&mut inbound).await;
    sleep(Duration::from_millis(25)).await;
    assert!(
        inbound.try_recv().is_err(),
        "heartbeat raced before welcome"
    );

    outbound.send(welcome()).await.expect("welcome");
    outbound
        .send(reconciliation(vec![
            SessionReconcileState {
                session_id: "session-1".into(),
                command_ack_through_seq: 2,
                event_ack_through_seq: 3,
                event_ack_sparse: vec![5],
            },
            SessionReconcileState {
                session_id: "session-2".into(),
                command_ack_through_seq: 4,
                event_ack_through_seq: 5,
                event_ack_sparse: vec![7],
            },
        ]))
        .await
        .expect("multiple-session reconciliation");
    let event = timeout(Duration::from_secs(1), events_receiver.recv())
        .await
        .expect("handshake result deadline")
        .expect("handshake result");
    let ConnectionEvent::HandshakeComplete { result, ready } = event else {
        panic!("expected handshake result");
    };
    assert_eq!(
        result
            .reconciliation
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>(),
        ["session-1", "session-2"]
    );
    assert!(
        inbound.try_recv().is_err(),
        "heartbeat raced before activation"
    );
    ready.send(()).expect("activate daemon");

    assert!(matches!(
        timeout(Duration::from_secs(1), inbound.recv())
            .await
            .expect("heartbeat deadline")
            .expect("heartbeat frame"),
        frame if frame == heartbeat
    ));

    daemon.abort();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_failure_closes_without_application_dispatch_or_reconnect() {
    let (address, mut connections, server) = spawn_server(server_timeouts(), 4).await;
    let (_outbound_sender, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let (events_sender, mut events_receiver) = mpsc::channel(8);
    let supervisor = ConnectionSupervisor::new(daemon_config(address));
    let daemon =
        tokio::spawn(async move { supervisor.run(outbound_receiver, events_sender).await });

    let connection = timeout(Duration::from_secs(1), connections.recv())
        .await
        .expect("connection admission deadline")
        .expect("connection");
    let mut inbound = connection.inbound;
    let outbound = connection.outbound;
    receive_hello(&mut inbound).await;
    outbound
        .send(ServerFrame::ProtocolError(ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: "unsupported_version".into(),
            message: "close connection".into(),
        }))
        .await
        .expect("protocol error");

    let result = timeout(Duration::from_secs(1), daemon)
        .await
        .expect("protocol failure deadline")
        .expect("supervisor task");
    match result {
        Err(ConnectionError::TerminalProtocol { code, message }) => {
            assert_eq!(code, "unsupported_version");
            assert_eq!(message, "close connection");
        }
        other => panic!("expected terminal protocol error, got {other:?}"),
    }
    assert!(
        events_receiver.try_recv().is_err(),
        "protocol error dispatched app frame"
    );
    sleep(Duration::from_millis(25)).await;
    assert!(
        connections.try_recv().is_err(),
        "terminal error reconnected"
    );

    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_first_frame_gets_protocol_error_without_admission() {
    let (address, mut connections, server) = spawn_server(server_timeouts(), 4).await;
    let url = format!("ws://{address}/daemon/ws");
    let (mut client, _) = connect_async(&url).await.expect("connect invalid peer");
    let heartbeat = encode_daemon_frame(&DaemonFrame::Heartbeat(north_protocol::Heartbeat {
        schema_version: SCHEMA_VERSION,
        daemon_id: "daemon-1".into(),
        sent_at: "2026-01-01T00:00:00Z".into(),
        application_state: "connected".into(),
    }))
    .expect("encode invalid first application frame");
    client
        .send(Message::Text(heartbeat.into()))
        .await
        .expect("send invalid first frame");

    let response = timeout(Duration::from_secs(1), client.next())
        .await
        .expect("protocol error deadline")
        .expect("protocol error response")
        .expect("protocol error WebSocket message");
    let Message::Text(response) = response else {
        panic!("expected protocol error text");
    };
    let ServerFrame::ProtocolError(error) =
        ServerFrame::from_json(response.as_ref()).expect("decode protocol error")
    else {
        panic!("expected protocol error frame");
    };
    assert_eq!(error.code, "expected_hello");
    assert!(connections.try_recv().is_err(), "invalid peer was admitted");

    let closed = timeout(Duration::from_secs(1), client.next())
        .await
        .expect("close deadline");
    assert!(matches!(closed, None | Some(Err(_))));
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_backpressure_still_observes_hello_and_admission_deadlines() {
    let (connection_sender, connection_receiver) = mpsc::channel(1);
    let (_dummy_inbound_sender, dummy_inbound) = mpsc::channel(1);
    let (dummy_outbound, _dummy_outbound_receiver) = mpsc::channel(1);
    connection_sender
        .try_send(DaemonConnection {
            inbound: dummy_inbound,
            outbound: dummy_outbound,
        })
        .expect("fill coordinator queue");
    let timeouts = ServerHandshakeTimeouts {
        hello: Duration::from_millis(40),
        admission: Duration::from_millis(40),
        welcome: Duration::from_millis(40),
        reconcile: Duration::from_millis(40),
    };
    let (address, mut _connections, server) = {
        let state = DaemonTransportState::with_handshake_timeouts(connection_sender, timeouts);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind backpressure server");
        let address = listener.local_addr().expect("backpressure address");
        let task = tokio::spawn(async move {
            axum::serve(listener, daemon_router(state))
                .await
                .expect("serve backpressure endpoint");
        });
        (address, connection_receiver, task)
    };
    let url = format!("ws://{address}/daemon/ws");

    let (mut no_hello, _) = connect_async(&url).await.expect("connect without hello");
    let closed_without_hello = timeout(Duration::from_millis(300), no_hello.next())
        .await
        .expect("hello timeout must close upgraded socket");
    assert!(matches!(closed_without_hello, None | Some(Err(_))));

    let (mut client_with_hello, _) = connect_async(&url).await.expect("connect with hello");
    let hello = encode_daemon_frame(&DaemonFrame::Hello(Hello::new(
        "daemon-1",
        "credential",
        vec![],
    )))
    .expect("encode hello");
    client_with_hello
        .send(Message::Text(hello.into()))
        .await
        .expect("send hello");
    let closed_without_admission = timeout(Duration::from_millis(300), client_with_hello.next())
        .await
        .expect("admission timeout must close upgraded socket");
    assert!(matches!(closed_without_admission, None | Some(Err(_))));

    server.abort();
}
