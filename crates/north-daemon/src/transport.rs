//! tokio-tungstenite daemon connection supervisor.
//!
//! The supervisor owns connect, hello, welcome/reconcile handshake, split
//! read/write halves, transport ping/pong, bounded outbound buffering,
//! disconnect, failure classification, backoff, and reconnect. Runtime/session
//! code sees only North protocol frames and channels.

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use north_protocol::{
    decode_server_frame, encode_daemon_frame, DaemonFrame, FrameError, Hello, ProtocolErrorFrame,
    ServerFrame,
};
use std::{error::Error, fmt, sync::Arc, time::Duration};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};

pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
pub const OUTBOUND_QUEUE_CAPACITY: usize = 256;
const INCOMING_QUEUE_CAPACITY: usize = 256;
const CONTROL_QUEUE_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPhase {
    Connecting,
    AwaitingWelcome,
    Authenticated,
    Reconciling,
    Active,
}

impl ConnectionPhase {
    fn hello_sent(self) -> Result<Self, ConnectionError> {
        match self {
            Self::Connecting => Ok(Self::AwaitingWelcome),
            _ => Err(ConnectionError::HandshakeViolation(
                "hello sent outside Connecting phase".into(),
            )),
        }
    }

    fn welcome_received(self) -> Result<Self, ConnectionError> {
        match self {
            Self::AwaitingWelcome => Ok(Self::Authenticated),
            _ => Err(ConnectionError::HandshakeViolation(
                "welcome received outside AwaitingWelcome phase".into(),
            )),
        }
    }

    fn begin_reconciliation(self) -> Result<Self, ConnectionError> {
        match self {
            Self::Authenticated => Ok(Self::Reconciling),
            _ => Err(ConnectionError::HandshakeViolation(
                "reconciliation started before authentication".into(),
            )),
        }
    }

    fn reconciliation_received(self) -> Result<Self, ConnectionError> {
        match self {
            Self::Reconciling => Ok(Self::Active),
            _ => Err(ConnectionError::HandshakeViolation(
                "reconciliation received outside Reconciling phase".into(),
            )),
        }
    }

