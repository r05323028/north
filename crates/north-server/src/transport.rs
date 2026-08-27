//! Axum WebSocket adapter for the daemon connection.
//!
//! This module owns HTTP upgrade, WebSocket frames, ping/pong, close handling,
//! size limits, and the single transport writer. It does not authenticate
//! credentials or mutate business state; the adapter reads the first
//! `DaemonFrame::Hello` before bounded coordinator admission, then the
//! connection coordinator performs daemon authentication, registration, and
//! reconciliation.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use north_protocol::{
    DaemonFrame, FrameError, ProtocolErrorFrame, ServerFrame, PROTOCOL_VERSION, SCHEMA_VERSION,
};
use std::{error::Error, fmt, future::Future, time::Duration};
use tokio::sync::mpsc;

pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
pub const INBOUND_QUEUE_CAPACITY: usize = 256;
pub const OUTBOUND_QUEUE_CAPACITY: usize = 256;
pub const CONNECTION_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct HandshakeTimeouts {
    /// Time allowed for the first application `hello` after upgrade.
    pub hello: Duration,
    /// Time allowed for the coordinator to admit a hello-bearing connection.
    pub admission: Duration,
}

impl Default for HandshakeTimeouts {
    fn default() -> Self {
        Self {
            hello: Duration::from_secs(5),
            admission: Duration::from_secs(5),
        }
    }
}

