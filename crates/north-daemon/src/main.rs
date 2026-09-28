use north_daemon::{
    coordination::{DaemonCoordinator, RuntimeActions},
    journal::{Journal, RuntimeExecutor},
    repository_inspection::RepositoryInspector,
    runtime::PiClarificationAdapter,
    scheduler::{RuntimeFollowup, RuntimeScheduler},
    status::{self, ConnectionState, DaemonStatus, ProcessState},
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
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[cfg(test)]
static START_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::mpsc,
};

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
        "--version" | "-V" => {
            println!("north-daemon {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
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
        .unwrap_or_else(status::default_state_path);
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
    std::io::stdout()
        .flush()
        .map_err(|_| CliError("write setup approval URL".into()))?;

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

struct SocketPathGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketPathGuard {
    fn new(path: PathBuf) -> Result<Self, CliError> {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| CliError("inspect daemon control socket".into()))?;
        if !metadata.file_type().is_socket() {
            return Err(CliError("daemon control path is not a socket".into()));
        }
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl Drop for SocketPathGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_socket()
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

struct StatusWriter {
    path: PathBuf,
    status: DaemonStatus,
}

impl StatusWriter {
    fn new(path: PathBuf, status: DaemonStatus) -> Result<Self, CliError> {
        let writer = Self { path, status };
        writer.persist()?;
        Ok(writer)
    }

    fn status(&self) -> &DaemonStatus {
        &self.status
    }

    fn persist(&self) -> Result<(), CliError> {
        status::write_status(&self.path, &self.status)
            .map_err(|_| CliError("write daemon status".into()))
    }

    fn update(
        &mut self,
        process_state: ProcessState,
        connection_state: ConnectionState,
        failure_class: Option<&str>,
    ) -> Result<(), CliError> {
        self.status.process_state = process_state;
        self.status.connection_state = connection_state;
        self.status.failure_class = failure_class.map(str::to_owned);
        self.persist()
    }
}

impl Drop for StatusWriter {
    fn drop(&mut self) {
        if !matches!(
            self.status.process_state,
            ProcessState::Stopped | ProcessState::Failed
        ) {
            self.status.process_state = ProcessState::Failed;
            self.status.connection_state = ConnectionState::Failed;
            self.status.failure_class = Some("daemon_exit".into());
            let _ = status::write_status(&self.path, &self.status);
        }
    }
}

fn daemon_instance_id() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp}", std::process::id())
}

async fn handle_control_request(
    mut stream: UnixStream,
    status_writer: &mut StatusWriter,
) -> Result<bool, CliError> {
    let mut request = String::new();
    let read = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::io::BufReader::new(&mut stream).read_line(&mut request),
    )
    .await
    .map_err(|_| CliError("daemon control request timed out".into()))?
    .map_err(|_| CliError("read daemon control request".into()))?;
    if read == 0 || request.len() > 256 {
        stream
            .write_all(b"error: invalid request\n")
            .await
            .map_err(|_| CliError("write daemon control response".into()))?;
        return Ok(false);
    }
    let request = request.trim_end_matches(['\r', '\n']);
    if request == "status" {
        status_writer.persist()?;
        let mut response = serde_json::to_vec(status_writer.status())
            .map_err(|_| CliError("encode daemon status".into()))?;
        response.push(b'\n');
        stream
            .write_all(&response)
            .await
            .map_err(|_| CliError("write daemon status".into()))?;
        return Ok(false);
    }
    if let Some(instance_id) = request.strip_prefix("stop ") {
        if instance_id == status_writer.status().instance_id {
            status_writer.update(ProcessState::Stopping, ConnectionState::Stopped, None)?;
            stream
                .write_all(b"stopping\n")
                .await
                .map_err(|_| CliError("write daemon stop response".into()))?;
            return Ok(true);
        }
        stream
            .write_all(b"error: instance mismatch\n")
            .await
            .map_err(|_| CliError("write daemon control response".into()))?;
        return Ok(false);
    }
    stream
        .write_all(b"error: unsupported request\n")
        .await
        .map_err(|_| CliError("write daemon control response".into()))?;
    Ok(false)
}

