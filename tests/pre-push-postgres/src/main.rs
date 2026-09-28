use std::{
    error::Error,
    ffi::{OsStr, OsString},
    io::{self, ErrorKind},
    process::{Command, ExitCode, ExitStatus},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use signal_hook::{
    consts::signal::{SIGINT, SIGTERM},
    flag,
};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{core::Mount, runners::SyncRunner, ImageExt},
};

const POSTGRES_TAG: &str = "16";
const POSTGRES_PORT: u16 = 5432;
const POSTGRES_DATABASE: &str = "north";
const POSTGRES_USER: &str = "north";
const POSTGRES_PASSWORD: &str = "north";
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn database_url(host: &str, port: u16) -> String {
    format!("postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@{host}:{port}/{POSTGRES_DATABASE}")
}

fn child_command() -> io::Result<(OsString, Vec<OsString>)> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new("--")) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "usage: north-pre-push-postgres -- <command> [args...]",
        ));
    }
    let command = args.next().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "missing validation command after --",
        )
    })?;
    Ok((command, args.collect()))
}

// ponytail: SIGKILL/host loss bypass cleanup; add a daemon-level reaper if hard-crash cleanup is required.
fn install_signal_handlers() -> Result<Arc<AtomicUsize>, Box<dyn Error>> {
    let received = Arc::new(AtomicUsize::new(0));
    flag::register_usize(SIGINT, Arc::clone(&received), SIGINT as usize)?;
    flag::register_usize(SIGTERM, Arc::clone(&received), SIGTERM as usize)?;
    Ok(received)
}

fn take_signal(received: &AtomicUsize) -> Option<usize> {
    match received.swap(0, Ordering::Relaxed) {
        0 => None,
        signal => Some(signal),
    }
}

fn child_status(
    command: OsString,
    args: Vec<OsString>,
    url: String,
    received: &AtomicUsize,
) -> io::Result<(ExitStatus, Option<usize>)> {
    let mut child = Command::new(command)
        .args(args)
        .env("NORTH_TEST_DATABASE_URL", url)
        .spawn()?;

    loop {
        if let Some(signal) = take_signal(received) {
            let _ = child.kill();
            return Ok((child.wait()?, Some(signal)));
        }
        if let Some(status) = child.try_wait()? {
            return Ok((status, take_signal(received)));
        }
        thread::sleep(CHILD_POLL_INTERVAL);
    }
}

fn exit_code(status: ExitStatus, signal: Option<usize>) -> ExitCode {
    signal
        .map(signal_exit_code)
        .unwrap_or_else(|| ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn signal_exit_code(signal: usize) -> ExitCode {
    ExitCode::from((128 + signal) as u8)
}

fn run() -> Result<ExitCode, Box<dyn Error>> {
    let (command, args) = child_command()?;
    let received = install_signal_handlers()?;

    eprintln!("Starting disposable PostgreSQL {POSTGRES_TAG} via Testcontainers...");
    let container = Postgres::default()
        .with_db_name(POSTGRES_DATABASE)
        .with_user(POSTGRES_USER)
        .with_password(POSTGRES_PASSWORD)
        .with_mount(Mount::tmpfs_mount("/var/lib/postgresql/data"))
        .with_tag(POSTGRES_TAG)
        .start()?;

    let endpoint = (|| -> Result<(String, u16), Box<dyn Error>> {
        let host = container.get_host()?.to_string();
        let port = container.get_host_port_ipv4(POSTGRES_PORT)?;
        Ok((host, port))
    })();
    let (host, port) = match endpoint {
        Ok(endpoint) => endpoint,
        Err(error) => {
            if let Err(cleanup_error) = container.rm() {
                eprintln!("pre-push PostgreSQL container cleanup failed: {cleanup_error}");
            }
            return Err(error);
        }
    };

    if let Some(signal) = take_signal(&received) {
        container.rm()?;
        return Ok(signal_exit_code(signal));
    }

    let child = child_status(command, args, database_url(&host, port), &received);
    let cleanup = container.rm();
    match child {
        Ok((status, signal)) => {
            if let Err(error) = cleanup {
                eprintln!("pre-push PostgreSQL container cleanup failed: {error}");
                if status.success() && signal.is_none() {
                    return Err(error.into());
                }
            }
            Ok(exit_code(status, signal))
        }
        Err(error) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!("pre-push PostgreSQL container cleanup failed: {cleanup_error}");
            }
            Err(error.into())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("pre-push PostgreSQL runner: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::database_url;

    #[test]
    fn connection_url_uses_discovered_host_and_port() {
        assert_eq!(
            database_url("docker.internal", 49_152),
            "postgres://north:north@docker.internal:49152/north"
        );
    }
}