#[derive(Debug)]
pub enum TransportError {
    Socket(String),
    Protocol(FrameError),
    IncompatibleProtocol,
    BinaryFrame,
    ExpectedHello,
    HandshakeTimeout(&'static str),
    PeerClosed,
    ChannelClosed,
    Task(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket(reason) => write!(f, "Axum WebSocket transport error: {reason}"),
            Self::Protocol(error) => write!(f, "North protocol error: {error}"),
            Self::IncompatibleProtocol => write!(f, "unsupported North protocol version"),
            Self::BinaryFrame => write!(f, "North 0.1 accepts JSON text frames only"),
            Self::ExpectedHello => write!(f, "first daemon frame must be hello"),
            Self::HandshakeTimeout(stage) => write!(f, "handshake timed out waiting for {stage}"),
            Self::PeerClosed => write!(f, "daemon WebSocket closed"),
            Self::ChannelClosed => write!(f, "daemon connection channel closed"),
            Self::Task(reason) => write!(f, "WebSocket task failed: {reason}"),
        }
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

/// Per-connection channels crossing the transport/coordinator boundary.
/// `inbound` is owned by protocol/session coordination; `outbound` is the
/// only path the coordinator gets to the WebSocket writer.
pub struct DaemonConnection {
    pub inbound: mpsc::Receiver<DaemonFrame>,
    pub outbound: mpsc::Sender<ServerFrame>,
}

#[derive(Clone)]
pub struct DaemonTransportState {
    connections: mpsc::Sender<DaemonConnection>,
    handshake_timeouts: HandshakeTimeouts,
}

impl DaemonTransportState {
    pub fn new(connections: mpsc::Sender<DaemonConnection>) -> Self {
        Self::with_handshake_timeouts(connections, HandshakeTimeouts::default())
    }

    pub fn with_handshake_timeouts(
        connections: mpsc::Sender<DaemonConnection>,
        handshake_timeouts: HandshakeTimeouts,
    ) -> Self {
        Self {
            connections,
            handshake_timeouts,
        }
    }

    pub fn channel() -> (Self, mpsc::Receiver<DaemonConnection>) {
        let (sender, receiver) = mpsc::channel(CONNECTION_QUEUE_CAPACITY);
        (Self::new(sender), receiver)
    }
}

/// Mounts the daemon endpoint. Browser routes remain HTTP/SSE routes owned by
/// the host application; this router only adds the daemon WebSocket boundary.
pub fn daemon_router(state: DaemonTransportState) -> Router {
    Router::new()
        .route("/daemon/ws", get(daemon_websocket_handler))
        .with_state(state)
}

/// Thin Axum upgrade handler. The adapter starts the hello timeout immediately
/// after upgrade, then admits the hello-bearing connection with its own bound.
pub async fn daemon_websocket_handler(
    State(state): State<DaemonTransportState>,
    ws: WebSocketUpgrade,
) -> Response {
    daemon_websocket_response(ws, state)
}

pub fn daemon_websocket_response(ws: WebSocketUpgrade, state: DaemonTransportState) -> Response {
    let handshake_timeouts = state.handshake_timeouts;
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_FRAME_SIZE)
        .on_upgrade(move |socket| async move {
            let (inbound_sender, inbound) = mpsc::channel(INBOUND_QUEUE_CAPACITY);
            let (outbound, outbound_receiver) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
            let admission = async move {
                state
                    .connections
                    .send(DaemonConnection { inbound, outbound })
                    .await
                    .map_err(|_| TransportError::ChannelClosed)
            };
            let _ = serve_websocket_with_admission(
                socket,
                inbound_sender,
                outbound_receiver,
                handshake_timeouts,
                admission,
            )
            .await;
        })
}

/// Runs one upgraded connection without coordinator admission. Tests and
/// embedders use this thin adapter entry point; the endpoint uses the bounded
/// admission variant below.
pub async fn serve_websocket(
    socket: WebSocket,
    inbound: mpsc::Sender<DaemonFrame>,
    outbound: mpsc::Receiver<ServerFrame>,
    handshake_timeouts: HandshakeTimeouts,
) -> Result<(), TransportError> {
    serve_websocket_with_admission(
        socket,
        inbound,
        outbound,
        handshake_timeouts,
        std::future::ready(Ok::<(), TransportError>(())),
    )
    .await
}

async fn serve_websocket_with_admission<F>(
    socket: WebSocket,
    inbound: mpsc::Sender<DaemonFrame>,
    mut outbound: mpsc::Receiver<ServerFrame>,
    handshake_timeouts: HandshakeTimeouts,
    admission: F,
) -> Result<(), TransportError>
where
    F: Future<Output = Result<(), TransportError>>,
{
    let (mut writer, mut reader) = socket.split();
    let hello = match tokio::time::timeout(handshake_timeouts.hello, async {
        loop {
            let message = reader.next().await.ok_or(TransportError::PeerClosed)?;
            let message = message.map_err(|error| TransportError::Socket(error.to_string()))?;
            match message {
                Message::Ping(payload) => {
                    writer
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| TransportError::Socket(error.to_string()))?;
                }
                Message::Pong(_) => {}
                message => {
                    if let Some(frame) = decode_daemon_message(message)? {
                        break Ok::<DaemonFrame, TransportError>(frame);
                    }
                }
            }
        }
    })
    .await
    {
        Err(_) => return Err(TransportError::HandshakeTimeout("hello")),
        Ok(Ok(frame)) => frame,
        Ok(Err(error)) => {
            if let Some(frame) = protocol_error_frame(&error) {
                let _ = writer.send(encode_server_message(&frame)?).await;
            }
            return Err(error);
        }
    };

    if !matches!(hello, DaemonFrame::Hello(_)) {
        let error = TransportError::ExpectedHello;
        if let Some(frame) = protocol_error_frame(&error) {
            let _ = writer.send(encode_server_message(&frame)?).await;
        }
        return Err(error);
    }
    inbound
        .send(hello)
        .await
        .map_err(|_| TransportError::ChannelClosed)?;

    tokio::time::timeout(handshake_timeouts.admission, admission)
        .await
        .map_err(|_| TransportError::HandshakeTimeout("coordinator admission"))??;

    loop {
        tokio::select! {
            message = reader.next() => {
                let message = message.ok_or(TransportError::PeerClosed)?;
                let message = message.map_err(|error| TransportError::Socket(error.to_string()))?;
                match message {
                    Message::Ping(payload) => writer
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| TransportError::Socket(error.to_string()))?,
                    Message::Pong(_) => {}
                    message => {
                        let frame = match decode_daemon_message(message) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => continue,
                            Err(error) => {
                                if let Some(frame) = protocol_error_frame(&error) {
                                    writer
                                        .send(encode_server_message(&frame)?)
                                        .await
                                        .map_err(|error| TransportError::Socket(error.to_string()))?;
                                }
                                return Err(error);
                            }
                        };
                        inbound
                            .send(frame)
                            .await
                            .map_err(|_| TransportError::ChannelClosed)?;
                    }
                }
            }
            frame = outbound.recv() => match frame {
                Some(frame) => writer
                    .send(encode_server_message(&frame)?)
                    .await
                    .map_err(|error| TransportError::Socket(error.to_string()))?,
                None => return Ok(()),
            },
        }
    }
}

