use north_daemon::{
    InspectionCancellation, InspectionPhase, InspectionRequest, RepositoryInspector,
    RepositorySource, RunAuthorization,
};
use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct GitFixture {
    remote: PathBuf,
    working: PathBuf,
    first_commit: String,
}

fn test_directory() -> TestDirectory {
    let path = std::env::temp_dir().join(format!(
        "north-repository-inspection-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| panic!("clock"))
            .as_nanos(),
        TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap_or_else(|_| panic!("test directory"));
    TestDirectory(path)
}

fn git<I, S>(directory: Option<&Path>, arguments: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect::<Vec<_>>();
    let mut command = Command::new("git");
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command
        .args(&arguments)
        .output()
        .unwrap_or_else(|_| panic!("run git fixture command"));
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn fixture(root: &Path) -> GitFixture {
    fixture_with_object_format(root, None)
}

fn sha256_fixture(root: &Path) -> GitFixture {
    fixture_with_object_format(root, Some("sha256"))
}

fn fixture_with_object_format(root: &Path, object_format: Option<&str>) -> GitFixture {
    fs::create_dir_all(root).unwrap_or_else(|_| panic!("fixture root"));
    let remote = root.join("remote.git");
    let working = root.join("working");
    let mut remote_init = vec!["init".to_owned(), "--bare".to_owned()];
    let mut working_init = vec!["init".to_owned()];
    if let Some(object_format) = object_format {
        let option = format!("--object-format={object_format}");
        remote_init.push(option.clone());
        working_init.push(option);
    }
    remote_init.push(
        remote
            .to_str()
            .unwrap_or_else(|| panic!("remote path"))
            .to_owned(),
    );
    working_init.push(
        working
            .to_str()
            .unwrap_or_else(|| panic!("working path"))
            .to_owned(),
    );
    git(None, remote_init);
    git(None, working_init);
    git(
        Some(&working),
        ["config", "user.email", "test@example.invalid"],
    );
    git(Some(&working), ["config", "user.name", "North test"]);
    fs::write(working.join("README.md"), "first\n").unwrap_or_else(|_| panic!("fixture file"));
    fs::write(working.join(".gitignore"), "*.north-ignored\n")
        .unwrap_or_else(|_| panic!("ignore file"));
    git(Some(&working), ["add", "README.md", ".gitignore"]);
    git(Some(&working), ["commit", "-m", "first"]);
    git(Some(&working), ["branch", "-M", "main"]);
    git(
        Some(&working),
        [
            "remote",
            "add",
            "origin",
            remote.to_str().unwrap_or_else(|| panic!("remote path")),
        ],
    );
    git(Some(&working), ["push", "-u", "origin", "main"]);
    git(Some(&remote), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let first_commit = git(Some(&working), ["rev-parse", "HEAD"]).trim().to_owned();
    GitFixture {
        remote,
        working,
        first_commit,
    }
}

fn inspector(root: &Path) -> RepositoryInspector {
    RepositoryInspector::new(root.join("cache"), root.join("workspaces"))
        .unwrap_or_else(|_| panic!("inspector"))
}

#[cfg(unix)]
fn git_http_server(root: PathBuf, stop: Arc<AtomicBool>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .unwrap_or_else(|_| panic!("credential test HTTP server"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|_| panic!("credential test HTTP address"))
        .port();
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|_| panic!("nonblocking credential test HTTP server"));
    let handle = thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if stream.set_nonblocking(false).is_ok() {
                        serve_git_http_request(&mut stream, &root);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://127.0.0.1:{port}/remote.git"), handle)
}

#[cfg(unix)]
fn serve_git_http_request(stream: &mut TcpStream, root: &Path) {
    let Some((method, target, headers, body)) = read_http_request(stream) else {
        return;
    };
    let authorized = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
    if !authorized {
        let _ = stream.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"north-test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut command = Command::new("git");
    command
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", method)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("REMOTE_USER", "north-test")
        .env("CONTENT_LENGTH", body.len().to_string());
    if let Some((_, value)) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        command.env("CONTENT_TYPE", value);
    }
    if let Some((_, value)) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("git-protocol"))
    {
        command.env("HTTP_GIT_PROTOCOL", value);
    }
    let mut child = match command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&body);
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(_) => return,
    };
    write_cgi_response(stream, &output.stdout);
}

#[cfg(unix)]
type HttpRequest = (String, String, Vec<(String, String)>, Vec<u8>);

#[cfg(unix)]
fn read_http_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let header_end = loop {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
    };
    let header_text = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = header_text.split("\r\n");
    let mut request = lines.next()?.split_whitespace();
    let method = request.next()?.to_owned();
    let target = request.next()?.to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
        .collect::<Vec<_>>();
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
    }
    Some((
        method,
        target,
        headers,
        raw[body_start..body_start + content_length].to_vec(),
    ))
}

