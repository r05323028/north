//! tokio-tungstenite daemon connection supervisor.
//!
//! The supervisor owns connect, hello, welcome/reconciliation handshake, split
//! read/write halves, transport ping/pong, bounded outbound buffering,
//! disconnect, failure classification, backoff, and reconnect. It delivers the
//! handshake result to coordination and waits for readiness before application
//! traffic. Runtime/session code sees only North protocol values and channels.

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use north_protocol::{
    decode_server_frame, encode_daemon_frame, DaemonFrame, FrameError, Hello, ProtocolErrorFrame,
    ReconcileSnapshot, ServerFrame, Welcome,
};
use std::{error::Error, fmt, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, Mutex};
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
    ReconciliationReceived,
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
            Self::Reconciling => Ok(Self::ReconciliationReceived),
            _ => Err(ConnectionError::HandshakeViolation(
                "reconciliation received outside Reconciling phase".into(),
            )),
        }
    }

    fn coordination_ready(self) -> Result<Self, ConnectionError> {
        match self {
            Self::ReconciliationReceived => Ok(Self::Active),
            _ => Err(ConnectionError::HandshakeViolation(
                "coordination became ready before reconciliation was received".into(),
            )),
        }
    }

    pub fn allows_application_traffic(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeResult {
    pub welcome: Welcome,
    pub reconciliation: ReconcileSnapshot,
}

/// Values delivered from the transport supervisor to North coordination.
#[derive(Debug)]
pub enum ConnectionEvent {
    HandshakeComplete {
        result: HandshakeResult,
        ready: oneshot::Sender<()>,
    },
    Frame(ServerFrame),
}

#[derive(Debug, Clone, Copy)]
pub struct HandshakeTimeouts {
    /// Time allowed to write hello after the socket connects.
    pub hello: Duration,
    /// Time allowed to receive welcome/authentication result.
    pub welcome: Duration,
    /// Time allowed to receive reconciliation state.
    pub reconcile: Duration,
    /// Time allowed for coordination to apply reconciliation and signal ready.
    pub coordination: Duration,
}

impl Default for HandshakeTimeouts {
    fn default() -> Self {
        Self {
            hello: Duration::from_secs(5),
            welcome: Duration::from_secs(10),
            reconcile: Duration::from_secs(10),
            coordination: Duration::from_secs(10),
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
    TerminalProtocol { code: String, message: String },
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
            Self::TerminalProtocol { code, message } => {
                write!(f, "terminal North protocol error ({code}): {message}")
            }
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
    /// Coordination receives `HandshakeComplete` and must signal readiness before
    /// the supervisor releases queued application traffic.
    pub async fn run(
        &self,
        outbound: mpsc::Receiver<DaemonFrame>,
        inbound: mpsc::Sender<ConnectionEvent>,
    ) -> Result<(), ConnectionError> {
        let outbound = Arc::new(Mutex::new(outbound));
        let mut attempt = 0;
        loop {
            match self
                .connect_once(Arc::clone(&outbound), inbound.clone(), &mut attempt)
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
        inbound: mpsc::Sender<ConnectionEvent>,
        attempt: &mut u32,
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
            mpsc::channel::<ServerFrame>(INCOMING_QUEUE_CAPACITY);
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
            let welcome = match welcome {
                ServerFrame::Welcome(welcome) => welcome,
                ServerFrame::ProtocolError(error) => {
                    return Err(protocol_failure(ServerFrame::ProtocolError(error)));
                }
                _ => {
                    return Err(ConnectionError::HandshakeViolation(
                        "expected welcome before application traffic".into(),
                    ));
                }
            };
            phase = phase.welcome_received()?.begin_reconciliation()?;

            let reconciliation = receive_frame(
                &mut incoming_receiver,
                self.config.handshake.reconcile,
                "reconciliation",
            )
            .await?;
            let reconciliation = match reconciliation {
                ServerFrame::Reconcile(reconciliation) => reconciliation,
                ServerFrame::ProtocolError(error) => {
                    return Err(protocol_failure(ServerFrame::ProtocolError(error)));
                }
                _ => {
                    return Err(ConnectionError::HandshakeViolation(
                        "expected reconcile before application traffic".into(),
                    ));
                }
            };
            phase = phase.reconciliation_received()?;

            let (ready, coordination) = oneshot::channel();
            tokio::time::timeout(
                self.config.handshake.coordination,
                inbound.send(ConnectionEvent::HandshakeComplete {
                    result: HandshakeResult {
                        welcome,
                        reconciliation,
                    },
                    ready,
                }),
            )
            .await
            .map_err(|_| ConnectionError::HandshakeTimeout("coordination"))?
            .map_err(|_| ConnectionError::ChannelClosed("inbound"))?;
            tokio::time::timeout(self.config.handshake.coordination, coordination)
                .await
                .map_err(|_| ConnectionError::HandshakeTimeout("coordination"))?
                .map_err(|_| ConnectionError::ChannelClosed("coordination"))?;
            phase = phase.coordination_ready()?;
            reset_transport_backoff(attempt);
            debug_assert!(phase.allows_application_traffic());

            loop {
                tokio::select! {
                    incoming = incoming_receiver.recv() => match incoming {
                        Some(ServerFrame::ProtocolError(error)) => {
                            return Err(protocol_failure(ServerFrame::ProtocolError(error)));
                        }
                        Some(ServerFrame::Welcome(_)) => {
                            return Err(ConnectionError::HandshakeViolation(
                                "welcome repeated after Active".into(),
                            ));
                        }
                        Some(ServerFrame::Reconcile(_)) => {
                            return Err(ConnectionError::HandshakeViolation(
                                "reconciliation repeated after Active".into(),
                            ));
                        }
                        Some(frame) => {
                            inbound
                                .send(ConnectionEvent::Frame(frame))
                                .await
                                .map_err(|_| ConnectionError::ChannelClosed("inbound"))?;
                        }
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
    incoming: &mut mpsc::Receiver<ServerFrame>,
    duration: Duration,
    stage: &'static str,
) -> Result<ServerFrame, ConnectionError> {
    tokio::time::timeout(duration, incoming.recv())
        .await
        .map_err(|_| ConnectionError::HandshakeTimeout(stage))?
        .ok_or(ConnectionError::PeerClosed)
}

fn protocol_failure(frame: ServerFrame) -> ConnectionError {
    let ServerFrame::ProtocolError(ProtocolErrorFrame { code, message, .. }) = frame else {
        return ConnectionError::HandshakeViolation("expected protocol error frame".into());
    };
    ConnectionError::TerminalProtocol { code, message }
}

fn reset_transport_backoff(attempt: &mut u32) {
    *attempt = 0;
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
    incoming: mpsc::Sender<ServerFrame>,
    controls: mpsc::Sender<Message>,
) -> Result<(), ConnectionError>
where
    R: Stream<Item = Result<Message, E>> + Unpin,
    E: fmt::Display,
{
    while let Some(message) = reader.next().await {
        let message = message.map_err(|error| ConnectionError::Socket(error.to_string()))?;
        match message {
            Message::Text(text) => {
                let frame =
                    decode_server_frame(text.as_ref()).map_err(ConnectionError::Protocol)?;
                incoming
                    .send(frame)
                    .await
                    .map_err(|_| ConnectionError::ChannelClosed("incoming"))?;
            }
            Message::Binary(_) => return Err(ConnectionError::BinaryFrame),
            Message::Ping(payload) => {
                if controls.send(Message::Pong(payload)).await.is_err() {
                    return Err(ConnectionError::PeerClosed);
                }
            }
            Message::Pong(_) => {}
            Message::Close(_) => return Err(ConnectionError::PeerClosed),
            Message::Frame(_) => {
                return Err(ConnectionError::Socket(
                    "unexpected raw WebSocket frame".into(),
                ));
            }
        }
    }
    Err(ConnectionError::PeerClosed)
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
        assert!(!ConnectionPhase::ReconciliationReceived.allows_application_traffic());
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
        assert_eq!(phase, ConnectionPhase::ReconciliationReceived);
        assert!(!phase.allows_application_traffic());
        let phase = phase.coordination_ready().unwrap();
        assert_eq!(phase, ConnectionPhase::Active);
        assert!(phase.allows_application_traffic());
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
    fn protocol_error_is_terminal() {
        let error = protocol_failure(ServerFrame::ProtocolError(ProtocolErrorFrame {
            schema_version: SCHEMA_VERSION,
            code: "credential_revoked".into(),
            message: "daemon credential revoked".into(),
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
    fn healthy_connection_resets_transport_backoff() {
        let mut attempt = 7;
        let phase = ConnectionPhase::ReconciliationReceived
            .coordination_ready()
            .expect("coordination can activate after reconciliation");
        assert!(phase.allows_application_traffic());
        reset_transport_backoff(&mut attempt);
        assert_eq!(attempt, 0);
        assert_eq!(
            ReconnectBackoff::default().delay(attempt),
            Duration::from_secs(1)
        );
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
        });
        let json = server_frame.to_json().expect("protocol JSON");
        let decoded = decode_server_message(Message::Text(json.into()))
            .expect("decode")
            .expect("application frame");
        assert_eq!(decoded, server_frame);
    }
}
