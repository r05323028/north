//! Axum WebSocket adapter for the daemon connection.
//!
//! This module owns HTTP upgrade, WebSocket frames, ping/pong, close handling,
//! size limits, and the single transport writer. It does not authenticate
//! credentials or mutate business state; the connection coordinator receives
//! the first `DaemonFrame::Hello` and performs daemon authentication,
//! registration, and reconciliation.

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
use north_protocol::{DaemonFrame, FrameError, ServerFrame};
use std::{error::Error, fmt};
use tokio::sync::mpsc;

pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
pub const INBOUND_QUEUE_CAPACITY: usize = 256;
pub const OUTBOUND_QUEUE_CAPACITY: usize = 256;
pub const CONNECTION_QUEUE_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum TransportError {
    Socket(String),
    Protocol(FrameError),
    BinaryFrame,
    ExpectedHello,
    ChannelClosed,
    Task(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket(reason) => write!(f, "Axum WebSocket transport error: {reason}"),
            Self::Protocol(error) => write!(f, "North protocol error: {error}"),
            Self::BinaryFrame => write!(f, "North 0.1 accepts JSON text frames only"),
            Self::ExpectedHello => write!(f, "first daemon frame must be hello"),
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
}

impl DaemonTransportState {
    pub fn new(connections: mpsc::Sender<DaemonConnection>) -> Self {
        Self { connections }
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

/// Thin Axum upgrade handler. Credential authentication and business routing
/// happen after the upgrade in the protocol/session coordinator.
pub async fn daemon_websocket_handler(
    State(state): State<DaemonTransportState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_FRAME_SIZE)
        .on_upgrade(move |socket| async move {
            let (inbound_sender, inbound) = mpsc::channel(INBOUND_QUEUE_CAPACITY);
            let (outbound, outbound_receiver) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
            let connection = DaemonConnection { inbound, outbound };
            if state.connections.send(connection).await.is_err() {
                return;
            }
            let _ = serve_websocket(socket, inbound_sender, outbound_receiver).await;
        })
}

/// Runs one upgraded connection. Axum owns WebSocket protocol behavior; this
/// adapter only maps text messages to/from North protocol frames and forwards
/// them through bounded channels.
pub async fn serve_websocket(
    socket: WebSocket,
    inbound: mpsc::Sender<DaemonFrame>,
    outbound: mpsc::Receiver<ServerFrame>,
) -> Result<(), TransportError> {
    let (mut writer, mut reader) = socket.split();

    let mut reader_task = tokio::spawn(async move {
        let mut first_application_frame = true;
        while let Some(message) = reader.next().await {
            let message = message.map_err(|error| TransportError::Socket(error.to_string()))?;
            let Some(frame) = decode_daemon_message(message)? else {
                continue;
            };
            if first_application_frame {
                if !matches!(frame, DaemonFrame::Hello(_)) {
                    return Err(TransportError::ExpectedHello);
                }
                first_application_frame = false;
            }
            inbound
                .send(frame)
                .await
                .map_err(|_| TransportError::ChannelClosed)?;
        }
        Ok::<(), TransportError>(())
    });

    let mut writer_task = tokio::spawn(async move {
        let mut outbound = outbound;
        while let Some(frame) = outbound.recv().await {
            let message = encode_server_message(&frame)?;
            writer
                .send(message)
                .await
                .map_err(|error| TransportError::Socket(error.to_string()))?;
        }
        Ok::<(), TransportError>(())
    });

    tokio::select! {
        result = &mut reader_task => {
            writer_task.abort();
            result.map_err(|error| TransportError::Task(error.to_string()))?
        }
        result = &mut writer_task => {
            reader_task.abort();
            result.map_err(|error| TransportError::Task(error.to_string()))?
        }
    }
}

pub fn encode_server_message(frame: &ServerFrame) -> Result<Message, TransportError> {
    Ok(Message::Text(
        frame.to_json().map_err(TransportError::Protocol)?.into(),
    ))
}

pub fn decode_daemon_message(message: Message) -> Result<Option<DaemonFrame>, TransportError> {
    match message {
        Message::Text(text) => DaemonFrame::from_json(text.as_ref())
            .map(Some)
            .map_err(TransportError::Protocol),
        Message::Binary(_) => Err(TransportError::BinaryFrame),
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(None),
    }
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
            fatal: true,
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
}
