use north_daemon::transport::{ConnectionConfig, ConnectionEvent, ConnectionSupervisor};
use north_protocol::{
    DaemonFrame, Heartbeat, Hello, ReconcileSnapshot, ServerFrame, SessionReconcileState,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

#[derive(Debug)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CliError {}

#[derive(Debug)]
enum CurlError {
    Retryable(String),
    Terminal(String),
}

impl fmt::Display for CurlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(message) => write!(f, "retryable curl failure: {message}"),
            Self::Terminal(message) => f.write_str(message),
        }
    }
}

impl From<CurlError> for CliError {
    fn from(error: CurlError) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct SetupCreated {
    request_token: String,
    verification_path: String,
    expires_in_seconds: i64,
}

#[derive(Debug, Deserialize)]
struct SetupStatus {
    status: String,
    #[serde(default)]
    daemon_id: Option<String>,
    #[serde(default)]
    credential: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LocalState {
    server_url: String,
    daemon_id: String,
    credential: String,
    capabilities: Vec<String>,
}

#[derive(Default)]
struct DaemonCoordination {
    sessions: Vec<SessionReconcileState>,
}

impl DaemonCoordination {
    fn apply_reconciliation(&mut self, snapshot: ReconcileSnapshot) {
        self.sessions = snapshot.sessions;
    }

