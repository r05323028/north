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
    )?;
    println!(
        "Approve daemon setup in browser: {base}{}",
        request.verification_path
    );

    let expires = u64::try_from(request.expires_in_seconds).unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(expires);
    let status_url = format!("{base}/daemon/setup/{}", request.request_token);
    let claimed = loop {
        if Instant::now() >= deadline {
            return Err(CliError("daemon setup request expired".into()));
        }
        let status: SetupStatus = curl_json("GET", &status_url, None)?;
        if status.status == "claimed" {
            break status;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
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
) -> Result<T, CliError> {
    let mut command = Command::new("curl");
    command
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            "15",
        ])
        .args(["--request", method, url]);
    if let Some(body) = body {
        command.args(["--header", "content-type: application/json", "--data", body]);
    }
    let output = command
        .output()
        .map_err(|error| CliError(format!("run curl: {error}")))?;
    if !output.status.success() {
        return Err(CliError(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError(format!("decode server response: {error}")))
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
