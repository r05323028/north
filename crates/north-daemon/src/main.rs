use north_daemon::{
    coordination::DaemonCoordinator,
    journal::{DispatchOutcome, Journal, RecoveryOutcome, RuntimeExecutor},
    repository_inspection::RepositoryInspector,
    transport::{ConnectionConfig, ConnectionControl, ConnectionEvent, ConnectionSupervisor},
};
use north_protocol::{DaemonFrame, Heartbeat, Hello};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
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

#[cfg(test)]
static START_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
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

/// Placeholder until `introduce-agent-requirement-clarification` supplies
/// North's production agent runtime adapter. Durable coordination is real, but
/// executable commands currently produce a not-configured/unknown fact.
///
/// Repository inspection is initialized as a future adapter-injection seam; it
/// is intentionally not invoked by this change's production dispatch path.
struct LocalRuntime {
    /// Used by the downstream clarification runtime, not by `dispatch` here.
    _repository_inspection: RepositoryInspector,
}

impl RuntimeExecutor for LocalRuntime {
    fn dispatch(
        &self,
        _runtime_operation_id: &str,
        _command_id: &str,
        _command: &north_protocol::Command,
    ) -> DispatchOutcome {
        // Agent prompting/SDK execution belongs to the downstream runtime
        // change. Keep this binary honest rather than claiming completion.
        DispatchOutcome::Unknown("runtime_adapter_not_configured".into())
    }

