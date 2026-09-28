use north_daemon::status::{self, ConnectionState, DaemonStatus, ProcessState};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};
use url::Url;

const APPROVAL_PREFIX: &str = "Approve daemon setup in browser: ";
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const START_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Deserialize)]
struct LocalDaemonState {
    server_url: String,
    daemon_id: String,
}

#[derive(Debug)]
struct Paths {
    state: PathBuf,
    status: PathBuf,
    socket: PathBuf,
    log: PathBuf,
}

impl Paths {
    fn from_state(state: PathBuf) -> Self {
        Self {
            status: status::status_path(&state),
            socket: status::control_socket_path(&state),
            log: status::log_path(&state),
            state,
        }
    }

    fn ensure_private_directory(&self) -> Result<(), String> {
        let parent = self
            .state
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        status::ensure_private_directory(parent)
            .map_err(|_| "cannot secure North state directory".to_owned())
    }
}

#[derive(Debug, Serialize)]
struct StatusOutput {
    process_state: &'static str,
    connection_state: &'static str,
    server_url: Option<String>,
    daemon_id: Option<String>,
    pid: Option<u32>,
    failure_class: Option<String>,
}

#[derive(Debug)]
enum ControlError {
    NotRunning,
    Unresponsive,
    Invalid,
}

// ponytail: stale lock fails closed; use advisory locking if automatic crash recovery is needed.
struct StartLock {
    path: PathBuf,
    device: u64,
    inode: u64,
    _file: fs::File,
}

impl StartLock {
    fn create(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        let metadata = file.metadata()?;
        Ok(Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
            _file: file,
        })
    }
}

impl Drop for StartLock {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn acquire_start_lock(paths: &Paths) -> Result<Option<StartLock>, String> {
    let lock_path = paths.state.with_file_name("daemon-start.lock");
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match StartLock::create(&lock_path) {
            Ok(lock) => return Ok(Some(lock)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Ok(status) = query_status(paths) {
                    println!("Daemon already running (pid {})", status.pid);
                    return Ok(None);
                }
                if Instant::now() >= deadline {
                    return Err(
                        "daemon startup lock is busy or stale; refusing to start a duplicate"
                            .into(),
                    );
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return Err("cannot create private daemon startup lock".into()),
        }
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("north: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => {
            print_usage();
            Ok(ExitCode::SUCCESS)
        }
        Some("--version") | Some("-V") => {
            println!("north {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("setup") => run_setup(&args[1..]),
        Some("daemon") => run_daemon(&args[1..]),
        Some(_) => Err("unknown command; use `north --help`".into()),
    }
}

fn run_setup(args: &[String]) -> Result<ExitCode, String> {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        println!("Usage: north setup --server-url HTTPS_URL [--label LABEL]");
        return Ok(ExitCode::SUCCESS);
    }
    let mut server_url = None;
    let mut label = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--server-url" => {
                index += 1;
                server_url = Some(args.get(index).ok_or("missing value for --server-url")?);
            }
            "--label" => {
                index += 1;
                label = Some(args.get(index).ok_or("missing value for --label")?);
            }
            _ => return Err("unknown setup option; use `north setup --help`".into()),
        }
        index += 1;
    }
    let raw_server_url = server_url.ok_or("north setup requires --server-url HTTPS_URL")?;
    let server_url = validate_server_url(raw_server_url)?;
    let paths = Paths::from_state(status::default_state_path());
    paths.ensure_private_directory()?;
    let daemon = sibling_daemon()?;
    run_setup_process(
        &daemon,
        &paths.state,
        server_url.as_str(),
        label.map(String::as_str),
    )?;
    start_daemon_with(&daemon, &paths, false)
}

fn run_setup_process(
    daemon: &Path,
    state_path: &Path,
    server_url: &str,
    label: Option<&str>,
) -> Result<(), String> {
    let mut command = Command::new(daemon);
    command
        .arg("setup")
        .args(["--server-url", server_url, "--state-file"])
        .arg(state_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(label) = label {
        command.args(["--label", label]);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "cannot start bundled north-daemon setup".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read daemon setup output".to_owned())?;

    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cannot read daemon setup output".into());
            }
        };
        if let Some(raw_url) = line.strip_prefix(APPROVAL_PREFIX) {
            let approval = match validate_approval_url(server_url, raw_url) {
                Ok(url) => url,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            if open_browser(&approval) {
                println!("Opened daemon approval in browser");
            } else {
                println!("Browser unavailable; open this approval URL: {approval}");
            }
        } else {
            println!("{line}");
        }
    }

    let result = child
        .wait()
        .map_err(|_| "cannot wait for daemon setup".to_owned())?;
    if result.success() {
        Ok(())
    } else {
        Err("daemon setup failed; no daemon was started".into())
    }
}