async fn start(args: &[String]) -> Result<(), CliError> {
    let state_path = option(args, "--state-file")
        .map(PathBuf::from)
        .unwrap_or_else(status::default_state_path);
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
    let pi_session_dir = default_daemon_directory(&state_path, "pi-sessions");
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
    let runtime = PiClarificationAdapter::new(repository_inspector, pi_session_dir)
        .map_err(|error| CliError(format!("initialize Pi clarification runtime: {error}")))?;
    let coordinator = DaemonCoordinator::new(journal, runtime);
    let (runtime_completion_sender, mut runtime_completion_receiver) = mpsc::unbounded_channel();
    let scheduler = RuntimeScheduler::new(coordinator.executor(), runtime_completion_sender);
    let recovered = coordinator
        .recover_for_scheduler()
        .map_err(|error| CliError(format!("recover daemon journal: {error}")))?;
    let control_path = status::control_socket_path(&state_path);
    let status_path = status::status_path(&state_path);
    let status_parent = control_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if state_path == status::default_state_path() {
        status::ensure_private_directory(status_parent)
            .map_err(|_| CliError("prepare daemon control directory".into()))?;
    } else {
        fs::create_dir_all(status_parent)
            .map_err(|error| CliError(format!("create daemon control directory: {error}")))?;
    }
    let control_listener = UnixListener::bind(&control_path).map_err(|error| {
        let message = match error.kind() {
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::AddrInUse => {
                "daemon control socket exists or is stale; refusing to replace it".into()
            }
            _ => format!("bind daemon control socket: {error}"),
        };
        CliError(message)
    })?;
    let _socket_guard = SocketPathGuard::new(control_path.clone())?;
    fs::set_permissions(&control_path, fs::Permissions::from_mode(0o600))
        .map_err(|_| CliError("protect daemon control socket".into()))?;
    let mut status_writer = StatusWriter::new(
        status_path,
        DaemonStatus {
            instance_id: daemon_instance_id(),
            pid: std::process::id(),
            server_url: state.server_url.clone(),
            daemon_id: state.daemon_id.clone(),
            process_state: ProcessState::Starting,
            connection_state: ConnectionState::Starting,
            failure_class: None,
        },
    )?;
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
    status_writer.update(ProcessState::Running, ConnectionState::Connecting, None)?;
    let mut shutdown = Box::pin(shutdown_signal());
    let mut pending_frames = recovered.frames;
    let mut pending_commands = recovered.commands;
    'event_loop: loop {
        tokio::select! {
            signal = &mut shutdown => {
                signal.map_err(|_| CliError("daemon shutdown signal handler failed".into()))?;
                break 'event_loop;
            }
            accepted = control_listener.accept() => {
                let (stream, _) = accepted
                    .map_err(|_| CliError("accept daemon control request".into()))?;
                if handle_control_request(stream, &mut status_writer).await? {
                    break 'event_loop;
                }
            }
            result = &mut task => {
                let _ = scheduler.request_shutdown();
                match result {
                    Ok(Ok(())) => {
                        status_writer.update(ProcessState::Stopped, ConnectionState::Stopped, None)?;
                        return Ok(());
                    }
                    Ok(Err(error)) => {
                        let failure_class = error.safe_failure_class();
                        if failure_class == "shutdown" {
                            status_writer.update(ProcessState::Stopped, ConnectionState::Stopped, None)?;
                            return Ok(());
                        }
                        status_writer.update(ProcessState::Failed, ConnectionState::Failed, Some(failure_class))?;
                        return Err(CliError(format!("daemon connection failed ({failure_class})")));
                    }
                    Err(_) => {
                        status_writer.update(ProcessState::Failed, ConnectionState::Failed, Some("supervisor"))?;
                        return Err(CliError("daemon supervisor failed".into()));
                    }
                }
            }
            completion = runtime_completion_receiver.recv() => {
                let Some(completion) = completion else {
                    let _ = scheduler.request_shutdown();
                    return Err(CliError("runtime completion channel closed".into()));
                };
                let finished = scheduler
                    .finish_active(&completion)
                    .map_err(|error| CliError(format!("finish scheduled runtime: {error}")))?;
                let actions = coordinator
                    .finish_runtime(
                        &completion.session_id,
                        &completion.command_id,
                        completion.outcome,
                        completion.events,
                    )
                    .map_err(|error| CliError(format!("finish runtime command: {error}")))?;
                emit_runtime_actions(&outbound, &scheduler, actions).await?;
                if let Some(followup) = finished.followup {
                    match followup {
                        RuntimeFollowup::FinishCancellation(command) => {
                            let actions = coordinator
                                .finish_runtime(
                                    &command.session_id,
                                    &command.command_id,
                                    north_daemon::DispatchOutcome::DispatchSucceeded,
                                    Vec::new(),
                                )
                                .map_err(|error| CliError(format!("finish cancellation: {error}")))?;
                            emit_runtime_actions(&outbound, &scheduler, actions).await?;
                            scheduler
                                .finish_followup(&command)
                                .map_err(|error| CliError(format!("cleanup cancellation runtime: {error}")))?;
                        }
                        RuntimeFollowup::RescheduleCancellation(command) => scheduler
                            .schedule_runtime(command)
                            .map_err(|error| CliError(format!("schedule cancellation: {error}")))?,
                    }
                }
            }
            event = events_receiver.recv() => match event {
                Some(ConnectionEvent::Reconnecting) => {
                    status_writer.update(ProcessState::Running, ConnectionState::Reconnecting, Some("transport"))?;
                }
                Some(ConnectionEvent::HandshakeComplete { result, ready }) => {
                    let mut actions = coordinator
                        .reconcile(result.reconciliation)
                        .map_err(|error| CliError(format!("reconcile daemon journal: {error}")))?;
                    actions.replay.append(&mut pending_frames);
                    ready.send(()).map_err(|_| CliError("supervisor stopped during handshake".into()))?;
                    status_writer.update(ProcessState::Running, ConnectionState::Connected, None)?;
                    for frame in actions.replay {
                        outbound
                            .send(frame)
                            .await
                            .map_err(|_| CliError("supervisor stopped during event replay".into()))?;
                    }
                    for command in pending_commands.drain(..) {
                        scheduler
                            .schedule(command)
                            .map_err(|error| CliError(format!("schedule recovered command: {error}")))?;
                    }
                }
                Some(ConnectionEvent::Frame(frame)) => {
                    let actions = match coordinator.accept_server_frame(frame) {
                        Ok(actions) => actions,
                        Err(north_daemon::coordination::CoordinationError::RetryableGap { .. }) => {
                            close_sender
                                .send(ConnectionControl::CloseRetryable)
                                .await
                                .map_err(|_| CliError("supervisor stopped at gap boundary".into()))?;
                            RuntimeActions::default()
                        }
                        Err(error) => {
                            let _ = scheduler.request_shutdown();
                            return Err(CliError(format!("process server frame: {error}")));
                        }
                    };
                    emit_runtime_actions(&outbound, &scheduler, actions).await?;
                }
                None => {
                    let _ = scheduler.request_shutdown();
                    return Err(CliError("supervisor event channel closed".into()));
                }
            }
        }
    }

    status_writer.update(ProcessState::Stopping, ConnectionState::Stopped, None)?;
    let _ = scheduler.request_shutdown();
    let _ = close_sender.send(ConnectionControl::Stop).await;
    match wait_for_supervisor_shutdown(&mut task, Duration::from_secs(10)).await {
        SupervisorShutdown::Complete => {}
        SupervisorShutdown::ConnectionFailed(failure_class) => {
            status_writer.update(
                ProcessState::Failed,
                ConnectionState::Failed,
                Some(&failure_class),
            )?;
            return Err(CliError(format!(
                "daemon shutdown failed ({failure_class})"
            )));
        }
        SupervisorShutdown::TaskFailed => {
            status_writer.update(
                ProcessState::Failed,
                ConnectionState::Failed,
                Some("supervisor"),
            )?;
            return Err(CliError("daemon supervisor failed during shutdown".into()));
        }
        SupervisorShutdown::TimedOut => {
            status_writer.update(
                ProcessState::Failed,
                ConnectionState::Failed,
                Some("shutdown_timeout"),
            )?;
            return Err(CliError("graceful daemon shutdown timed out".into()));
        }
    }
    status_writer.update(ProcessState::Stopped, ConnectionState::Stopped, None)?;
    Ok(())
}