    fn recover(
        &self,
        _runtime_operation_id: &str,
        _command_id: &str,
        _command: &north_protocol::Command,
    ) -> RecoveryOutcome {
        RecoveryOutcome::Unknown
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
    if !server_url.starts_with("https://") {
        return Err(CliError(
            "daemon setup requires an https:// server URL".into(),
        ));
    }
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
    let journal_path = option(args, "--journal-file")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_path.with_extension("journal.json"));
    let cache_root = option(args, "--repository-cache-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_daemon_directory(&state_path, "repository-cache"));
    let workspace_root = option(args, "--repository-workspace-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_daemon_directory(&state_path, "disposable-workspaces"));
    let repository_inspector = RepositoryInspector::new(cache_root, workspace_root)
        .map_err(|error| CliError(format!("initialize repository inspection: {error}")))?;
    for failure in repository_inspector.startup_cleanup().failures {
        eprintln!(
            "north-daemon: startup cleanup failed for {}: {}",
            failure.path.display(),
            failure.reason
        );
    }
    let journal = Journal::open(&journal_path, daemon_id.clone())
        .map_err(|error| CliError(format!("open {}: {error}", journal_path.display())))?;
    let coordinator = DaemonCoordinator::new(
        journal,
        LocalRuntime {
            _repository_inspection: repository_inspector,
        },
    );
    let recovered = coordinator
        .recover()
        .map_err(|error| CliError(format!("recover daemon journal: {error}")))?;
    let (outbound, outbound_receiver) = ConnectionSupervisor::outbound_channel();
    let (close_sender, close_receiver) = ConnectionSupervisor::control_channel();
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
    let mut task = tokio::spawn(async move {
        supervisor
            .run_with_control(outbound_receiver, events, close_receiver)
            .await
    });
    let mut pending_frames = recovered;
    loop {
        tokio::select! {
            result = &mut task => {
                return result
                    .map_err(|error| CliError(format!("supervisor task failed: {error}")))?
                    .map_err(|error| CliError(error.to_string()));
            }
            event = events_receiver.recv() => match event {
                Some(ConnectionEvent::HandshakeComplete { result, ready }) => {
                    let mut frames = coordinator
                        .reconcile(result.reconciliation)
                        .map_err(|error| CliError(format!("reconcile daemon journal: {error}")))?
                        .replay;
                    frames.append(&mut pending_frames);
                    ready.send(()).map_err(|_| CliError("supervisor stopped during handshake".into()))?;
                    for frame in frames {
                        outbound
                            .send(frame)
                            .await
                            .map_err(|_| CliError("supervisor stopped during event replay".into()))?;
                    }
                }
                Some(ConnectionEvent::Frame(frame)) => {
                    let responses = match coordinator.process_server_frame(frame) {
                        Ok(responses) => responses,
                        Err(north_daemon::coordination::CoordinationError::RetryableGap { .. }) => {
                            close_sender
                                .send(ConnectionControl::CloseRetryable)
                                .await
                                .map_err(|_| CliError("supervisor stopped at gap boundary".into()))?;
                            Vec::new()
                        }
                        Err(error) => {
                            return Err(CliError(format!("process server frame: {error}")));
                        }
                    };
                    for response in responses {
                        outbound
                            .send(response)
                            .await
                            .map_err(|_| CliError("supervisor stopped while sending response".into()))?;
                    }
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
    })?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CliError(format!("sync {}: {error}", parent.display())))?;
    Ok(())
}

fn websocket_url(server_url: &str) -> Result<String, CliError> {
    let (scheme, rest) = if let Some(rest) = server_url.strip_prefix("https://") {
        ("wss", rest)
    } else if let Some(rest) = server_url.strip_prefix("wss://") {
        ("wss", rest)
    } else {
        return Err(CliError("server URL must use https:// or wss://".into()));
    };
    Ok(format!(
        "{scheme}://{}/daemon/ws",
        rest.trim_end_matches('/')
    ))
}

fn default_daemon_directory(state_path: &Path, name: &str) -> PathBuf {
    state_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(name)
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

const SETUP_USAGE: &str =
    "north-daemon setup --server-url HTTPS_URL [--label LABEL] [--state-file PATH]";
const START_USAGE: &str = "north-daemon start [--state-file PATH] [--journal-file PATH] [--repository-cache-dir PATH] [--repository-workspace-dir PATH]";

fn print_usage() {
    println!("{SETUP_USAGE}");
    println!("{START_USAGE}");
}

#[cfg(test)]
mod tests {
    use super::*;

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
            websocket_url("wss://127.0.0.1:8080").expect("wss URL"),
            "wss://127.0.0.1:8080/daemon/ws"
        );
        assert!(websocket_url("http://127.0.0.1:8080").is_err());
        assert!(websocket_url("north.example").is_err());
    }

    #[test]
    fn default_directories_follow_state_file_parent() {
        assert_eq!(
            default_daemon_directory(Path::new("/tmp/north/state.json"), "cache"),
            PathBuf::from("/tmp/north/cache")
        );
        assert_eq!(
            default_daemon_directory(Path::new("state.json"), "cache"),
            PathBuf::from("./cache")
        );
    }

    #[test]
    fn usage_lists_repository_directory_options() {
        assert!(START_USAGE.contains("--repository-cache-dir"));
        assert!(START_USAGE.contains("--repository-workspace-dir"));
        print_usage();
    }

    #[tokio::test]
    async fn start_initializes_repository_roots_before_opening_journal() {
        let root = env::temp_dir().join(format!(
            "north-daemon-start-{}-{}",
            std::process::id(),
            START_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("create startup test root");
        let state_path = root.join("state.json");
        write_state(
            &state_path,
            &LocalState {
                server_url: "https://example.test".into(),
                daemon_id: "daemon-1".into(),
                credential: "secret".into(),
                capabilities: vec!["agent".into()],
            },
        )
        .expect("write startup state");
        let journal_path = root.join("journal-directory");
        fs::create_dir_all(&journal_path).expect("create invalid journal path");
        let explicit_cache = root.join("explicit-cache");
        let explicit_workspace = root.join("explicit-workspace");
        let unsafe_staging = explicit_cache.join("id-7265706f/.source-unsafe");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let namespace = explicit_cache.join("id-7265706f");
            fs::create_dir_all(&namespace).expect("create cleanup namespace");
            let outside = root.join("outside");
            fs::create_dir_all(&outside).expect("create cleanup target");
            let outside_marker = outside.join("must-survive");
            fs::write(&outside_marker, "outside").expect("create cleanup marker");
            symlink(&outside, &unsafe_staging).expect("create cleanup symlink");
        }

        let explicit_args = vec![
            "start".into(),
            "--state-file".into(),
            state_path.to_string_lossy().into_owned(),
            "--journal-file".into(),
            journal_path.to_string_lossy().into_owned(),
            "--repository-cache-dir".into(),
            explicit_cache.to_string_lossy().into_owned(),
            "--repository-workspace-dir".into(),
            explicit_workspace.to_string_lossy().into_owned(),
        ];
        let explicit_error = start(&explicit_args)
            .await
            .expect_err("journal path is a directory");
        assert!(matches!(explicit_error, CliError(message) if message.starts_with("open ")));
        assert!(explicit_cache.is_dir());
        assert!(explicit_workspace.is_dir());
        #[cfg(unix)]
        {
            assert!(unsafe_staging.is_symlink());
            assert!(root.join("outside/must-survive").is_file());
        }

        let default_args = vec![
            "start".into(),
            "--state-file".into(),
            state_path.to_string_lossy().into_owned(),
            "--journal-file".into(),
            journal_path.to_string_lossy().into_owned(),
        ];
        let default_error = start(&default_args)
            .await
            .expect_err("journal path is a directory");
        assert!(matches!(default_error, CliError(message) if message.starts_with("open ")));
        assert!(root.join("repository-cache").is_dir());
        assert!(root.join("disposable-workspaces").is_dir());

        fs::remove_dir_all(root).expect("remove startup test root");
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