    pub fn allows_application_traffic(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HandshakeTimeouts {
    /// Time allowed to write hello after the socket connects.
    pub hello: Duration,
    /// Time allowed to receive welcome/authentication result.
    pub welcome: Duration,
    /// Time allowed to receive reconciliation state.
    pub reconcile: Duration,
}

impl Default for HandshakeTimeouts {
    fn default() -> Self {
        Self {
            hello: Duration::from_secs(5),
            welcome: Duration::from_secs(10),
            reconcile: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    Retryable,
    Terminal,
    Shutdown,
}

#[derive(Debug, Clone, Copy)]
pub struct ReconnectBackoff {
    pub initial: Duration,
    pub maximum: Duration,
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(30),
        }
    }
}

impl ReconnectBackoff {
    pub fn delay(self, attempt: u32) -> Duration {
        let exponent = attempt.min(16);
        let multiplier = 1u32 << exponent;
        self.initial
            .checked_mul(multiplier)
            .unwrap_or(self.maximum)
            .min(self.maximum)
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub server_url: String,
    pub hello: Hello,
    pub backoff: ReconnectBackoff,
    pub handshake: HandshakeTimeouts,
    pub max_message_size: usize,
    pub max_frame_size: usize,
}

impl ConnectionConfig {
    pub fn new(server_url: impl Into<String>, hello: Hello) -> Self {
        Self {
            server_url: server_url.into(),
            hello,
            backoff: ReconnectBackoff::default(),
            handshake: HandshakeTimeouts::default(),
            max_message_size: MAX_MESSAGE_SIZE,
            max_frame_size: MAX_FRAME_SIZE,
        }
    }
}

#[derive(Debug)]
pub enum ConnectionError {
    Connect(String),
    Socket(String),
    Protocol(FrameError),
    BinaryFrame,
    PeerClosed,
    ChannelClosed(&'static str),
    Task(String),
    HandshakeTimeout(&'static str),
    HandshakeViolation(String),
    TerminalProtocol {
        code: String,
        message: String,
        fatal: bool,
    },
}

impl ConnectionError {
    pub fn failure_class(&self) -> FailureClass {
        match self {
            Self::ChannelClosed(_) => FailureClass::Shutdown,
            Self::Protocol(_)
            | Self::BinaryFrame
            | Self::HandshakeViolation(_)
            | Self::TerminalProtocol { .. } => FailureClass::Terminal,
            Self::Connect(_)
            | Self::Socket(_)
            | Self::PeerClosed
            | Self::Task(_)
            | Self::HandshakeTimeout(_) => FailureClass::Retryable,
        }
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(reason) => write!(f, "tokio-tungstenite connect failed: {reason}"),
            Self::Socket(reason) => write!(f, "WebSocket transport error: {reason}"),
            Self::Protocol(error) => write!(f, "North protocol error: {error}"),
            Self::BinaryFrame => write!(f, "North 0.1 accepts JSON text frames only"),
            Self::PeerClosed => write!(f, "server WebSocket closed"),
            Self::ChannelClosed(side) => write!(f, "{side} connection channel closed"),
            Self::Task(reason) => write!(f, "connection task failed: {reason}"),
            Self::HandshakeTimeout(stage) => write!(f, "handshake timed out waiting for {stage}"),
            Self::HandshakeViolation(reason) => write!(f, "handshake violation: {reason}"),
            Self::TerminalProtocol {
                code,
                message,
                fatal,
            } => write!(
                f,
                "terminal North protocol error ({code}, fatal={fatal}): {message}"
            ),
        }
    }
}

impl Error for ConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

pub struct ConnectionSupervisor {
    config: ConnectionConfig,
}

impl ConnectionSupervisor {
    pub fn new(config: ConnectionConfig) -> Self {
        Self { config }
    }

    pub fn outbound_channel() -> (mpsc::Sender<DaemonFrame>, mpsc::Receiver<DaemonFrame>) {
        mpsc::channel(OUTBOUND_QUEUE_CAPACITY)
    }

    /// Runs one connection lifecycle. Only retryable transport failures enter
    /// local backoff. Protocol/auth/reconciliation failures surface to the host
    /// and stop automatic reconnect; a closed application channel is shutdown.
    pub async fn run(
        &self,
        outbound: mpsc::Receiver<DaemonFrame>,
        inbound: mpsc::Sender<ServerFrame>,
    ) -> Result<(), ConnectionError> {
        let outbound = Arc::new(Mutex::new(outbound));
        let mut attempt = 0;
        loop {
            match self
                .connect_once(Arc::clone(&outbound), inbound.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(error) => match error.failure_class() {
                    FailureClass::Shutdown | FailureClass::Terminal => return Err(error),
                    FailureClass::Retryable => {
                        tokio::time::sleep(self.config.backoff.delay(attempt)).await;
                        attempt = attempt.saturating_add(1);
                    }
                },
            }
        }
    }

    async fn connect_once(
        &self,
        outbound: Arc<Mutex<mpsc::Receiver<DaemonFrame>>>,
        inbound: mpsc::Sender<ServerFrame>,
    ) -> Result<(), ConnectionError> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(self.config.max_message_size))
            .max_frame_size(Some(self.config.max_frame_size))
            .max_write_buffer_size(self.config.max_message_size);
        let (stream, _) = connect_async_with_config(&self.config.server_url, Some(config), true)
            .await
            .map_err(|error| ConnectionError::Connect(error.to_string()))?;
        let (mut writer, reader) = stream.split();

        let hello = encode_daemon_frame(&DaemonFrame::Hello(self.config.hello.clone()))
            .map_err(ConnectionError::Protocol)?;
        tokio::time::timeout(
            self.config.handshake.hello,
            writer.send(Message::Text(hello.into())),
        )
        .await
        .map_err(|_| ConnectionError::HandshakeTimeout("hello"))?
        .map_err(|error| ConnectionError::Socket(error.to_string()))?;

        let mut phase = ConnectionPhase::Connecting.hello_sent()?;
        let (control_sender, control_receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let (incoming_sender, mut incoming_receiver) =
            mpsc::channel::<Result<ServerFrame, ConnectionError>>(INCOMING_QUEUE_CAPACITY);
        let (writer_sender, writer_receiver) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);

        let mut writer_task = tokio::spawn(write_loop(writer, writer_receiver, control_receiver));
        let mut reader_task = tokio::spawn(read_loop(reader, incoming_sender, control_sender));

        let result = async {
            let welcome = receive_frame(
                &mut incoming_receiver,
                self.config.handshake.welcome,
                "welcome",
            )
            .await?;
            if matches!(welcome, ServerFrame::ProtocolError(_)) {
                return Err(protocol_failure(welcome));
            }
            if !matches!(welcome, ServerFrame::Welcome(_)) {
                return Err(ConnectionError::HandshakeViolation(
                    "expected welcome before application traffic".into(),
                ));
            }
            phase = phase.welcome_received()?.begin_reconciliation()?;

            let reconciliation = receive_frame(
                &mut incoming_receiver,
                self.config.handshake.reconcile,
                "reconciliation",
            )
            .await?;
            if matches!(reconciliation, ServerFrame::ProtocolError(_)) {
                return Err(protocol_failure(reconciliation));
            }
            if !matches!(reconciliation, ServerFrame::Reconcile(_)) {
                return Err(ConnectionError::HandshakeViolation(
                    "expected reconcile before application traffic".into(),
                ));
            }
            phase = phase.reconciliation_received()?;
            debug_assert!(phase.allows_application_traffic());

            loop {
                tokio::select! {
                    incoming = incoming_receiver.recv() => match incoming {
                        Some(Ok(ServerFrame::ProtocolError(error))) => {
                            return Err(protocol_failure(ServerFrame::ProtocolError(error)));
                        }
                        Some(Ok(ServerFrame::Welcome(_))) => {
                            return Err(ConnectionError::HandshakeViolation(
                                "welcome repeated after Active".into(),
                            ));
                        }
                        Some(Ok(frame)) => {
                            inbound.send(frame).await.map_err(|_| ConnectionError::ChannelClosed("inbound"))?;
                        }
                        Some(Err(error)) => return Err(error),
                        None => return Err(ConnectionError::PeerClosed),
                    },
                    frame = next_outbound(&outbound), if phase.allows_application_traffic() => match frame {
                        Some(frame) => writer_sender.send(WriterCommand::Frame(frame)).await.map_err(|_| ConnectionError::PeerClosed)?,
                        None => return Err(ConnectionError::ChannelClosed("outbound")),
                    },
                    result = &mut reader_task => {
                        writer_task.abort();
                        return result.map_err(|error| ConnectionError::Task(error.to_string()))?;
                    }
                    result = &mut writer_task => {
                        reader_task.abort();
                        return result.map_err(|error| ConnectionError::Task(error.to_string()))?;
                    }
                }
            }
        }
        .await;

        reader_task.abort();
        writer_task.abort();
        result
    }
}

type SharedOutbound = Arc<Mutex<mpsc::Receiver<DaemonFrame>>>;

enum WriterCommand {
    Frame(DaemonFrame),
}

async fn next_outbound(outbound: &SharedOutbound) -> Option<DaemonFrame> {
    outbound.lock().await.recv().await
}

async fn receive_frame(
    incoming: &mut mpsc::Receiver<Result<ServerFrame, ConnectionError>>,
    duration: Duration,
    stage: &'static str,
) -> Result<ServerFrame, ConnectionError> {
    tokio::time::timeout(duration, incoming.recv())
        .await
        .map_err(|_| ConnectionError::HandshakeTimeout(stage))?
        .ok_or(ConnectionError::PeerClosed)?
}

fn protocol_failure(frame: ServerFrame) -> ConnectionError {
    let ServerFrame::ProtocolError(ProtocolErrorFrame {
        code,
        message,
        fatal,
        ..
    }) = frame
    else {
        return ConnectionError::HandshakeViolation("expected protocol error frame".into());
    };
    ConnectionError::TerminalProtocol {
        code,
        message,
        fatal,
    }
}

async fn write_loop<W>(
    mut writer: W,
    mut commands: mpsc::Receiver<WriterCommand>,
    mut controls: mpsc::Receiver<Message>,
) -> Result<(), ConnectionError>
where
    W: Sink<Message> + Unpin,
    W::Error: fmt::Display,
{
    loop {
        tokio::select! {
            control = controls.recv() => match control {
                Some(message) => writer.send(message).await.map_err(|error| ConnectionError::Socket(error.to_string()))?,
                None => return Err(ConnectionError::PeerClosed),
            },
            command = commands.recv() => match command {
                Some(WriterCommand::Frame(frame)) => send_frame(&mut writer, frame).await?,
                None => return Err(ConnectionError::ChannelClosed("writer")),
            },
        }
    }
}

async fn send_frame<W>(writer: &mut W, frame: DaemonFrame) -> Result<(), ConnectionError>
where
    W: Sink<Message> + Unpin,
    W::Error: fmt::Display,
{
    let text = encode_daemon_frame(&frame).map_err(ConnectionError::Protocol)?;
    writer
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| ConnectionError::Socket(error.to_string()))
}

async fn read_loop<R, E>(
    mut reader: R,
    incoming: mpsc::Sender<Result<ServerFrame, ConnectionError>>,
    controls: mpsc::Sender<Message>,
) -> Result<(), ConnectionError>
where
    R: Stream<Item = Result<Message, E>> + Unpin,
    E: fmt::Display,
{
    while let Some(message) = reader.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                let _ = incoming
                    .send(Err(ConnectionError::Socket(error.to_string())))
                    .await;
                return Ok(());
            }
        };
        match message {
            Message::Text(text) => {
                let frame = match decode_server_frame(text.as_ref()) {
                    Ok(frame) => frame,
                    Err(error) => {
                        let _ = incoming.send(Err(ConnectionError::Protocol(error))).await;
                        return Ok(());
                    }
                };
                if incoming.send(Ok(frame)).await.is_err() {
                    return Err(ConnectionError::ChannelClosed("incoming"));
                }
            }
            Message::Binary(_) => {
                let _ = incoming.send(Err(ConnectionError::BinaryFrame)).await;
                return Ok(());
            }
            Message::Ping(payload) => {
                if controls.send(Message::Pong(payload)).await.is_err() {
                    return Err(ConnectionError::PeerClosed);
                }
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                let _ = incoming.send(Err(ConnectionError::PeerClosed)).await;
                return Ok(());
            }
            Message::Frame(_) => {
                let _ = incoming
                    .send(Err(ConnectionError::Socket(
                        "unexpected raw WebSocket frame".into(),
                    )))
                    .await;
                return Ok(());
            }
        }
    }
    let _ = incoming.send(Err(ConnectionError::PeerClosed)).await;
    Ok(())
}