#[cfg(unix)]
fn write_cgi_response(stream: &mut TcpStream, output: &[u8]) {
    let Some(header_end) = output.windows(4).position(|window| window == b"\r\n\r\n") else {
        return;
    };
    let header_text = String::from_utf8_lossy(&output[..header_end]);
    let mut status = "200 OK";
    let mut headers = String::new();
    for line in header_text.split("\r\n") {
        if let Some(value) = line.strip_prefix("Status: ") {
            status = value;
        } else if !line.is_empty() {
            headers.push_str(line);
            headers.push_str("\r\n");
        }
    }
    let response = format!("HTTP/1.1 {status}\r\n{headers}\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&output[header_end + 4..]);
}

#[cfg(unix)]
fn run_credential_helper_child() {
    let root = PathBuf::from(
        std::env::var_os("NORTH_CREDENTIAL_HELPER_ROOT")
            .unwrap_or_else(|| panic!("credential helper root")),
    );
    let log = PathBuf::from(
        std::env::var_os("NORTH_CREDENTIAL_HELPER_LOG")
            .unwrap_or_else(|| panic!("credential helper log")),
    );
    let fixture = fixture(&root.join("source"));
    let stop = Arc::new(AtomicBool::new(false));
    let (url, server) = git_http_server(root.join("source"), Arc::clone(&stop));
    let inspector = inspector(&root);
    let source = RepositorySource::new("repo-1", url);
    let request = InspectionRequest::new("session-1", "task-1", source.clone());
    let authorization = RunAuthorization::new("session-1", vec![source])
        .unwrap_or_else(|_| panic!("credential helper authorization"));
    let prepared = inspector
        .prepare(&request, &authorization)
        .unwrap_or_else(|error| panic!("credentialed host-Git preparation: {error}"));
    assert_eq!(prepared.commit_sha(), fixture.first_commit);
    inspector
        .dispose(prepared)
        .unwrap_or_else(|_| panic!("dispose credentialed checkout"));
    stop.store(true, Ordering::Relaxed);
    server
        .join()
        .unwrap_or_else(|_| panic!("credential test HTTP server thread"));
    let invocations = fs::read_to_string(log).unwrap_or_else(|_| panic!("credential helper log"));
    assert!(
        invocations.lines().any(|line| line == "get"),
        "Git did not invoke configured credential helper: {invocations:?}"
    );
}

#[cfg(unix)]
fn run_failed_clone_child() {
    let root = PathBuf::from(
        std::env::var_os("NORTH_FAILED_CLONE_ROOT").unwrap_or_else(|| panic!("failed clone root")),
    );
    let wrapper = PathBuf::from(
        std::env::var_os("NORTH_FAILED_CLONE_WRAPPER")
            .unwrap_or_else(|| panic!("failed clone wrapper")),
    );
    let log = PathBuf::from(
        std::env::var_os("NORTH_FAILED_CLONE_LOG").unwrap_or_else(|| panic!("failed clone log")),
    );
    let fixture = fixture(&root.join("source"));
    let inspector = inspector(&root);
    let source = RepositorySource::new("repo-1", fixture.remote.to_string_lossy());
    let request = InspectionRequest::new("session-1", "task-1", source.clone());
    let authorization = RunAuthorization::new("session-1", vec![source])
        .unwrap_or_else(|_| panic!("failed clone authorization"));
    let error = inspector
        .prepare(&request, &authorization)
        .expect_err("forced failed mirror clone");
    assert_eq!(error.phase, InspectionPhase::Cache);
    assert!(
        !error.cleanup_failed(),
        "safe staging cleanup failed: {error:?}"
    );
    let namespace = inspector.repository_cache_path("repo-1");
    let staging_remains = fs::read_dir(&namespace)
        .unwrap_or_else(|_| panic!("cache namespace"))
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".source-"))
        });
    assert!(!staging_remains, "failed clone left staging material");
    let invocation = fs::read_to_string(log).unwrap_or_else(|_| panic!("failed clone log"));
    assert!(invocation.lines().any(|line| line == "clone"));
    assert!(invocation.lines().any(|line| line == "--mirror"));
    assert!(invocation.lines().any(|line| line.contains(".source-")));
    assert!(wrapper.exists());
}

#[cfg(unix)]
fn run_ssh_command_child() {
    let root = PathBuf::from(
        std::env::var_os("NORTH_SSH_COMMAND_ROOT").unwrap_or_else(|| panic!("SSH command root")),
    );
    let log = PathBuf::from(
        std::env::var_os("NORTH_SSH_COMMAND_LOG").unwrap_or_else(|| panic!("SSH command log")),
    );
    let inspector = inspector(&root);
    let source = RepositorySource::new("repo-1", "ssh://git@127.0.0.1:22/remote.git");
    let request = InspectionRequest::new("session-1", "task-1", source.clone());
    let authorization = RunAuthorization::new("session-1", vec![source])
        .unwrap_or_else(|_| panic!("SSH command authorization"));
    let error = inspector
        .prepare(&request, &authorization)
        .expect_err("fake SSH command must reject fetch");
    assert_eq!(error.phase, InspectionPhase::Cache);
    let invocations = fs::read_to_string(log).unwrap_or_else(|_| panic!("SSH command log"));
    assert!(
        invocations
            .lines()
            .any(|line| line.contains("git-upload-pack")),
        "Git did not invoke configured core.sshCommand: {invocations:?}"
    );
}

fn request(repository_id: &str, task_id: &str, remote: &Path) -> InspectionRequest {
    InspectionRequest::new(
        "session-1",
        task_id,
        RepositorySource::new(
            repository_id,
            remote.to_str().unwrap_or_else(|| panic!("remote path")),
        ),
    )
}

