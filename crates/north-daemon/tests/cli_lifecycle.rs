#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    cli: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let root = PathBuf::from("/tmp").join(format!("north-{}-{nonce}", std::process::id()));
        let home = root.join("home");
        let bin = root.join("bin");
        fs::create_dir_all(&home).expect("create test home");
        fs::create_dir_all(&bin).expect("create test bin");
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).expect("protect test home");

        let cli = bin.join("north");
        let daemon = bin.join("north-daemon");
        fs::copy(env!("CARGO_BIN_EXE_north"), &cli).expect("copy north binary");
        fs::copy(env!("CARGO_BIN_EXE_north-daemon"), &daemon).expect("copy daemon binary");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).expect("protect north binary");
        fs::set_permissions(&daemon, fs::Permissions::from_mode(0o700))
            .expect("protect daemon binary");

        Self { root, home, cli }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run north CLI")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.cli);
        command.args(args).env("HOME", &self.home);
        command
    }

    fn write_state(&self) {
        let directory = self.home.join(".north");
        fs::create_dir_all(&directory).expect("create state directory");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .expect("protect state directory");
        fs::write(
            directory.join("daemon.json"),
            r#"{"server_url":"https://127.0.0.1:9","daemon_id":"daemon-test","credential":"test-secret","capabilities":["agent"]}"#,
        )
        .expect("write test daemon state");
        fs::set_permissions(
            directory.join("daemon.json"),
            fs::Permissions::from_mode(0o600),
        )
        .expect("protect test daemon state");
    }

    fn wait_for_status(&self, expected: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let output = self.run(&["daemon", "status", "--output", "json"]);
            let body = String::from_utf8_lossy(&output.stdout).into_owned();
            if body.contains(expected) {
                return body;
            }
            assert!(
                Instant::now() < deadline,
                "status never reached {expected}: {body}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn child(&self, args: &[&str]) -> Child {
        self.command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn north CLI")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.run(&["daemon", "stop"]);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn reported_pid(output: &Output) -> Option<u32> {
    String::from_utf8_lossy(&output.stdout)
        .split("(pid ")
        .nth(1)?
        .split(')')
        .next()?
        .parse()
        .ok()
}

#[test]
fn concurrent_start_status_reconnect_and_graceful_stop() {
    let fixture = Fixture::new();
    fixture.write_state();

    let first = fixture.child(&["daemon", "start"]);
    let second = fixture.child(&["daemon", "start"]);
    let first = first
        .wait_with_output()
        .expect("wait for first daemon start");
    let second = second
        .wait_with_output()
        .expect("wait for second daemon start");
    let log = fs::read_to_string(fixture.home.join(".north/daemon.log"))
        .unwrap_or_else(|_| "<no daemon log>".into());
    assert!(
        first.status.success() && second.status.success(),
        "first start: {}; second start: {}; daemon log: {log}",
        String::from_utf8_lossy(&first.stderr),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(reported_pid(&first), reported_pid(&second));

    let status = fixture.wait_for_status("\"process_state\":\"running\"", Duration::from_secs(3));
    assert!(
        status.contains("\"connection_state\":\"reconnecting\""),
        "{status}"
    );
    assert!(!status.contains("test-secret"), "status exposed credential");

    let stopped = fixture.run(&["daemon", "stop"]);
    assert!(
        stopped.status.success(),
        "stop failed: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let status = fixture.wait_for_status("\"process_state\":\"stopped\"", Duration::from_secs(2));
    assert!(
        status.contains("\"connection_state\":\"stopped\""),
        "{status}"
    );
}

#[test]
fn stale_socket_is_reported_and_never_removed_or_signaled() {
    let fixture = Fixture::new();
    fixture.write_state();
    let socket = fixture.home.join(".north/daemon-control.sock");
    fs::write(&socket, "stale socket marker").expect("write stale socket marker");

    let status = fixture.run(&["daemon", "status", "--output", "json"]);
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(status.status.success());
    assert!(body.contains("\"process_state\":\"stale\""), "{body}");

    let start = fixture.run(&["daemon", "start"]);
    assert!(!start.status.success());
    assert!(String::from_utf8_lossy(&start.stderr).contains("refusing to replace"));
    let stop = fixture.run(&["daemon", "stop"]);
    assert!(!stop.status.success());
    assert!(String::from_utf8_lossy(&stop.stderr).contains("no PID signal was sent"));
    assert!(fs::read_to_string(socket).is_ok_and(|contents| contents == "stale socket marker"));
}