fn open_browser(url: &Url) -> bool {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return false;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    Command::new(opener)
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

fn run_daemon(args: &[String]) -> Result<ExitCode, String> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(ExitCode::SUCCESS);
    };
    match command {
        "start" => match &args[1..] {
            [] => start_daemon(false),
            [flag] if flag == "--foreground" => start_daemon(true),
            _ => Err("usage: north daemon start [--foreground]".into()),
        },
        "stop" if args.len() == 1 => stop_daemon(),
        "status" => {
            let json = match &args[1..] {
                [] => false,
                [flag, format] if flag == "--output" && format == "json" => true,
                _ => return Err("usage: north daemon status [--output json]".into()),
            };
            show_status(json)?;
            Ok(ExitCode::SUCCESS)
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(ExitCode::SUCCESS)
        }
        _ => Err("unknown daemon command; use `north daemon --help`".into()),
    }
}

fn start_daemon(foreground: bool) -> Result<ExitCode, String> {
    let paths = Paths::from_state(status::default_state_path());
    paths.ensure_private_directory()?;
    read_local_state(&paths.state)?;
    let daemon = sibling_daemon()?;
    start_daemon_with(&daemon, &paths, foreground)
}

fn start_daemon_with(daemon: &Path, paths: &Paths, foreground: bool) -> Result<ExitCode, String> {
    let Some(_start_lock) = acquire_start_lock(paths)? else {
        return Ok(ExitCode::SUCCESS);
    };
    if let Ok(status) = query_status(paths) {
        println!("Daemon already running (pid {})", status.pid);
        return Ok(ExitCode::SUCCESS);
    }
    if fs::symlink_metadata(&paths.socket).is_ok() {
        return Err(
            "daemon control socket is stale or unresponsive; refusing to replace it".into(),
        );
    }

    if foreground {
        let result = Command::new(daemon)
            .arg("start")
            .arg("--state-file")
            .arg(&paths.state)
            .status()
            .map_err(|_| "cannot start bundled north-daemon".to_owned())?;
        return Ok(ExitCode::from(result.code().unwrap_or(1) as u8));
    }

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&paths.log)
        .map_err(|_| "cannot open private daemon log".to_owned())?;
    fs::set_permissions(&paths.log, fs::Permissions::from_mode(0o600))
        .map_err(|_| "cannot protect daemon log".to_owned())?;
    let stderr = log
        .try_clone()
        .map_err(|_| "cannot open private daemon log".to_owned())?;
    let mut child = Command::new(daemon)
        .arg("start")
        .arg("--state-file")
        .arg(&paths.state)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|_| "cannot start bundled north-daemon".to_owned())?;

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(status) = query_status(paths) {
            if status.pid == child.id() {
                println!("Daemon started (pid {})", status.pid);
                return Ok(ExitCode::SUCCESS);
            }
            if child.try_wait().ok().flatten().is_some() {
                println!("Daemon already running (pid {})", status.pid);
                return Ok(ExitCode::SUCCESS);
            }
        }
        if child
            .try_wait()
            .map_err(|_| "cannot inspect daemon startup".to_owned())?
            .is_some()
        {
            if let Ok(status) = query_status(paths) {
                println!("Daemon already running (pid {})", status.pid);
                return Ok(ExitCode::SUCCESS);
            }
            return Err("daemon exited during startup; inspect ~/.north/daemon.log".into());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "daemon startup timed out; child stopped, inspect ~/.north/daemon.log".into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn stop_daemon() -> Result<ExitCode, String> {
    let paths = Paths::from_state(status::default_state_path());
    paths.ensure_private_directory()?;
    let running = match query_status(&paths) {
        Ok(status) => status,
        Err(ControlError::NotRunning) if fs::symlink_metadata(&paths.socket).is_err() => {
            println!("Daemon already stopped");
            return Ok(ExitCode::SUCCESS);
        }
        Err(_) => {
            return Err(
                "daemon control socket is stale or unresponsive; no PID signal was sent".into(),
            );
        }
    };

    let response = control_request(&paths.socket, &format!("stop {}", running.instance_id))
        .map_err(|_| "daemon control socket did not accept stop request".to_owned())?;
    if response != "stopping" {
        return Err("daemon rejected stop request; instance identity did not match".into());
    }

    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        match query_status(&paths) {
            Ok(status) if status.process_state == ProcessState::Stopped => {
                println!("Daemon stopped");
                return Ok(ExitCode::SUCCESS);
            }
            Err(ControlError::NotRunning)
                if status::read_status(&paths.status)
                    .ok()
                    .flatten()
                    .is_some_and(|status| status.process_state == ProcessState::Stopped) =>
            {
                println!("Daemon stopped");
                return Ok(ExitCode::SUCCESS);
            }
            _ => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(
        "graceful daemon stop timed out; daemon may still be running; no PID signal was sent"
            .into(),
    )
}

fn show_status(json: bool) -> Result<(), String> {
    let paths = Paths::from_state(status::default_state_path());
    paths.ensure_private_directory()?;
    let output = match query_status(&paths) {
        Ok(status) => status_output(&status),
        Err(_) => stale_status_output(&paths),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string(&output).map_err(|_| "cannot encode daemon status".to_owned())?
        );
    } else {
        println!("Process: {}", output.process_state);
        println!("Connection: {}", output.connection_state);
        if let Some(id) = output.daemon_id {
            println!("Daemon: {id}");
        }
        if let Some(url) = output.server_url {
            println!("Server: {url}");
        }
        if let Some(pid) = output.pid {
            println!("PID: {pid}");
        }
        if let Some(class) = output.failure_class {
            println!("Failure: {class}");
        }
    }
    Ok(())
}

fn stale_status_output(paths: &Paths) -> StatusOutput {
    stale_status_output_from(
        status::read_status(&paths.status).ok().flatten(),
        read_local_state(&paths.state).ok(),
        fs::symlink_metadata(&paths.socket).is_ok(),
    )
}

fn stale_status_output_from(
    persisted: Option<DaemonStatus>,
    local_state: Option<LocalDaemonState>,
    socket_exists: bool,
) -> StatusOutput {
    if let Some(status) = persisted {
        if !socket_exists
            && matches!(
                status.process_state,
                ProcessState::Stopped | ProcessState::Failed
            )
        {
            return status_output(&status);
        }
        let mut output = status_output(&status);
        output.process_state = "stale";
        output.connection_state = "stopped";
        output.failure_class = Some("control_unavailable".into());
        return output;
    }
    match local_state {
        Some(state) => StatusOutput {
            process_state: if socket_exists { "stale" } else { "stopped" },
            connection_state: "stopped",
            server_url: safe_server_url(&state.server_url),
            daemon_id: Some(state.daemon_id),
            pid: None,
            failure_class: socket_exists.then(|| "control_unavailable".into()),
        },
        None => StatusOutput {
            process_state: if socket_exists { "stale" } else { "stopped" },
            connection_state: "stopped",
            server_url: None,
            daemon_id: None,
            pid: None,
            failure_class: socket_exists.then(|| "control_unavailable".into()),
        },
    }
}

fn status_output(status: &DaemonStatus) -> StatusOutput {
    StatusOutput {
        process_state: process_state_name(status.process_state),
        connection_state: connection_state_name(status.connection_state),
        server_url: safe_server_url(&status.server_url),
        daemon_id: Some(status.daemon_id.clone()),
        pid: Some(status.pid),
        failure_class: status.failure_class.clone(),
    }
}

fn process_state_name(state: ProcessState) -> &'static str {
    match state {
        ProcessState::Starting => "starting",
        ProcessState::Running => "running",
        ProcessState::Stopping => "stopping",
        ProcessState::Stopped => "stopped",
        ProcessState::Failed => "failed",
        ProcessState::Stale => "stale",
    }
}