fn authorization(repository_id: &str, remote: &Path) -> RunAuthorization {
    RunAuthorization::new(
        "session-1",
        vec![RepositorySource::new(
            repository_id,
            remote.to_str().unwrap_or_else(|| panic!("remote path")),
        )],
    )
    .unwrap_or_else(|_| panic!("authorization"))
}

#[cfg(unix)]
#[test]
fn failed_mirror_clone_cleans_owned_staging() {
    if std::env::var("NORTH_FAILED_CLONE_CHILD").is_ok_and(|value| value == "1") {
        run_failed_clone_child();
        return;
    }

    let directory = test_directory();
    let wrapper_dir = directory.0.join("git-wrapper-bin");
    fs::create_dir_all(&wrapper_dir).unwrap_or_else(|_| panic!("failed clone wrapper directory"));
    let wrapper = wrapper_dir.join("git");
    let log = directory.0.join("failed-clone.log");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
if [ "$1" = clone ] && [ "$2" = --mirror ]; then
  "$NORTH_REAL_GIT" "$@"
  status=$?
  printf '%s\n' "$@" > "$NORTH_FAILED_CLONE_LOG"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
  exit 1
fi
exec "$NORTH_REAL_GIT" "$@"
"#,
    )
    .unwrap_or_else(|_| panic!("failed clone wrapper"));
    let mut permissions = fs::metadata(&wrapper)
        .unwrap_or_else(|_| panic!("failed clone wrapper metadata"))
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    fs::set_permissions(&wrapper, permissions)
        .unwrap_or_else(|_| panic!("failed clone wrapper permissions"));
    let real_git = Command::new("which")
        .arg("git")
        .output()
        .unwrap_or_else(|_| panic!("locate Git"));
    assert!(real_git.status.success(), "locate Git failed");
    let real_git = String::from_utf8(real_git.stdout)
        .unwrap_or_else(|_| panic!("Git path"))
        .trim()
        .to_owned();
    let mut path_entries = vec![wrapper
        .parent()
        .unwrap_or_else(|| panic!("wrapper directory"))
        .to_owned()];
    path_entries.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let child_path = std::env::join_paths(path_entries).unwrap_or_else(|_| panic!("child PATH"));
    let output = Command::new(
        std::env::current_exe().unwrap_or_else(|_| panic!("failed clone test binary")),
    )
    .args([
        "--exact",
        "failed_mirror_clone_cleans_owned_staging",
        "--nocapture",
    ])
    .env("NORTH_FAILED_CLONE_CHILD", "1")
    .env("NORTH_FAILED_CLONE_ROOT", &directory.0)
    .env("NORTH_FAILED_CLONE_WRAPPER", &wrapper)
    .env("NORTH_FAILED_CLONE_LOG", &log)
    .env("NORTH_REAL_GIT", real_git)
    .env("PATH", child_path)
    .env("GIT_TERMINAL_PROMPT", "0")
    .output()
    .unwrap_or_else(|_| panic!("run failed clone child"));
    assert!(
        output.status.success(),
        "failed clone child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn configured_credential_helper_is_used_for_host_git_fetch() {
    if std::env::var("NORTH_CREDENTIAL_HELPER_CHILD").is_ok_and(|value| value == "1") {
        run_credential_helper_child();
        return;
    }

    let directory = test_directory();
    let home = directory.0.join("home");
    let helper = directory.0.join("credential-helper.sh");
    let log = directory.0.join("credential-helper.log");
    fs::create_dir_all(&home).unwrap_or_else(|_| panic!("credential helper home"));
    fs::write(
        &helper,
        r#"#!/bin/sh
printf '%s\n' "$1" >> "$NORTH_CREDENTIAL_HELPER_LOG"
if [ "$1" = get ]; then
  printf 'username=north-test\npassword=north-test\n'
fi
"#,
    )
    .unwrap_or_else(|_| panic!("credential helper script"));
    let mut permissions = fs::metadata(&helper)
        .unwrap_or_else(|_| panic!("credential helper metadata"))
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    fs::set_permissions(&helper, permissions)
        .unwrap_or_else(|_| panic!("credential helper permissions"));
    fs::write(
        home.join(".gitconfig"),
        format!("[credential]\n\thelper = {}\n", helper.display()),
    )
    .unwrap_or_else(|_| panic!("credential helper Git config"));

    let output = Command::new(
        std::env::current_exe().unwrap_or_else(|_| panic!("credential helper test binary")),
    )
    .args([
        "--exact",
        "configured_credential_helper_is_used_for_host_git_fetch",
        "--nocapture",
    ])
    .env("NORTH_CREDENTIAL_HELPER_CHILD", "1")
    .env("NORTH_CREDENTIAL_HELPER_ROOT", &directory.0)
    .env("NORTH_CREDENTIAL_HELPER_LOG", &log)
    .env("HOME", &home)
    .env("GIT_TERMINAL_PROMPT", "0")
    .output()
    .unwrap_or_else(|_| panic!("run credential helper child"));
    assert!(
        output.status.success(),
        "credential helper child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn configured_core_ssh_command_is_used_for_host_git_fetch() {
    if std::env::var("NORTH_SSH_COMMAND_CHILD").is_ok_and(|value| value == "1") {
        run_ssh_command_child();
        return;
    }

    let directory = test_directory();
    let home = directory.0.join("home");
    let command = directory.0.join("ssh-command.sh");
    let log = directory.0.join("ssh-command.log");
    fs::create_dir_all(&home).unwrap_or_else(|_| panic!("SSH command home"));
    fs::write(
        &command,
        r#"#!/bin/sh
printf '%s\n' "$@" >> "$NORTH_SSH_COMMAND_LOG"
exit 1
"#,
    )
    .unwrap_or_else(|_| panic!("SSH command script"));
    let mut permissions = fs::metadata(&command)
        .unwrap_or_else(|_| panic!("SSH command metadata"))
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    fs::set_permissions(&command, permissions)
        .unwrap_or_else(|_| panic!("SSH command permissions"));
    fs::write(
        home.join(".gitconfig"),
        format!(
            "[core]\n\tsshCommand = {}\n[ssh]\n\tvariant = ssh\n",
            command.display()
        ),
    )
    .unwrap_or_else(|_| panic!("SSH command Git config"));

    let output =
        Command::new(std::env::current_exe().unwrap_or_else(|_| panic!("SSH command test binary")))
            .args([
                "--exact",
                "configured_core_ssh_command_is_used_for_host_git_fetch",
                "--nocapture",
            ])
            .env("NORTH_SSH_COMMAND_CHILD", "1")
            .env("NORTH_SSH_COMMAND_ROOT", &directory.0)
            .env("NORTH_SSH_COMMAND_LOG", &log)
            .env("HOME", &home)
            .env_remove("GIT_SSH_COMMAND")
            .env_remove("GIT_SSH")
            .env_remove("GIT_SSH_VARIANT")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap_or_else(|_| panic!("run SSH command child"));
    assert!(
        output.status.success(),
        "SSH command child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pins_detached_full_sha_and_disposes_workspace() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let seen_path = Arc::new(Mutex::new(None));
    let seen_path_in_runtime = Arc::clone(&seen_path);
    let request = request("repo-1", "task-1", &fixture.remote);

    let result = inspector
        .inspect(
            &request,
            &authorization("repo-1", &fixture.remote),
            |workspace| {
                *seen_path_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("path lock")) = Some(workspace.to_owned());
                assert!(workspace.starts_with(inspector.workspace_root()));
                assert!(!workspace.starts_with(inspector.cache_root()));
                assert_eq!(
                    git(Some(workspace), ["rev-parse", "--verify", "HEAD^{commit}"]).trim(),
                    fixture.first_commit
                );
                assert_eq!(
                    git(Some(workspace), ["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
                    "HEAD"
                );
                Ok(())
            },
        )
        .unwrap_or_else(|_| panic!("inspection"));

    assert_eq!(result.repository_id, "repo-1");
    assert_eq!(result.commit_sha, fixture.first_commit);
    assert_eq!(result.commit_sha.len(), fixture.first_commit.len());
    let evidence = result.reviewed_repository();
    assert_eq!(evidence.repository_id, "repo-1");
    assert_eq!(evidence.commit_sha, fixture.first_commit);
    let workspace = seen_path
        .lock()
        .unwrap_or_else(|_| panic!("path lock"))
        .clone()
        .unwrap_or_else(|| panic!("runtime workspace"));
    assert!(!workspace.exists());
    assert!(inspector
        .repository_cache_path("repo-1")
        .join("source.git")
        .is_dir());
}

#[test]
fn sha256_object_format_is_pinned_without_fixed_width_assumption() {
    let directory = test_directory();
    let fixture = sha256_fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let result = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            |_| Ok(()),
        )
        .unwrap_or_else(|_| panic!("SHA-256 inspection"));
    assert_eq!(result.commit_sha, fixture.first_commit);
    assert_eq!(result.commit_sha.len(), 64);
}

#[test]
fn moving_remote_does_not_change_prepared_revision() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .unwrap_or_else(|_| panic!("prepare checkout"));

    fs::write(fixture.working.join("README.md"), "second\n")
        .unwrap_or_else(|_| panic!("second file"));
    git(Some(&fixture.working), ["add", "README.md"]);
    git(Some(&fixture.working), ["commit", "-m", "second"]);
    git(Some(&fixture.working), ["push", "origin", "main"]);

    assert_eq!(prepared.commit_sha(), fixture.first_commit);
    assert_eq!(
        git(
            Some(prepared.path()),
            ["rev-parse", "--verify", "HEAD^{commit}"]
        )
        .trim(),
        fixture.first_commit
    );
    inspector
        .dispose(prepared)
        .unwrap_or_else(|_| panic!("dispose checkout"));
}

#[test]
fn runtime_revision_change_is_rejected_before_success() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    fs::write(fixture.working.join("README.md"), "second\n")
        .unwrap_or_else(|_| panic!("second file"));
    git(Some(&fixture.working), ["add", "README.md"]);
    git(Some(&fixture.working), ["commit", "-m", "second"]);
    git(Some(&fixture.working), ["push", "origin", "main"]);

    let inspector = inspector(&directory.0);
    let error = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            |workspace| {
                git(
                    Some(workspace),
                    ["checkout", "--detach", fixture.first_commit.as_str()],
                );
                Ok(())
            },
        )
        .expect_err("runtime revision change must fail");
    assert_eq!(error.phase, InspectionPhase::Revision);
    assert!(error.reason.contains("revision changed"));
}

