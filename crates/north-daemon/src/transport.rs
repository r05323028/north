//! tokio-tungstenite daemon connection supervisor.
//!
//! The supervisor owns connect, hello, split read/write halves, transport
//! ping/pong, bounded outbound buffering, disconnect, backoff, and reconnect.
//! Runtime/session code sees only North protocol frames and channels.

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use north_protocol::{
    decode_server_frame, encode_daemon_frame, DaemonFrame, FrameError, Hello, ServerFrame,
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
const CONTROL_QUEUE_CAPACITY: usize = 8;

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
    pub max_message_size: usize,
    pub max_frame_size: usize,
}

impl ConnectionConfig {
    pub fn new(server_url: impl Into<String>, hello: Hello) -> Self {
        Self {
            server_url: server_url.into(),
            hello,
            backoff: ReconnectBackoff::default(),
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

    /// Runs one connection lifecycle. Transport failures reconnect with local
    /// backoff; a closed application channel is a deliberate shutdown. North
    /// retry budgets and session attempt counts do not live here.
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
                Err(error @ ConnectionError::ChannelClosed(_)) => return Err(error),
                Err(_transport_error) => {
                    tokio::time::sleep(self.config.backoff.delay(attempt)).await;
                    attempt = attempt.saturating_add(1);
                }
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
        let (writer, reader) = stream.split();
        let (control_sender, control_receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let hello = self.config.hello.clone();

        let mut writer_task = tokio::spawn(write_loop(writer, outbound, control_receiver, hello));
        let mut reader_task = tokio::spawn(read_loop(reader, inbound, control_sender));

        tokio::select! {
            result = &mut writer_task => {
                reader_task.abort();
                result.map_err(|error| ConnectionError::Task(error.to_string()))?
            }
            result = &mut reader_task => {
                writer_task.abort();
                result.map_err(|error| ConnectionError::Task(error.to_string()))?
            }
        }
    }
}

type SharedOutbound = Arc<Mutex<mpsc::Receiver<DaemonFrame>>>;

async fn next_outbound(outbound: &SharedOutbound) -> Option<DaemonFrame> {
    outbound.lock().await.recv().await
}

async fn write_loop<W>(
    mut writer: W,
    outbound: SharedOutbound,
    mut controls: mpsc::Receiver<Message>,
    hello: Hello,
) -> Result<(), ConnectionError>
where
    W: Sink<Message> + Unpin,
    W::Error: fmt::Display,
{
    send_frame(&mut writer, DaemonFrame::Hello(hello)).await?;
    loop {
        tokio::select! {
            control = controls.recv() => match control {
                Some(message) => writer.send(message).await.map_err(|error| ConnectionError::Socket(error.to_string()))?,
                None => return Err(ConnectionError::PeerClosed),
            },
            frame = next_outbound(&outbound) => match frame {
                Some(frame) => send_frame(&mut writer, frame).await?,
                None => return Err(ConnectionError::ChannelClosed("outbound")),
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
    inbound: mpsc::Sender<ServerFrame>,
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
                inbound
                    .send(frame)
                    .await
                    .map_err(|_| ConnectionError::ChannelClosed("inbound"))?;
            }
            Message::Binary(_) => return Err(ConnectionError::BinaryFrame),
            Message::Ping(payload) => controls
                .send(Message::Pong(payload))
                .await
                .map_err(|_| ConnectionError::PeerClosed)?,
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