enum SupervisorShutdown {
    Complete,
    ConnectionFailed(String),
    TaskFailed,
    TimedOut,
}

async fn wait_for_supervisor_shutdown(
    task: &mut tokio::task::JoinHandle<Result<(), north_daemon::transport::ConnectionError>>,
    timeout: Duration,
) -> SupervisorShutdown {
    match tokio::time::timeout(timeout, &mut *task).await {
        Ok(Ok(Ok(()))) => SupervisorShutdown::Complete,
        Ok(Ok(Err(error))) if error.safe_failure_class() == "shutdown" => {
            SupervisorShutdown::Complete
        }
        Ok(Ok(Err(error))) => {
            SupervisorShutdown::ConnectionFailed(error.safe_failure_class().to_owned())
        }
        Ok(Err(_)) => SupervisorShutdown::TaskFailed,
        Err(_) => {
            task.abort();
            let _ = task.await;
            SupervisorShutdown::TimedOut
        }
    }
}

async fn shutdown_signal() -> std::io::Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = ctrl_c => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    ctrl_c.await
}

async fn emit_runtime_actions<E: RuntimeExecutor + 'static>(
    outbound: &mpsc::Sender<DaemonFrame>,
    scheduler: &RuntimeScheduler<E>,
    actions: RuntimeActions,
) -> Result<(), CliError> {
    for frame in actions.frames {
        outbound
            .send(frame)
            .await
            .map_err(|_| CliError("supervisor stopped while sending runtime frame".into()))?;
    }
    for command in actions.commands {
        scheduler
            .schedule(command)
            .map_err(|error| CliError(format!("schedule runtime command: {error}")))?;
    }
    Ok(())
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
    async fn shutdown_timeout_aborts_supervisor_task() {
        let mut task = tokio::spawn(std::future::pending::<
            Result<(), north_daemon::transport::ConnectionError>,
        >());
        let result = wait_for_supervisor_shutdown(&mut task, Duration::ZERO).await;
        assert!(matches!(result, SupervisorShutdown::TimedOut));
        assert!(task.is_finished());
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