#[test]
fn cache_fetches_new_revision_for_next_run() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let request = request("repo-1", "task-1", &fixture.remote);
    inspector
        .inspect(&request, &authorization("repo-1", &fixture.remote), |_| {
            Ok(())
        })
        .unwrap_or_else(|_| panic!("first inspection"));

    fs::write(fixture.working.join("README.md"), "second\n")
        .unwrap_or_else(|_| panic!("second file"));
    git(Some(&fixture.working), ["add", "README.md"]);
    git(Some(&fixture.working), ["commit", "-m", "second"]);
    git(Some(&fixture.working), ["push", "origin", "main"]);
    let second_commit = git(Some(&fixture.working), ["rev-parse", "HEAD"])
        .trim()
        .to_owned();

    let result = inspector
        .inspect(&request, &authorization("repo-1", &fixture.remote), |_| {
            Ok(())
        })
        .unwrap_or_else(|_| panic!("second inspection"));
    assert_eq!(result.commit_sha, second_commit);
}

#[test]
fn local_cache_url_rewrite_is_removed_before_fetch() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let request = request("repo-1", "task-1", &fixture.remote);
    let authorization = authorization("repo-1", &fixture.remote);
    inspector
        .inspect(&request, &authorization, |_| Ok(()))
        .unwrap_or_else(|_| panic!("first inspection"));
    let cache = inspector.repository_cache_path("repo-1").join("source.git");
    git(
        Some(&cache),
        [
            "config",
            "url.file:///outside/.insteadOf",
            fixture
                .remote
                .to_str()
                .unwrap_or_else(|| panic!("remote path")),
        ],
    );
    git(Some(&cache), ["config", "credential.helper", "!false"]);
    git(Some(&cache), ["config", "protocol.file.allow", "always"]);
    inspector
        .inspect(&request, &authorization, |_| Ok(()))
        .unwrap_or_else(|_| panic!("fetch after local rewrite"));
    let local_helper = Command::new("git")
        .current_dir(&cache)
        .args(["config", "--local", "--get", "credential.helper"])
        .output()
        .unwrap_or_else(|_| panic!("inspect local helper"));
    assert!(!local_helper.status.success());
    let local_protocol = Command::new("git")
        .current_dir(&cache)
        .args(["config", "--local", "--get", "protocol.file.allow"])
        .output()
        .unwrap_or_else(|_| panic!("inspect local protocol policy"));
    assert!(!local_protocol.status.success());
}

