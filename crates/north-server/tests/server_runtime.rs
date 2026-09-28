use std::{
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Output, Stdio},
    thread,
    time::Duration,
};

const TEST_OTP_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn free_address() -> SocketAddr {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve loopback port")
        .local_addr()
        .expect("read loopback address")
}

fn reserved_address() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve loopback port");
    let address = listener.local_addr().expect("read loopback address");
    (listener, address)
}

fn spawn_server(database_url: &str, otp_key: Option<&str>, address: SocketAddr) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_north-server"));
    command
        .env("DATABASE_URL", database_url)
        .env("NORTH_BIND_ADDR", address.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    match otp_key {
        Some(value) => {
            command.env("NORTH_OTP_HMAC_KEY", value);
        }
        None => {
            command.env_remove("NORTH_OTP_HMAC_KEY");
        }
    }
    command.spawn().expect("spawn north-server")
}

fn health_request(address: SocketAddr) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(100)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .expect("set health read timeout");
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write health request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read health response");
    Some(response)
}

fn wait_for_health(child: &mut Child, address: SocketAddr) -> String {
    for _ in 0..200 {
        if let Some(status) = child.try_wait().expect("poll server process") {
            panic!("server exited before health: {status}");
        }
        if let Some(response) = health_request(address) {
            return response;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("server did not expose health endpoint at {address}");
}

fn wait_for_exit(mut child: Child) -> Output {
    for _ in 0..1600 {
        if child.try_wait().expect("poll server process").is_some() {
            return child.wait_with_output().expect("read server output");
        }
        thread::sleep(Duration::from_millis(25));
    }
    child.kill().expect("stop stuck server process");
    child
        .wait_with_output()
        .expect("read stopped server output")
}

fn assert_startup_failure(output: &Output, category: &str) {
    assert!(!output.status.success());
    let message = stderr(output);
    assert!(
        message.contains(&format!("north-server startup failed: {category}")),
        "server did not fail with expected startup category: {category}"
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn missing_otp_key_fails_before_listening_without_secret_output() {
    let (reservation, address) = reserved_address();
    let database_url = "postgres://north:database-secret@127.0.0.1:1/north";
    let output = wait_for_exit(spawn_server(database_url, None, address));

    assert_startup_failure(&output, "configuration");
    assert!(!stderr(&output).contains("database-secret"));
    assert!(!stderr(&output).contains(TEST_OTP_KEY));
    drop(reservation);
}

#[test]
fn malformed_otp_key_fails_before_listening_without_secret_output() {
    let (reservation, address) = reserved_address();
    let invalid_key = "not-a-valid-otp-key";
    let output = wait_for_exit(spawn_server(
        "postgres://north:database-secret@127.0.0.1:1/north",
        Some(invalid_key),
        address,
    ));

    assert_startup_failure(&output, "configuration");
    assert!(!stderr(&output).contains(invalid_key));
    assert!(!stderr(&output).contains("database-secret"));
    drop(reservation);
}

#[test]
fn unavailable_database_fails_before_listening_without_database_url_output() {
    let (reservation, address) = reserved_address();
    let database_url = "postgres://north:database-secret@127.0.0.1:1/north";
    let output = wait_for_exit(spawn_server(database_url, Some(TEST_OTP_KEY), address));

    assert_startup_failure(&output, "database connection");
    assert!(!stderr(&output).contains(database_url));
    assert!(!stderr(&output).contains("database-secret"));
    drop(reservation);
}

#[test]
#[ignore = "requires NORTH_TEST_DATABASE_URL pointing at an isolated PostgreSQL database"]
fn valid_startup_exposes_health_and_shutdown_is_graceful() {
    let database_url = env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for server runtime tests");
    let address = free_address();
    let mut child = spawn_server(&database_url, Some(TEST_OTP_KEY), address);
    let response = wait_for_health(&mut child, address);

    assert!(
        response.starts_with("HTTP/1.1 200"),
        "unexpected health response: {response}"
    );
    assert!(
        response.ends_with("ok\n"),
        "unexpected health body: {response}"
    );

    thread::sleep(Duration::from_secs(11));
    let long_running_response = wait_for_health(&mut child, address);
    assert!(
        long_running_response.starts_with("HTTP/1.1 200"),
        "server stopped before termination signal: {long_running_response}"
    );

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .expect("send SIGTERM");
        assert!(status.success(), "kill command failed: {status}");
    }
    #[cfg(not(unix))]
    child.kill().expect("stop server");

    let output = wait_for_exit(child);
    assert!(
        output.status.success(),
        "graceful shutdown failed: {}",
        stderr(&output)
    );
}

#[test]
#[ignore = "requires NORTH_TEST_MIGRATION_FAILURE_DATABASE_URL with migration permission denied"]
fn migration_failure_fails_before_listening_without_database_url_output() {
    let database_url = env::var("NORTH_TEST_MIGRATION_FAILURE_DATABASE_URL")
        .expect("NORTH_TEST_MIGRATION_FAILURE_DATABASE_URL is required");
    let (reservation, address) = reserved_address();
    let output = wait_for_exit(spawn_server(&database_url, Some(TEST_OTP_KEY), address));

    assert_startup_failure(&output, "migration (");
    assert!(!stderr(&output).contains(&database_url));
    assert!(!stderr(&output).contains(TEST_OTP_KEY));
    drop(reservation);
}