pub fn encode_daemon_message(frame: &DaemonFrame) -> Result<Message, ConnectionError> {
    Ok(Message::Text(
        encode_daemon_frame(frame)
            .map_err(ConnectionError::Protocol)?
            .into(),
    ))
}

pub fn decode_server_message(message: Message) -> Result<Option<ServerFrame>, ConnectionError> {
    match message {
        Message::Text(text) => decode_server_frame(text.as_ref())
            .map(Some)
            .map_err(ConnectionError::Protocol),
        Message::Binary(_) => Err(ConnectionError::BinaryFrame),
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Err(ConnectionError::PeerClosed),
        Message::Frame(_) => Err(ConnectionError::Socket(
            "unexpected raw WebSocket frame".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_protocol::{ProtocolErrorFrame, SCHEMA_VERSION};

    #[test]
    fn phases_gate_application_traffic_until_reconciliation() {
        assert!(!ConnectionPhase::Connecting.allows_application_traffic());
        assert!(!ConnectionPhase::AwaitingWelcome.allows_application_traffic());
        assert!(!ConnectionPhase::Authenticated.allows_application_traffic());
        assert!(!ConnectionPhase::Reconciling.allows_application_traffic());
        assert!(ConnectionPhase::Active.allows_application_traffic());
    }

    #[test]
    fn phase_transitions_are_ordered() {
        let phase = ConnectionPhase::Connecting
            .hello_sent()
            .unwrap()
            .welcome_received()
            .unwrap()
            .begin_reconciliation()
            .unwrap()
            .reconciliation_received()
            .unwrap();
        assert_eq!(phase, ConnectionPhase::Active);
        assert!(ConnectionPhase::Connecting.welcome_received().is_err());
    }

    #[tokio::test]
    async fn handshake_timeout_is_retryable() {
        let (_sender, mut receiver) = mpsc::channel(1);
        let error = receive_frame(&mut receiver, Duration::from_millis(1), "welcome")
            .await
            .expect_err("empty handshake must time out");
        assert_eq!(error.failure_class(), FailureClass::Retryable);
    }

    #[test]
    fn fatal_protocol_error_is_terminal() {
        let error = protocol_failure(ServerFrame::ProtocolError(ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: "credential_revoked".into(),
            message: "daemon credential revoked".into(),
            fatal: true,
        }));
        assert_eq!(error.failure_class(), FailureClass::Terminal);
    }

    #[test]
    fn backoff_is_bounded() {
        let backoff = ReconnectBackoff {
            initial: Duration::from_secs(1),
            maximum: Duration::from_secs(5),
        };
        assert_eq!(backoff.delay(0), Duration::from_secs(1));
        assert_eq!(backoff.delay(3), Duration::from_secs(5));
        assert_eq!(backoff.delay(20), Duration::from_secs(5));
    }

    #[test]
    fn daemon_frames_use_text_messages_and_server_protocol_errors_decode() {
        let frame = DaemonFrame::Hello(Hello::new("daemon-1", "credential", vec![]));
        let Message::Text(text) = encode_daemon_message(&frame).expect("encode") else {
            panic!("North transport must use text frames");
        };
        assert!(matches!(
            decode_server_message(Message::Text(text)),
            Err(ConnectionError::Protocol(_))
        ));

        let server_frame = ServerFrame::ProtocolError(ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: "unsupported_version".into(),
            message: "upgrade required".into(),
            fatal: true,
        });
        let json = server_frame.to_json().expect("protocol JSON");
        let decoded = decode_server_message(Message::Text(json.into()))
            .expect("decode")
            .expect("application frame");
        assert_eq!(decoded, server_frame);
    }
}