#[test]
fn replaced_workspace_is_not_read_or_deleted() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let request = request("repo-1", "task-1", &fixture.remote);
    let authorization = authorization("repo-1", &fixture.remote);
    let mut replacement = None;
    let result = inspector.inspect(&request, &authorization, |workspace| {
        replacement = Some(workspace.to_owned());
        fs::remove_dir_all(workspace).unwrap_or_else(|_| panic!("remove original workspace"));
        fs::create_dir(workspace).unwrap_or_else(|_| panic!("replace workspace"));
        Ok(())
    });
    let error = result.expect_err("replacement must fail integrity checks");
    assert_eq!(error.phase, InspectionPhase::Revision);
    assert!(error.cleanup_failure.is_some());
    assert!(replacement
        .unwrap_or_else(|| panic!("replacement path"))
        .exists());
}

#[test]
fn concurrent_same_repository_inspections_have_independent_workspaces() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = Arc::new(inspector(&directory.0));
    let (entered_tx, entered_rx) = mpsc::channel();
    let mut release_senders = Vec::new();
    let paths = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let mut handles = Vec::new();

    for (task, own_file, other_file) in [("task-a", "a.txt", "b.txt"), ("task-b", "b.txt", "a.txt")]
    {
        let inspector = Arc::clone(&inspector);
        let entered_tx = entered_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        release_senders.push(release_tx);
        let paths = Arc::clone(&paths);
        let remote = fixture.remote.clone();
        handles.push(thread::spawn(move || {
            let authorization = authorization("repo-1", &remote);
            let result = inspector.inspect(
                &request("repo-1", task, &remote),
                &authorization,
                move |workspace| {
                    paths
                        .lock()
                        .unwrap_or_else(|_| panic!("path lock"))
                        .push(workspace.to_owned());
                    fs::write(workspace.join(own_file), own_file)
                        .unwrap_or_else(|_| panic!("workspace mutation"));
                    entered_tx
                        .send(())
                        .unwrap_or_else(|_| panic!("entered notification"));
                    release_rx
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap_or_else(|_| panic!("release notification"));
                    assert!(!workspace.join(other_file).exists());
                    Ok(())
                },
            );
            assert!(matches!(
                result,
                Err(error) if error.phase == InspectionPhase::DirtyTree
                    && error.is_contaminated()
            ));
        }));
    }
    let mut all_entered = true;
    for _ in 0..2 {
        if entered_rx.recv_timeout(Duration::from_secs(30)).is_err() {
            all_entered = false;
            break;
        }
    }
    for release_tx in release_senders {
        let _ = release_tx.send(());
    }
    let mut thread_panicked = false;
    for handle in handles {
        if handle.join().is_err() {
            thread_panicked = true;
        }
    }
    assert!(
        all_entered,
        "both repository inspections must enter runtime"
    );
    assert!(!thread_panicked, "inspection thread");

    let paths = paths.lock().unwrap_or_else(|_| panic!("path lock"));
    assert_eq!(paths.len(), 2);
    assert_ne!(paths[0], paths[1]);
    assert!(inspector
        .repository_cache_path("repo-1")
        .join("source.git")
        .is_dir());
}

