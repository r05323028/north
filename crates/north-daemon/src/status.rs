use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    fs::OpenOptions,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Stale,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Starting,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DaemonStatus {
    pub instance_id: String,
    pub pid: u32,
    pub server_url: String,
    pub daemon_id: String,
    pub process_state: ProcessState,
    pub connection_state: ConnectionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
}

pub fn default_state_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".north/daemon.json")
}

pub fn sidecar_path(state_path: &Path, name: &str) -> PathBuf {
    state_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

pub fn status_path(state_path: &Path) -> PathBuf {
    sidecar_path(state_path, "daemon-status.json")
}

pub fn control_socket_path(state_path: &Path) -> PathBuf {
    sidecar_path(state_path, "daemon-control.sock")
}

pub fn log_path(state_path: &Path) -> PathBuf {
    sidecar_path(state_path, "daemon.log")
}

pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon state parent is not a directory",
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

pub fn read_status(path: &Path) -> io::Result<Option<DaemonStatus>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn write_status(path: &Path, status: &DaemonStatus) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_private_directory(parent)?;

    let temporary =
        path.with_extension(format!("tmp-{}-{}", std::process::id(), status.instance_id));
    let result = (|| {
        let bytes = serde_json::to_vec(status)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    fn sample_status() -> DaemonStatus {
        DaemonStatus {
            instance_id: "123-456".into(),
            pid: 123,
            server_url: "https://north.example".into(),
            daemon_id: "daemon-1".into(),
            process_state: ProcessState::Running,
            connection_state: ConnectionState::Connected,
            failure_class: None,
        }
    }

    #[test]
    fn status_file_is_atomic_readable_and_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state/daemon-status.json");

        write_status(&path, &sample_status()).unwrap();

        assert_eq!(read_status(&path).unwrap(), Some(sample_status()));
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn status_json_has_stable_states_and_no_credential_field() {
        let value = serde_json::to_value(sample_status()).unwrap();
        assert_eq!(value["process_state"], "running");
        assert_eq!(value["connection_state"], "connected");
        assert!(value.get("credential").is_none());
    }
}