fn connection_state_name(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Starting => "starting",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
        ConnectionState::Reconnecting => "reconnecting",
        ConnectionState::Failed => "failed",
        ConnectionState::Stopped => "stopped",
    }
}

fn query_status(paths: &Paths) -> Result<DaemonStatus, ControlError> {
    let response = control_request(&paths.socket, "status")?;
    let live: DaemonStatus = serde_json::from_str(&response).map_err(|_| ControlError::Invalid)?;
    let saved = status::read_status(&paths.status)
        .map_err(|_| ControlError::Invalid)?
        .ok_or(ControlError::Invalid)?;
    if live.instance_id != saved.instance_id || live.pid != saved.pid {
        return Err(ControlError::Invalid);
    }
    Ok(live)
}

fn control_request(socket: &Path, request: &str) -> Result<String, ControlError> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ControlError::NotRunning
        } else {
            ControlError::Unresponsive
        }
    })?;
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|_| ControlError::Unresponsive)?;
    stream
        .set_write_timeout(Some(CONTROL_TIMEOUT))
        .map_err(|_| ControlError::Unresponsive)?;
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|_| ControlError::Unresponsive)?;
    let mut response = String::new();
    BufReader::new(stream)
        .take(8192)
        .read_line(&mut response)
        .map_err(|_| ControlError::Unresponsive)?;
    if response.is_empty() || response.len() >= 8192 {
        return Err(ControlError::Unresponsive);
    }
    Ok(response.trim_end_matches(['\r', '\n']).to_owned())
}