#[test]
fn different_repositories_do_not_wait_on_one_keyed_lock() {
    let directory = test_directory();
    let fixture_a = fixture(&directory.0.join("a"));
    let fixture_b = fixture(&directory.0.join("b"));
    let inspector = Arc::new(inspector(&directory.0));
    let state = Arc::new((Mutex::new((0_usize, false)), Condvar::new()));
    let mut handles = Vec::new();

    for (repository_id, remote) in [("repo-a", fixture_a.remote), ("repo-b", fixture_b.remote)] {
        let inspector = Arc::clone(&inspector);
        let state = Arc::clone(&state);
        handles.push(thread::spawn(move || {
            let authorization = authorization(repository_id, &remote);
            let result = inspector.inspect(
                &request(repository_id, "task-1", &remote),
                &authorization,
                move |_| {
                    let (lock, wake) = &*state;
                    let mut state = lock.lock().unwrap_or_else(|_| panic!("start state lock"));
                    state.0 += 1;
                    wake.notify_all();
                    while !state.1 {
                        let (next, timeout) = wake
                            .wait_timeout(state, Duration::from_secs(30))
                            .unwrap_or_else(|_| panic!("start state wait"));
                        state = next;
                        if timeout.timed_out() && !state.1 {
                            return Err("different repository callback did not release".into());
                        }
                    }
                    Ok(())
                },
            );
            result.unwrap_or_else(|_| panic!("independent repository inspection"));
        }));
    }

    let (lock, wake) = &*state;
    let state = lock.lock().unwrap_or_else(|_| panic!("start state lock"));
    let (mut state, _) = wake
        .wait_timeout_while(state, Duration::from_secs(30), |state| {
            state.0 < 2 && !state.1
        })
        .unwrap_or_else(|_| panic!("wait for independent callbacks"));
    let both_started_before_release = state.0 == 2;
    state.1 = true;
    wake.notify_all();
    drop(state);
    for handle in handles {
        handle
            .join()
            .unwrap_or_else(|_| panic!("inspection thread"));
    }
    assert!(
        both_started_before_release,
        "different repository keys must run independently"
    );
}

#[test]
fn runtime_failure_and_contamination_are_reported_separately() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let seen_path = Arc::new(Mutex::new(None));
    let seen_path_in_runtime = Arc::clone(&seen_path);
    let error = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            |workspace| {
                *seen_path_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("path lock")) = Some(workspace.to_owned());
                fs::write(workspace.join("runtime.txt"), "mutated")
                    .unwrap_or_else(|_| panic!("runtime mutation"));
                Err("provider failed".into())
            },
        )
        .expect_err("runtime failure");
    assert_eq!(error.phase, InspectionPhase::Runtime);
    assert_eq!(error.reason, "provider failed");
    assert!(error.contamination.is_some());
    assert!(!error.cleanup_failed());
    assert!(!seen_path
        .lock()
        .unwrap_or_else(|_| panic!("path lock"))
        .as_ref()
        .unwrap_or_else(|| panic!("runtime workspace"))
        .exists());
}

#[test]
fn runtime_panic_is_classified_and_disposed() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let seen_path = Arc::new(Mutex::new(None));
    let seen_path_in_runtime = Arc::clone(&seen_path);
    let error = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            move |workspace| {
                *seen_path_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("path lock")) = Some(workspace.to_owned());
                panic!("runtime panic");
            },
        )
        .expect_err("runtime panic");
    assert_eq!(error.phase, InspectionPhase::Runtime);
    assert_eq!(error.reason, "runtime inspection panicked");
    assert!(!seen_path
        .lock()
        .unwrap_or_else(|_| panic!("path lock"))
        .as_ref()
        .unwrap_or_else(|| panic!("runtime workspace"))
        .exists());
}

#[test]
fn ignored_mutation_is_contamination_and_is_disposed() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let seen_path = Arc::new(Mutex::new(None));
    let seen_path_in_runtime = Arc::clone(&seen_path);
    let error = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            move |workspace| {
                *seen_path_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("path lock")) = Some(workspace.to_owned());
                fs::write(workspace.join("runtime.north-ignored"), "mutated")
                    .unwrap_or_else(|_| panic!("ignored mutation"));
                Ok(())
            },
        )
        .expect_err("ignored mutation");
    assert_eq!(error.phase, InspectionPhase::DirtyTree);
    assert!(error
        .contamination
        .as_deref()
        .is_some_and(|details| details.contains("runtime.north-ignored")));
    assert!(!seen_path
        .lock()
        .unwrap_or_else(|_| panic!("path lock"))
        .as_ref()
        .unwrap_or_else(|| panic!("runtime workspace"))
        .exists());
}

#[test]
fn cancellation_discards_workspace_without_publishing_result() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let cancellation = InspectionCancellation::new();
    let seen_path = Arc::new(Mutex::new(None));
    let seen_path_in_runtime = Arc::clone(&seen_path);
    let cancellation_in_runtime = cancellation.clone();
    let error = inspector
        .inspect_with_cancellation(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
            &cancellation,
            move |workspace, _| {
                *seen_path_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("path lock")) = Some(workspace.to_owned());
                cancellation_in_runtime.cancel();
                fs::write(workspace.join("cancelled.txt"), "discard")
                    .unwrap_or_else(|_| panic!("mutation"));
                Ok(())
            },
        )
        .expect_err("cancelled inspection");
    assert_eq!(error.phase, InspectionPhase::Cancellation);
    assert!(error.is_contaminated());
    assert!(!seen_path
        .lock()
        .unwrap_or_else(|_| panic!("path lock"))
        .as_ref()
        .unwrap_or_else(|| panic!("runtime workspace"))
        .exists());
}