    fn apply_server_frame(&mut self, frame: ServerFrame) -> Result<(), CliError> {
        match frame {
            ServerFrame::Command(_) | ServerFrame::EventAck(_) => Ok(()),
            ServerFrame::Reconcile(_) => Err(CliError(
                "duplicate reconciliation reached active coordination".into(),
            )),
            ServerFrame::Welcome(_) => Err(CliError(
                "welcome reached active coordination unexpectedly".into(),
            )),
            ServerFrame::ProtocolError(error) => Err(CliError(error.message)),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match run(env::args().skip(1).collect()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("north-daemon: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Vec<String>) -> Result<(), CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };
    match command {
        "setup" => setup(&args[1..]).await,
        "start" => start(&args[1..]).await,
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => Err(CliError(format!("unknown command {other}; use `help`"))),
    }
}

async fn setup(args: &[String]) -> Result<(), CliError> {
    let server_url = required_option(args, "--server-url")?;
    let label = option(args, "--label").unwrap_or_else(|| "North daemon".into());
    let state_path = option(args, "--state-file")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_path);
    let base = server_url.trim_end_matches('/');
    let request: SetupCreated = curl_json(
        "POST",
        &format!("{base}/daemon/setup/request"),
        Some(&serde_json::json!({"label": label}).to_string()),
    )
    .map_err(CliError::from)?;
    println!(
        "Approve daemon setup in browser: {base}{}",
        request.verification_path
    );

    let expires = u64::try_from(request.expires_in_seconds).unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(expires);
    let status_url = format!("{base}/daemon/setup/{}", request.request_token);
    let claimed = poll_setup_status(
        deadline,
        || curl_json::<SetupStatus>("GET", &status_url, None),
        |duration| tokio::time::sleep(duration),
    )
    .await?;
    let daemon_id = claimed
        .daemon_id
        .ok_or_else(|| CliError("setup response omitted daemon_id".into()))?;
    let credential = claimed
        .credential
        .ok_or_else(|| CliError("setup response omitted credential".into()))?;
    write_state(
        &state_path,
        &LocalState {
            server_url: server_url.to_owned(),
            daemon_id: daemon_id.clone(),
            credential,
            capabilities: vec!["agent".into()],
        },
    )?;
    println!(
        "Daemon {daemon_id} credentials saved to {}",
        state_path.display()
    );
    Ok(())
}

async fn poll_setup_status<F, W, Fut>(
    deadline: Instant,
    mut poll: F,
    mut wait: W,
) -> Result<SetupStatus, CliError>
where
    F: FnMut() -> Result<SetupStatus, CurlError>,
    W: FnMut(Duration) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut retry_delay = Duration::from_secs(1);
    loop {
        if Instant::now() >= deadline {
            return Err(CliError("daemon setup request expired".into()));
        }
        match poll() {
            Ok(status) => {
                retry_delay = Duration::from_secs(1);
                if status.status == "claimed" {
                    return Ok(status);
                }
                wait(Duration::from_secs(2)).await;
            }
            Err(CurlError::Retryable(error)) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let sleep_for = retry_delay.min(remaining);
                if sleep_for.is_zero() {
                    return Err(CliError(
                        "daemon setup request expired while polling".into(),
                    ));
                }
                eprintln!("north-daemon: {error}; retrying");
                wait(sleep_for).await;
                retry_delay = retry_delay
                    .checked_mul(2)
                    .unwrap_or(Duration::from_secs(8))
                    .min(Duration::from_secs(8));
            }
            Err(CurlError::Terminal(error)) => {
                return Err(CliError(format!("poll daemon setup: {error}")));
            }
        }
    }
}

async fn start(args: &[String]) -> Result<(), CliError> {
    let state_path = option(args, "--state-file")
        .map(PathBuf::from)
        .unwrap_or_else(default_state_path);
    let state: LocalState = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .map_err(|error| CliError(format!("read {}: {error}", state_path.display())))?,
    )
    .map_err(|error| CliError(format!("parse {}: {error}", state_path.display())))?;
    let websocket_url = websocket_url(&state.server_url)?;
    let daemon_id = state.daemon_id.clone();
    let config = ConnectionConfig::new(
        websocket_url,
        Hello::new(daemon_id.clone(), state.credential, state.capabilities),
    );
    let (outbound, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let (events, mut events_receiver) = mpsc::channel(256);
    let heartbeat_sender = outbound.clone();
    let heartbeat_daemon_id = daemon_id;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if heartbeat_sender
                .send(DaemonFrame::Heartbeat(Heartbeat {
                    schema_version: north_protocol::SCHEMA_VERSION,
                    daemon_id: heartbeat_daemon_id.clone(),
                    sent_at: format!("{:?}", std::time::SystemTime::now()),
                    application_state: "connected".into(),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let supervisor = ConnectionSupervisor::new(config);
    let mut task = tokio::spawn(async move { supervisor.run(outbound_receiver, events).await });
    let _outbound = outbound;
    let mut coordination = DaemonCoordination::default();
    loop {
        tokio::select! {
            result = &mut task => {
                return result
                    .map_err(|error| CliError(format!("supervisor task failed: {error}")))?
                    .map_err(|error| CliError(error.to_string()));
            }
            event = events_receiver.recv() => match event {
                Some(ConnectionEvent::HandshakeComplete { result, ready }) => {
                    coordination.apply_reconciliation(result.reconciliation);
                    ready.send(()).map_err(|_| CliError("supervisor stopped during handshake".into()))?;
                }
                Some(ConnectionEvent::Frame(frame)) => {
                    coordination.apply_server_frame(frame)?;
                }
                None => return Err(CliError("supervisor event channel closed".into())),
            }
        }
    }
}

fn curl_json<T: DeserializeOwned>(
    method: &str,
    url: &str,
    body: Option<&str>,
) -> Result<T, CurlError> {
    let mut command = Command::new("curl");
    command
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            "15",
            "--write-out",
            "\n%{http_code}",
        ])
        .args(["--request", method, url]);
    if let Some(body) = body {
        command.args(["--header", "content-type: application/json", "--data", body]);
    }
    let output = command
        .output()
        .map_err(|error| CurlError::Terminal(format!("run curl: {error}")))?;
    let (body, http_status) = split_curl_response(&output.stdout);
    if !output.status.success() || !(200..300).contains(&http_status) {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if detail.is_empty() {
            format!("HTTP status {http_status}")
        } else {
            detail
        };
        return Err(classify_curl_failure(
            http_status,
            detail,
            output.status.code(),
        ));
    }
    serde_json::from_slice(body)
        .map_err(|error| CurlError::Terminal(format!("decode server response: {error}")))
}

fn classify_curl_failure(http_status: u16, detail: String, exit_code: Option<i32>) -> CurlError {
    let retryable_network_error =
        matches!(exit_code, Some(5 | 6 | 7 | 16 | 18 | 28 | 52 | 55 | 56));
    if http_status >= 500 || (http_status == 0 && retryable_network_error) {
        CurlError::Retryable(detail)
    } else {
        CurlError::Terminal(detail)
    }
}

fn split_curl_response(output: &[u8]) -> (&[u8], u16) {
    let Some(separator) = output.iter().rposition(|byte| *byte == b'\n') else {
        return (output, 0);
    };
    let status = std::str::from_utf8(&output[separator + 1..])
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    (&output[..separator], status)
}

fn write_state(path: &Path, state: &LocalState) -> Result<(), CliError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| CliError(format!("create {}: {error}", parent.display())))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| CliError(format!("encode daemon state: {error}")))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&temporary)
        .map_err(|error| CliError(format!("create {}: {error}", temporary.display())))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| CliError(format!("write {}: {error}", temporary.display())))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CliError(format!("install {}: {error}", path.display()))
    })
}