fn read_local_state(path: &Path) -> Result<LocalDaemonState, String> {
    let bytes = fs::read(path).map_err(|_| "daemon is not set up; run `north setup`".to_owned())?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "daemon state is invalid; inspect ~/.north/daemon.json".into())
}

fn sibling_daemon() -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|_| "cannot locate north executable".to_owned())?;
    let path = current
        .parent()
        .ok_or_else(|| "north executable has no parent directory".to_owned())?
        .join("north-daemon");
    if !path.is_file() {
        return Err("bundled north-daemon is missing beside north".into());
    }
    Ok(path)
}

fn validate_server_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "invalid server URL".to_owned())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("server URL must be HTTPS without credentials, query, or fragment".into());
    }
    Ok(url)
}

fn validate_approval_url(server_url: &str, raw: &str) -> Result<Url, String> {
    let server = validate_server_url(server_url)?;
    let approval =
        Url::parse(raw).map_err(|_| "server returned an invalid approval URL".to_owned())?;
    let segments: Vec<_> = approval
        .path_segments()
        .map(|segments| segments.collect())
        .unwrap_or_default();
    let token_index = segments.len().checked_sub(2);
    let valid_path = token_index.is_some_and(|index| {
        index >= 2
            && segments[index - 2] == "daemon"
            && segments[index - 1] == "setup"
            && !segments[index].is_empty()
            && segments[index]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            && segments.last() == Some(&"approve")
    });
    if approval.scheme() != "https"
        || approval.origin() != server.origin()
        || !approval.username().is_empty()
        || approval.password().is_some()
        || approval.query().is_some()
        || approval.fragment().is_some()
        || !valid_path
    {
        return Err("server returned a non-HTTPS or cross-origin approval URL".into());
    }
    Ok(approval)
}

fn safe_server_url(raw: &str) -> Option<String> {
    validate_server_url(raw)
        .ok()
        .map(|url| url.origin().ascii_serialization())
}

fn print_usage() {
    println!("North operator CLI\n\nUsage:\n  north setup --server-url HTTPS_URL [--label LABEL]\n  north daemon start [--foreground]\n  north daemon stop\n  north daemon status [--output json]\n  north --version");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_status() -> DaemonStatus {
        DaemonStatus {
            instance_id: "instance-1".into(),
            pid: 42,
            server_url: "https://north.example/private".into(),
            daemon_id: "daemon-1".into(),
            process_state: ProcessState::Running,
            connection_state: ConnectionState::Connected,
            failure_class: None,
        }
    }

    #[test]
    fn setup_urls_require_https_and_exact_origin() {
        assert!(validate_server_url("http://north.example").is_err());
        assert!(validate_server_url("https://user:pass@north.example").is_err());
        assert!(validate_approval_url(
            "https://north.example",
            "https://evil.example/daemon/setup/abc/approve"
        )
        .is_err());
        assert!(validate_approval_url(
            "https://north.example",
            "https://north.example/daemon/setup/abc/approve"
        )
        .is_ok());
    }

    #[test]
    fn status_output_maps_process_and_connection_states() {
        let mut status = sample_status();
        for (state, name) in [
            (ProcessState::Starting, "starting"),
            (ProcessState::Running, "running"),
            (ProcessState::Stopping, "stopping"),
            (ProcessState::Stopped, "stopped"),
            (ProcessState::Failed, "failed"),
            (ProcessState::Stale, "stale"),
        ] {
            status.process_state = state;
            assert_eq!(status_output(&status).process_state, name);
        }
        for (state, name) in [
            (ConnectionState::Starting, "starting"),
            (ConnectionState::Connecting, "connecting"),
            (ConnectionState::Connected, "connected"),
            (ConnectionState::Reconnecting, "reconnecting"),
            (ConnectionState::Failed, "failed"),
            (ConnectionState::Stopped, "stopped"),
        ] {
            status.connection_state = state;
            assert_eq!(status_output(&status).connection_state, name);
        }
    }

    #[test]
    fn status_output_never_reports_stale_connected_or_exposes_instance_id() {
        let output = stale_status_output_from(Some(sample_status()), None, false);
        assert_eq!(output.process_state, "stale");
        assert_eq!(output.connection_state, "stopped");
        assert!(serde_json::to_string(&output)
            .is_ok_and(|json| !json.contains("instance-1") && !json.contains("credential")));

        let socket_without_status = stale_status_output_from(
            None,
            Some(LocalDaemonState {
                server_url: "https://north.example".into(),
                daemon_id: "daemon-1".into(),
            }),
            true,
        );
        assert_eq!(socket_without_status.process_state, "stale");
        assert_eq!(socket_without_status.connection_state, "stopped");
    }
}