#[test]
fn unknown_repository_is_rejected_before_cache_or_runtime() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let called = Arc::new(Mutex::new(false));
    let called_in_runtime = Arc::clone(&called);
    let authorization = authorization("known", &fixture.remote);
    let error = inspector
        .inspect_authorized(
            &request("unknown", "task-1", &fixture.remote),
            &authorization,
            move |_| {
                *called_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("called lock")) = true;
                Ok(())
            },
        )
        .expect_err("unknown identity");
    assert_eq!(error.phase, InspectionPhase::Authorization);
    assert!(!*called.lock().unwrap_or_else(|_| panic!("called lock")));
    assert!(!inspector.repository_cache_path("unknown").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_cache_namespace_is_rejected_before_git_access() {
    use std::os::unix::fs::symlink;

    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let cache_namespace = inspector.repository_cache_path("repo-1");
    symlink(&fixture.remote, &cache_namespace)
        .unwrap_or_else(|_| panic!("cache namespace symlink"));
    let error = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .expect_err("symlinked cache namespace");
    assert_eq!(error.phase, InspectionPhase::Cache);
    assert!(!inspector.workspace_root().join("session-1").exists());
}

#[test]
fn replaced_workspace_root_is_rejected() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let root = inspector.workspace_root().to_owned();
    let original = root.with_file_name("workspaces-original");
    fs::rename(&root, &original).unwrap_or_else(|_| panic!("move original workspace root"));
    fs::create_dir(&root).unwrap_or_else(|_| panic!("replace workspace root"));

    let error = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .expect_err("replaced root");
    assert_eq!(error.phase, InspectionPhase::Workspace);
    assert!(!inspector.startup_cleanup().is_clean());

    fs::remove_dir(&root).unwrap_or_else(|_| panic!("remove replacement root"));
    fs::rename(original, root).unwrap_or_else(|_| panic!("restore workspace root"));
}

#[test]
fn authorized_run_uses_immutable_authorization_snapshot() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let authorization = authorization("repo-1", &fixture.remote);
    // Actual catalog disable and historical citation acceptance are covered by
    // the north-server repository integration test; this checks snapshot lifetime.
    let active_repository_ids = Arc::new(Mutex::new(vec!["repo-1".to_owned()]));
    let active_repository_ids_in_runtime = Arc::clone(&active_repository_ids);
    let result = inspector
        .inspect(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization,
            move |_| {
                active_repository_ids_in_runtime
                    .lock()
                    .unwrap_or_else(|_| panic!("active catalog lock"))
                    .clear();
                Ok(())
            },
        )
        .unwrap_or_else(|_| panic!("authorized in-flight inspection"));
    assert_eq!(result.repository_id, "repo-1");
    assert!(active_repository_ids
        .lock()
        .unwrap_or_else(|_| panic!("active catalog lock"))
        .is_empty());
}

#[test]
fn startup_cleanup_removes_stale_cache_staging_but_not_source_or_unrelated() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .unwrap_or_else(|_| panic!("prepare cache"));
    inspector
        .dispose(prepared)
        .unwrap_or_else(|_| panic!("dispose cache workspace"));
    let namespace = inspector.repository_cache_path("repo-1");
    let staging = namespace.join(".source-stale-1");
    fs::create_dir_all(&staging).unwrap_or_else(|_| panic!("stale staging"));
    fs::write(staging.join("partial.pack"), "partial")
        .unwrap_or_else(|_| panic!("staging material"));
    let unrelated = inspector.cache_root().join("unrelated");
    fs::create_dir_all(unrelated.join(".source-not-a-namespace"))
        .unwrap_or_else(|_| panic!("unrelated cache directory"));
    let unrelated_file = inspector.cache_root().join("unrelated.txt");
    fs::write(&unrelated_file, "retain").unwrap_or_else(|_| panic!("unrelated cache file"));
    let root_staging = inspector.cache_root().join(".source-at-root");
    fs::create_dir(&root_staging).unwrap_or_else(|_| panic!("root staging"));

    let source_cache = namespace.join("source.git");
    let report = inspector.startup_cleanup();
    assert!(report.is_clean(), "cleanup report: {report:?}");
    assert!(report.removed.iter().any(|path| path == &staging));
    assert!(!staging.exists());
    assert!(source_cache.is_dir(), "source.git must remain reusable");
    assert!(unrelated.join(".source-not-a-namespace").is_dir());
    assert!(unrelated_file.is_file());
    assert!(root_staging.is_dir());
}