fn websocket_url(server_url: &str) -> Result<String, CliError> {
    let (scheme, rest) = if let Some(rest) = server_url.strip_prefix("https://") {
        ("wss", rest)
    } else if let Some(rest) = server_url.strip_prefix("http://") {
        ("ws", rest)
    } else if let Some(rest) = server_url.strip_prefix("wss://") {
        ("wss", rest)
    } else if let Some(rest) = server_url.strip_prefix("ws://") {
        ("ws", rest)
    } else {
        return Err(CliError("server URL must use http(s) or ws(s)".into()));
    };
    Ok(format!(
        "{scheme}://{}/daemon/ws",
        rest.trim_end_matches('/')
    ))
}

fn default_state_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".north/daemon.json")
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn required_option(args: &[String], name: &str) -> Result<String, CliError> {
    option(args, name).ok_or_else(|| CliError(format!("missing {name}")))
}

fn print_usage() {
    println!("north-daemon setup --server-url URL [--label LABEL] [--state-file PATH]");
    println!("north-daemon start [--state-file PATH]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordination_applies_reconciliation_before_readiness() {
        let mut coordination = DaemonCoordination::default();
        coordination.apply_reconciliation(ReconcileSnapshot {
            schema_version: north_protocol::SCHEMA_VERSION,
            sessions: vec![SessionReconcileState {
                session_id: "session-1".into(),
                command_ack_through_seq: 3,
                event_ack_through_seq: 2,
                event_ack_sparse: vec![4],
            }],
        });
        assert_eq!(coordination.sessions.len(), 1);
        assert_eq!(coordination.sessions[0].session_id, "session-1");
        assert_eq!(coordination.sessions[0].command_ack_through_seq, 3);
    }

    #[test]
    fn polling_failures_have_terminality() {
        assert!(matches!(
            classify_curl_failure(0, "connection refused".into(), Some(7)),
            CurlError::Retryable(_)
        ));
        assert!(matches!(
            classify_curl_failure(503, "service unavailable".into(), Some(22)),
            CurlError::Retryable(_)
        ));
        assert!(matches!(
            classify_curl_failure(400, "bad request".into(), Some(22)),
            CurlError::Terminal(_)
        ));
        assert!(matches!(
            classify_curl_failure(0, "malformed URL".into(), Some(3)),
            CurlError::Terminal(_)
        ));
        let (body, status) = split_curl_response(b"{\"status\":\"pending\"}\n200");
        assert_eq!(body, b"{\"status\":\"pending\"}");
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn polling_retries_connection_failure_until_claimed() {
        let mut polls = 0;
        let claimed = poll_setup_status(
            Instant::now() + Duration::from_secs(2),
            || {
                polls += 1;
                if polls == 1 {
                    Err(CurlError::Retryable("connection refused".into()))
                } else {
                    Ok(SetupStatus {
                        status: "claimed".into(),
                        daemon_id: Some("daemon-1".into()),
                        credential: Some("credential-1".into()),
                    })
                }
            },
            |_| async {},
        )
        .await
        .expect("retry then claim");
        assert_eq!(polls, 2);
        assert_eq!(claimed.status, "claimed");
    }

    #[tokio::test]
    async fn polling_stops_at_expiry_and_terminal_failure() {
        let mut polls = 0;
        let expired = poll_setup_status(
            Instant::now(),
            || {
                polls += 1;
                Ok(SetupStatus {
                    status: "pending".into(),
                    daemon_id: None,
                    credential: None,
                })
            },
            |_| async {},
        )
        .await;
        assert!(matches!(
            expired,
            Err(CliError(message)) if message == "daemon setup request expired"
        ));
        assert_eq!(polls, 0);

        let terminal = poll_setup_status(
            Instant::now() + Duration::from_secs(1),
            || Err(CurlError::Terminal("bad request".into())),
            |_| async {},
        )
        .await;
        assert!(matches!(
            terminal,
            Err(CliError(message)) if message == "poll daemon setup: bad request"
        ));
    }

    #[test]
    fn websocket_urls_preserve_server_authority() {
        assert_eq!(
            websocket_url("https://north.example/").expect("wss URL"),
            "wss://north.example/daemon/ws"
        );
        assert_eq!(
            websocket_url("http://127.0.0.1:8080").expect("ws URL"),
            "ws://127.0.0.1:8080/daemon/ws"
        );
        assert!(websocket_url("north.example").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = env::temp_dir().join(format!("north-daemon-state-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        write_state(
            &path,
            &LocalState {
                server_url: "http://localhost".into(),
                daemon_id: "daemon-1".into(),
                credential: "secret".into(),
                capabilities: vec!["agent".into()],
            },
        )
        .expect("write state");
        assert_eq!(
            fs::metadata(&path)
                .expect("state metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_file(path).expect("remove state");
    }
}