fn protocol_error_frame(error: &TransportError) -> Option<ServerFrame> {
    let code = match error {
        TransportError::Protocol(_) => "invalid_frame",
        TransportError::IncompatibleProtocol => "incompatible_protocol",
        TransportError::BinaryFrame => "binary_frame",
        TransportError::ExpectedHello => "expected_hello",
        _ => return None,
    };
    Some(ServerFrame::ProtocolError(ProtocolErrorFrame {
        schema_version: SCHEMA_VERSION,
        code: code.into(),
        message: error.to_string(),
    }))
}

pub fn encode_server_message(frame: &ServerFrame) -> Result<Message, TransportError> {
    Ok(Message::Text(
        frame.to_json().map_err(TransportError::Protocol)?.into(),
    ))
}

pub fn decode_daemon_message(message: Message) -> Result<Option<DaemonFrame>, TransportError> {
    match message {
        Message::Text(text) if has_incompatible_protocol(text.as_ref()) => {
            Err(TransportError::IncompatibleProtocol)
        }
        Message::Text(text) => DaemonFrame::from_json(text.as_ref())
            .map(Some)
            .map_err(TransportError::Protocol),
        Message::Binary(_) => Err(TransportError::BinaryFrame),
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Err(TransportError::PeerClosed),
    }
}

fn has_incompatible_protocol(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    value.get("frame").and_then(serde_json::Value::as_str) == Some("hello")
        && value
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .and_then(|payload| payload.get("protocol_version"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|version| version != PROTOCOL_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_protocol::{Heartbeat, SCHEMA_VERSION};

    #[test]
    fn server_frames_use_text_messages_and_daemon_binary_is_rejected() {
        let frame = ServerFrame::ProtocolError(north_protocol::ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: "test".into(),
            message: "no side effect".into(),
        });
        let Message::Text(text) = encode_server_message(&frame).expect("encode") else {
            panic!("North transport must use text frames");
        };
        assert_eq!(
            ServerFrame::from_json(text.as_ref()).expect("decode"),
            frame
        );
        assert!(matches!(
            decode_daemon_message(Message::Binary(Vec::new().into())),
            Err(TransportError::BinaryFrame)
        ));
    }

    #[test]
    fn transport_ping_pong_is_not_application_heartbeat() {
        assert!(decode_daemon_message(Message::Ping(Vec::new().into()))
            .expect("ping is transport control")
            .is_none());
        let _heartbeat = Heartbeat {
            schema_version: SCHEMA_VERSION,
            daemon_id: "daemon-1".into(),
            sent_at: "2026-01-01T00:00:00Z".into(),
            application_state: "connected".into(),
        };
    }

    #[test]
    fn protocol_version_mismatch_gets_incompatibility_error() {
        let message = Message::Text(
            r#"{"frame":"hello","payload":{"protocol_version":"0.2","schema_version":1,"daemon_id":"daemon-1","credential":"secret","capabilities":[]}}"#.into(),
        );
        let error = decode_daemon_message(message).expect_err("version mismatch");
        assert!(matches!(error, TransportError::IncompatibleProtocol));
        let ServerFrame::ProtocolError(frame) = protocol_error_frame(&error).expect("error frame")
        else {
            panic!("expected protocol error");
        };
        assert_eq!(frame.code, "incompatible_protocol");
    }

    #[test]
    fn handshake_timeouts_have_explicit_stages() {
        let timeouts = HandshakeTimeouts::default();
        assert_eq!(timeouts.hello, Duration::from_secs(5));
        assert_eq!(timeouts.admission, Duration::from_secs(5));
    }
}