#[cfg(unix)]
#[test]
fn cache_staging_symlink_cannot_escape_cache_root() {
    use std::os::unix::fs::symlink;

    let directory = test_directory();
    let inspector = inspector(&directory.0);
    let namespace = inspector.repository_cache_path("repo-1");
    fs::create_dir_all(&namespace).unwrap_or_else(|_| panic!("cache namespace"));
    let outside = directory.0.join("outside");
    fs::create_dir_all(&outside).unwrap_or_else(|_| panic!("outside directory"));
    let marker = outside.join("must-survive");
    fs::write(&marker, "outside").unwrap_or_else(|_| panic!("outside marker"));
    let staging = namespace.join(".source-escape");
    symlink(&outside, &staging).unwrap_or_else(|_| panic!("staging symlink"));

    let report = inspector.startup_cleanup();
    assert!(!report.is_clean(), "symlink cleanup must be rejected");
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.path == staging));
    assert!(staging.is_symlink());
    assert!(marker.is_file());
}

#[test]
fn replaced_cache_root_is_rejected_without_deleting_staging() {
    let directory = test_directory();
    let inspector = inspector(&directory.0);
    let root = inspector.cache_root().to_owned();
    let original = root.with_file_name("cache-original");
    let namespace_name = inspector
        .repository_cache_path("repo-1")
        .file_name()
        .unwrap_or_else(|| panic!("cache namespace name"))
        .to_owned();
    fs::rename(&root, &original).unwrap_or_else(|_| panic!("move cache root"));
    let original_staging = original.join(&namespace_name).join(".source-original");
    fs::create_dir_all(&original_staging).unwrap_or_else(|_| panic!("original staging"));
    fs::create_dir(&root).unwrap_or_else(|_| panic!("replace cache root"));
    let replacement_staging = root.join(&namespace_name).join(".source-replacement");
    fs::create_dir_all(&replacement_staging).unwrap_or_else(|_| panic!("replacement staging"));

    let report = inspector.startup_cleanup();
    assert!(!report.is_clean(), "replaced cache root must be rejected");
    assert!(report.failures.iter().any(|failure| failure.path == root));
    assert!(original_staging.is_dir());
    assert!(replacement_staging.is_dir());

    fs::remove_dir_all(&root).unwrap_or_else(|_| panic!("remove replacement cache root"));
    fs::rename(original, root).unwrap_or_else(|_| panic!("restore cache root"));
}

#[test]
fn startup_cleanup_removes_orphans_but_not_cache() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .unwrap_or_else(|_| panic!("prepare orphan"));
    let orphan = prepared.path().to_owned();
    drop(prepared);
    let cache = inspector.repository_cache_path("repo-1");
    let report = inspector.startup_cleanup();
    assert!(report.is_clean());
    assert!(report.removed.iter().any(|path| path == &orphan));
    assert!(!orphan.exists());
    assert!(cache.join("source.git").is_dir());
}

#[test]
fn startup_cleanup_refuses_active_inspection() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .unwrap_or_else(|_| panic!("prepare active checkout"));
    let path = prepared.path().to_owned();

    let report = inspector.startup_cleanup();
    assert!(!report.is_clean());
    assert!(report
        .failures
        .iter()
        .any(|failure| { failure.reason.contains("repository inspection is active") }));
    assert!(path.is_dir());

    inspector
        .dispose(prepared)
        .unwrap_or_else(|_| panic!("dispose active checkout"));
}

#[test]
fn read_git_allowlist_keeps_runtime_on_read_operations() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization("repo-1", &fixture.remote),
        )
        .unwrap_or_else(|_| panic!("prepare checkout"));
    let status = prepared
        .read_git(
            &authorization("repo-1", &fixture.remote),
            &["status".into(), "--porcelain=v1".into()],
        )
        .unwrap_or_else(|_| panic!("read status"));
    assert!(status.trim().is_empty());
    let error = prepared
        .read_git(&authorization("repo-1", &fixture.remote), &["push".into()])
        .expect_err("deny push");
    assert_eq!(error.phase, InspectionPhase::Runtime);
    inspector
        .dispose(prepared)
        .unwrap_or_else(|_| panic!("dispose checkout"));
}

#[test]
fn cleanup_failure_leaves_workspace_for_later_orphan_recovery() {
    let directory = test_directory();
    let fixture = fixture(&directory.0);
    let inspector = inspector(&directory.0);
    let authorization = authorization("repo-1", &fixture.remote);
    let prepared = inspector
        .prepare(
            &request("repo-1", "task-1", &fixture.remote),
            &authorization,
        )
        .unwrap_or_else(|_| panic!("prepare checkout"));
    let path = prepared.path().to_owned();
    let workspace_name = path
        .file_name()
        .unwrap_or_else(|| panic!("workspace name"))
        .to_owned();
    let root = inspector.workspace_root().to_owned();
    let original = root.with_file_name("workspaces-original");
    let orphan = original.join(workspace_name);
    fs::rename(&root, &original).unwrap_or_else(|_| panic!("move workspace root"));
    fs::create_dir(&root).unwrap_or_else(|_| panic!("replace workspace root"));

    let error = inspector
        .dispose(prepared)
        .expect_err("replaced workspace root must fail cleanup");
    assert_eq!(error.phase, InspectionPhase::Cleanup);
    assert!(error.cleanup_failed());
    assert!(orphan.exists());

    fs::remove_dir(&root).unwrap_or_else(|_| panic!("remove replacement root"));
    fs::rename(&original, &root).unwrap_or_else(|_| panic!("restore workspace root"));
    assert!(path.exists());
    let report = inspector.startup_cleanup();
    assert!(
        report.removed.iter().any(|removed| removed == &path),
        "cleanup report: {report:?}"
    );
    assert!(!path.exists());
}
