//! Host-Git source preparation for daemon-side repository inspection.
//!
//! This module owns local source material and disposable checkouts only. It
//! does not access server persistence, store credentials, or decide business
//! state.

use north_protocol::{RepositoryContext, ReviewedRepositoryWire};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fmt::{self, Write as _},
    fs,
    io::Read,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, TryLockError,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(not(any(unix, windows)))]
compile_error!("north-daemon requires stable filesystem identity support");

/// Git subcommands exposed to runtime inspection. Everything else is denied.
pub const READ_ONLY_GIT_COMMANDS: &[&str] = &[
    "cat-file",
    "describe",
    "diff",
    "log",
    "ls-files",
    "rev-parse",
    "show",
    "status",
];

const WORKSPACE_PREFIX: &str = "workspace-";
const CACHE_STAGING_PREFIX: &str = ".source-";
const WORKSPACE_MARKER: &str = "north-workspace-identity";
const GIT_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_GIT_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FsIdentity {
    first: u64,
    second: u64,
}

#[derive(Debug, Default)]
struct CleanupGate {
    state: Mutex<CleanupGateState>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct CleanupGateState {
    active_operations: usize,
    cleanup_running: bool,
}

#[derive(Debug)]
struct OperationLease {
    gate: Arc<CleanupGate>,
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active_operations = state.active_operations.saturating_sub(1);
        self.gate.wake.notify_all();
    }
}

#[derive(Debug, Clone)]
struct OperationPermit {
    _lease: Arc<OperationLease>,
}

#[derive(Debug)]
struct CleanupPermit {
    gate: Arc<CleanupGate>,
}

impl Drop for CleanupPermit {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.cleanup_running = false;
        self.gate.wake.notify_all();
    }
}

impl CleanupGate {
    #[cfg(test)]
    fn enter_operation(self: &Arc<Self>) -> OperationPermit {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.cleanup_running {
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.active_operations += 1;
        drop(state);
        OperationPermit {
            _lease: Arc::new(OperationLease {
                gate: Arc::clone(self),
            }),
        }
    }

    fn enter_operation_with_cancellation(
        self: &Arc<Self>,
        cancellation: &InspectionCancellation,
    ) -> Result<OperationPermit, InspectionError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.cleanup_running {
            if cancellation.is_cancelled() {
                return Err(InspectionError::new(
                    InspectionPhase::Cancellation,
                    "inspection cancelled while waiting for repository cleanup",
                ));
            }
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        if cancellation.is_cancelled() {
            return Err(InspectionError::new(
                InspectionPhase::Cancellation,
                "inspection cancelled before repository preparation",
            ));
        }
        state.active_operations += 1;
        drop(state);
        Ok(OperationPermit {
            _lease: Arc::new(OperationLease {
                gate: Arc::clone(self),
            }),
        })
    }

    fn begin_cleanup(self: &Arc<Self>) -> Result<CleanupPermit, String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.cleanup_running {
            return Err("startup cleanup is already running".into());
        }
        if state.active_operations != 0 {
            return Err("startup cleanup skipped while repository inspection is active".into());
        }
        state.cleanup_running = true;
        drop(state);
        Ok(CleanupPermit {
            gate: Arc::clone(self),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceIdentity {
    name: String,
    workspace: FsIdentity,
    git_directory: FsIdentity,
    marker: FsIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheStagingIdentity {
    namespace: FsIdentity,
    staging: FsIdentity,
}

struct CheckoutContext<'a> {
    cache_root: &'a Path,
    cache_root_identity: FsIdentity,
    cache: &'a Path,
    workspace_root: &'a Path,
    workspace_root_identity: FsIdentity,
    workspace: &'a Path,
    commit_sha: &'a str,
}

static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static CACHE_CLEANUP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionPhase {
    Authorization,
    Cache,
    Workspace,
    Revision,
    Runtime,
    Cancellation,
    DirtyTree,
    Cleanup,
}

impl fmt::Display for InspectionPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Authorization => "authorization",
            Self::Cache => "cache",
            Self::Workspace => "workspace",
            Self::Revision => "revision",
            Self::Runtime => "runtime",
            Self::Cancellation => "cancellation",
            Self::DirtyTree => "dirty-tree",
            Self::Cleanup => "cleanup",
        };
        f.write_str(name)
    }
}

/// Failure facts keep runtime failure, contamination, and cleanup failure
/// separate so callers do not mistake one for another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionError {
    pub phase: InspectionPhase,
    pub reason: String,
    pub contamination: Option<String>,
    pub cleanup_failure: Option<String>,
}

impl InspectionError {
    fn new(phase: InspectionPhase, reason: impl Into<String>) -> Self {
        Self {
            phase,
            reason: reason.into(),
            contamination: None,
            cleanup_failure: None,
        }
    }

    pub fn is_contaminated(&self) -> bool {
        matches!(self.phase, InspectionPhase::DirtyTree) || self.contamination.is_some()
    }

    pub fn cleanup_failed(&self) -> bool {
        self.cleanup_failure.is_some() || matches!(self.phase, InspectionPhase::Cleanup)
    }
}

impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "repository inspection {}: {}", self.phase, self.reason)?;
        if let Some(contamination) = &self.contamination {
            write!(f, "; contamination: {contamination}")?;
        }
        if let Some(cleanup_failure) = &self.cleanup_failure {
            write!(f, "; cleanup failed: {cleanup_failure}")?;
        }
        Ok(())
    }
}

impl std::error::Error for InspectionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupFailure {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed: Vec<PathBuf>,
    pub failures: Vec<CleanupFailure>,
}

impl CleanupReport {
    fn append(&mut self, mut other: Self) {
        self.removed.append(&mut other.removed);
        self.failures.append(&mut other.failures);
    }

    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Cooperative cancellation handle passed to a runtime adapter.
#[derive(Debug, Clone, Default)]
pub struct InspectionCancellation {
    cancelled: Arc<AtomicBool>,
}

impl InspectionCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_shared_flag(cancelled: Arc<AtomicBool>) -> Self {
        Self { cancelled }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

fn ensure_not_cancelled(
    cancellation: &InspectionCancellation,
    reason: impl Into<String>,
) -> Result<(), InspectionError> {
    if cancellation.is_cancelled() {
        Err(InspectionError::new(InspectionPhase::Cancellation, reason))
    } else {
        Ok(())
    }
}

fn lock_repository<'a>(
    lock: &'a Arc<Mutex<()>>,
    cancellation: &InspectionCancellation,
) -> Result<std::sync::MutexGuard<'a, ()>, InspectionError> {
    loop {
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled while waiting for repository",
        )?;
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn cleanup_workspace_after_error(
    mut error: InspectionError,
    root: &Path,
    path: &Path,
    root_identity: FsIdentity,
    identity: Option<&WorkspaceIdentity>,
) -> InspectionError {
    if let Some(identity) = identity {
        if let Err(cleanup_failure) =
            remove_owned_workspace_path(root, path, root_identity, identity)
        {
            error.cleanup_failure = Some(cleanup_failure);
        }
    } else {
        error.cleanup_failure = Some(
            "workspace ownership was not established; path retained for startup cleanup".into(),
        );
    }
    error
}

/// Host-Git source metadata. Production callers should construct this through
/// [`RepositorySource::from_context`], which is the server-wire boundary;
/// [`RepositorySource::new`] also supports local Git fixtures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySource {
    pub repository_id: String,
    pub url: String,
}

impl RepositorySource {
    pub fn new(repository_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            repository_id: repository_id.into(),
            url: url.into(),
        }
    }

    pub fn from_context(context: &RepositoryContext) -> Result<Self, InspectionError> {
        let source = Self::new(&context.repository_id, &context.url);
        source.validate()?;
        if !server_repository_location(&source.url) {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "repository context URL is outside server Git URL policy",
            ));
        }
        Ok(source)
    }

    fn validate(&self) -> Result<(), InspectionError> {
        if self.repository_id.trim().is_empty() {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "repository ID is empty",
            ));
        }
        if self.url.trim().is_empty() || self.url.chars().any(char::is_whitespace) {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "repository URL is empty or contains whitespace",
            ));
        }
        if !credential_free_location(&self.url) {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "repository URL contains credentials or query data",
            ));
        }
        Ok(())
    }
}

/// Immutable repository authorization copied from one server-assembled run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAuthorization {
    session_id: String,
    repositories: Vec<RepositorySource>,
}

impl RunAuthorization {
    pub fn new(
        session_id: impl Into<String>,
        repositories: Vec<RepositorySource>,
    ) -> Result<Self, InspectionError> {
        let session_id = session_id.into();
        if session_id.trim().is_empty() {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "authorization session ID is empty",
            ));
        }
        let mut repository_ids = HashSet::with_capacity(repositories.len());
        for repository in &repositories {
            repository.validate()?;
            if !repository_ids.insert(&repository.repository_id) {
                return Err(InspectionError::new(
                    InspectionPhase::Authorization,
                    format!(
                        "repository {:?} is authorized more than once",
                        repository.repository_id
                    ),
                ));
            }
        }
        Ok(Self {
            session_id,
            repositories,
        })
    }

    pub fn from_session_start(
        session_id: impl Into<String>,
        start: &north_protocol::SessionStart,
    ) -> Result<Self, InspectionError> {
        let repositories = start
            .repositories
            .iter()
            .map(RepositorySource::from_context)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(session_id, repositories)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn repositories(&self) -> &[RepositorySource] {
        &self.repositories
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionRequest {
    pub session_id: String,
    pub task_id: String,
    pub repository: RepositorySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedGitOutput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
}

impl InspectionRequest {
    pub fn new(
        session_id: impl Into<String>,
        task_id: impl Into<String>,
        repository: RepositorySource,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            task_id: task_id.into(),
            repository,
        }
    }

    fn validate(&self) -> Result<(), InspectionError> {
        if self.session_id.trim().is_empty() || self.task_id.trim().is_empty() {
            return Err(InspectionError::new(
                InspectionPhase::Authorization,
                "session and task IDs are required",
            ));
        }
        self.repository.validate()
    }
}

#[derive(Debug, Clone)]
pub struct PreparedWorkspace {
    session_id: String,
    repository_id: String,
    repository_url: String,
    commit_sha: String,
    workspace_root: PathBuf,
    workspace_root_identity: FsIdentity,
    workspace_identity: WorkspaceIdentity,
    git_config: Vec<u8>,
    path: PathBuf,
    _cleanup_permit: OperationPermit,
}

impl PartialEq for PreparedWorkspace {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.repository_id == other.repository_id
            && self.repository_url == other.repository_url
            && self.commit_sha == other.commit_sha
            && self.workspace_root == other.workspace_root
            && self.workspace_root_identity == other.workspace_root_identity
            && self.workspace_identity == other.workspace_identity
            && self.git_config == other.git_config
            && self.path == other.path
    }
}

impl Eq for PreparedWorkspace {}

impl PreparedWorkspace {
    pub fn repository_id(&self) -> &str {
        &self.repository_id
    }

    pub fn commit_sha(&self) -> &str {
        &self.commit_sha
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_git(
        &self,
        authorization: &RunAuthorization,
        args: &[String],
    ) -> Result<String, InspectionError> {
        let source = RepositorySource::new(&self.repository_id, &self.repository_url);
        validate_authorized_source(&self.session_id, &source, authorization)?;
        validate_workspace_path(
            &self.workspace_root,
            self.workspace_root_identity,
            &self.path,
        )
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
        validate_git_config(self)
            .map_err(|reason| InspectionError::new(InspectionPhase::Runtime, reason))?;
        run_read_git(self, args)
    }

    /// Execute one authorized read-only Git operation with a hard stdout cap.
    /// Oversized output is returned as a deterministic prefix instead of
    /// turning one large tracked file or listing into a fatal inspection error.
    pub fn read_git_bounded(
        &self,
        authorization: &RunAuthorization,
        args: &[String],
        max_output_bytes: usize,
    ) -> Result<String, InspectionError> {
        self.read_git_bounded_output(authorization, args, max_output_bytes, None)
            .map(|output| String::from_utf8_lossy(&output.bytes).into_owned())
    }

    /// Read bounded source bytes while allowing repository preparation/runtime
    /// cancellation to stop the Git child.
    pub(crate) fn read_git_bounded_with_cancellation(
        &self,
        authorization: &RunAuthorization,
        args: &[String],
        max_output_bytes: usize,
        cancellation: &InspectionCancellation,
    ) -> Result<BoundedGitOutput, InspectionError> {
        self.read_git_bounded_output(authorization, args, max_output_bytes, Some(cancellation))
    }

    fn read_git_bounded_output(
        &self,
        authorization: &RunAuthorization,
        args: &[String],
        max_output_bytes: usize,
        cancellation: Option<&InspectionCancellation>,
    ) -> Result<BoundedGitOutput, InspectionError> {
        let source = RepositorySource::new(&self.repository_id, &self.repository_url);
        validate_authorized_source(&self.session_id, &source, authorization)?;
        validate_workspace_path(
            &self.workspace_root,
            self.workspace_root_identity,
            &self.path,
        )
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
        validate_git_config(self)
            .map_err(|reason| InspectionError::new(InspectionPhase::Runtime, reason))?;
        if !read_git_allowed(args) {
            return Err(InspectionError::new(
                InspectionPhase::Runtime,
                "Git command is outside the read-only allowlist",
            ));
        }
        git_output_bounded_with_cancellation(
            &self.path,
            &self.workspace_root,
            self.workspace_root_identity,
            Some(&self.workspace_identity),
            None,
            args.to_vec(),
            max_output_bytes,
            cancellation,
        )
        .map_err(|error| inspection_error_from_git(InspectionPhase::Runtime, error))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionResult {
    pub repository_id: String,
    pub commit_sha: String,
}

impl InspectionResult {
    pub fn reviewed_repository(&self) -> ReviewedRepositoryWire {
        ReviewedRepositoryWire {
            repository_id: self.repository_id.clone(),
            commit_sha: self.commit_sha.clone(),
        }
    }
}

/// Cache and workspace roots are daemon-owned mode-0700 namespaces. Direct
/// child allocation and canonical rechecks reject ordinary path redirection;
/// North 0.1 does not claim kernel-level sandboxing against a privileged actor.
#[derive(Clone)]
pub struct RepositoryInspector {
    cache_root: PathBuf,
    cache_root_identity: FsIdentity,
    workspace_root: PathBuf,
    workspace_root_identity: FsIdentity,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    cleanup_gate: Arc<CleanupGate>,
}

impl fmt::Debug for RepositoryInspector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryInspector")
            .field("cache_root", &self.cache_root)
            .field("workspace_root", &self.workspace_root)
            .finish_non_exhaustive()
    }
}

impl RepositoryInspector {
    /// Create an inspector with separate reusable-cache and disposable roots.
    /// Overlapping roots are rejected so startup cleanup cannot delete cache
    /// material.
    pub fn new(
        cache_root: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<Self, InspectionError> {
        let cache_root = cache_root.into();
        let workspace_root = workspace_root.into();
        reject_symlink_root(&cache_root, InspectionPhase::Cache)?;
        reject_symlink_root(&workspace_root, InspectionPhase::Workspace)?;
        fs::create_dir_all(&cache_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Cache,
                format!("create cache root {}: {error}", cache_root.display()),
            )
        })?;
        fs::create_dir_all(&workspace_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Workspace,
                format!(
                    "create workspace root {}: {error}",
                    workspace_root.display()
                ),
            )
        })?;
        reject_symlink_root(&cache_root, InspectionPhase::Cache)?;
        reject_symlink_root(&workspace_root, InspectionPhase::Workspace)?;
        restrict_root_permissions(&cache_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Cache,
                format!("restrict cache root {}: {error}", cache_root.display()),
            )
        })?;
        restrict_root_permissions(&workspace_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Workspace,
                format!(
                    "restrict workspace root {}: {error}",
                    workspace_root.display()
                ),
            )
        })?;
        let cache_root = fs::canonicalize(&cache_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Cache,
                format!("resolve cache root {}: {error}", cache_root.display()),
            )
        })?;
        let workspace_root = fs::canonicalize(&workspace_root).map_err(|error| {
            InspectionError::new(
                InspectionPhase::Workspace,
                format!(
                    "resolve workspace root {}: {error}",
                    workspace_root.display()
                ),
            )
        })?;
        if roots_overlap(&cache_root, &workspace_root) {
            return Err(InspectionError::new(
                InspectionPhase::Workspace,
                "cache and disposable workspace roots must not overlap",
            ));
        }
        let cache_root_identity = directory_identity(&cache_root)
            .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
        let workspace_root_identity = directory_identity(&workspace_root)
            .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
        Ok(Self {
            cache_root,
            cache_root_identity,
            workspace_root,
            workspace_root_identity,
            locks: Arc::new(Mutex::new(HashMap::new())),
            cleanup_gate: Arc::new(CleanupGate::default()),
        })
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Return one repository's cache namespace. Runtime adapters should not
    /// pass this path to their provider; use [`PreparedWorkspace::path`].
    pub fn repository_cache_path(&self, repository_id: &str) -> PathBuf {
        self.cache_root.join(path_component(repository_id))
    }

    /// Remove stale cache staging and disposable workspaces in separate,
    /// independently bounded cleanup passes.
    pub fn startup_cleanup(&self) -> CleanupReport {
        let _cleanup = match self.cleanup_gate.begin_cleanup() {
            Ok(permit) => permit,
            Err(reason) => {
                return CleanupReport {
                    removed: Vec::new(),
                    failures: vec![CleanupFailure {
                        path: self.cache_root.clone(),
                        reason,
                    }],
                };
            }
        };
        let mut report = cleanup_stale_cache_staging(&self.cache_root, self.cache_root_identity);
        report.append(cleanup_stale_workspaces(
            &self.workspace_root,
            self.workspace_root_identity,
        ));
        report
    }

    /// Prepare a detached, independently mutable checkout. The per-repository
    /// lock remains held through cache mutation, clone, and revision checks.
    pub fn prepare(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
    ) -> Result<PreparedWorkspace, InspectionError> {
        let cancellation = InspectionCancellation::new();
        self.prepare_authorized_with_cancellation(request, authorization, &cancellation)
    }

    fn validate_cache_root(&self) -> Result<(), InspectionError> {
        validate_root_identity(&self.cache_root, self.cache_root_identity)
            .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))
    }

    fn validate_workspace_root(&self) -> Result<(), InspectionError> {
        validate_root_identity(&self.workspace_root, self.workspace_root_identity)
            .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))
    }

    fn validate_roots(&self) -> Result<(), InspectionError> {
        self.validate_cache_root()?;
        self.validate_workspace_root()
    }

    fn prepare_unchecked(
        &self,
        request: &InspectionRequest,
        cancellation: &InspectionCancellation,
    ) -> Result<PreparedWorkspace, InspectionError> {
        request.validate()?;
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before repository preparation",
        )?;
        let operation = self
            .cleanup_gate
            .enter_operation_with_cancellation(cancellation)?;
        self.validate_roots()?;
        let lock = self.lock_for(&request.repository.repository_id);
        let _guard = lock_repository(&lock, cancellation)?;

        let cache = self.ensure_cache(&request.repository, cancellation)?;
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before revision resolution",
        )?;
        self.validate_cache_root()?;
        validate_cache_path(&self.cache_root, self.cache_root_identity, &cache)
            .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
        let commit_sha = resolve_commit(
            &cache,
            &self.cache_root,
            self.cache_root_identity,
            None,
            Some(cancellation),
        )?;
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before checkout allocation",
        )?;
        let path = self.allocate_workspace(request)?;
        let mut checkout_identity = None;
        if let Err(error) = create_checkout(
            CheckoutContext {
                cache_root: &self.cache_root,
                cache_root_identity: self.cache_root_identity,
                cache: &cache,
                workspace_root: &self.workspace_root,
                workspace_root_identity: self.workspace_root_identity,
                workspace: &path,
                commit_sha: &commit_sha,
            },
            &mut checkout_identity,
            cancellation,
        ) {
            return Err(cleanup_workspace_after_error(
                error,
                &self.workspace_root,
                &path,
                self.workspace_root_identity,
                checkout_identity.as_ref(),
            ));
        }
        if cancellation.is_cancelled() {
            return Err(cleanup_workspace_after_error(
                InspectionError::new(
                    InspectionPhase::Cancellation,
                    "inspection cancelled after checkout",
                ),
                &self.workspace_root,
                &path,
                self.workspace_root_identity,
                checkout_identity.as_ref(),
            ));
        }
        let workspace_identity = match checkout_identity {
            Some(identity) => identity,
            None => {
                return Err(cleanup_workspace_after_error(
                    InspectionError::new(
                        InspectionPhase::Workspace,
                        "checkout identity was not captured",
                    ),
                    &self.workspace_root,
                    &path,
                    self.workspace_root_identity,
                    None,
                ));
            }
        };
        if cancellation.is_cancelled() {
            return Err(cleanup_workspace_after_error(
                InspectionError::new(
                    InspectionPhase::Cancellation,
                    "inspection cancelled before checkout config capture",
                ),
                &self.workspace_root,
                &path,
                self.workspace_root_identity,
                Some(&workspace_identity),
            ));
        }
        let git_config = match read_git_config(&path) {
            Ok(git_config) => git_config,
            Err(reason) => {
                return Err(cleanup_workspace_after_error(
                    InspectionError::new(InspectionPhase::Workspace, reason),
                    &self.workspace_root,
                    &path,
                    self.workspace_root_identity,
                    Some(&workspace_identity),
                ));
            }
        };
        if cancellation.is_cancelled() {
            return Err(cleanup_workspace_after_error(
                InspectionError::new(
                    InspectionPhase::Cancellation,
                    "inspection cancelled after checkout config capture",
                ),
                &self.workspace_root,
                &path,
                self.workspace_root_identity,
                Some(&workspace_identity),
            ));
        }
        Ok(PreparedWorkspace {
            session_id: request.session_id.clone(),
            repository_id: request.repository.repository_id.clone(),
            repository_url: request.repository.url.clone(),
            commit_sha,
            workspace_root: self.workspace_root.clone(),
            workspace_root_identity: self.workspace_root_identity,
            workspace_identity,
            git_config,
            path,
            _cleanup_permit: operation,
        })
    }

    /// Enforce the immutable server-supplied run repository set before any
    /// cache access or workspace creation.
    pub fn prepare_authorized(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
    ) -> Result<PreparedWorkspace, InspectionError> {
        self.prepare(request, authorization)
    }

    pub fn prepare_authorized_with_cancellation(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
        cancellation: &InspectionCancellation,
    ) -> Result<PreparedWorkspace, InspectionError> {
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before repository authorization",
        )?;
        validate_repository_selection(request, authorization)?;
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled after repository authorization",
        )?;
        self.prepare_unchecked(request, cancellation)
    }

    pub fn dispose(&self, workspace: PreparedWorkspace) -> Result<(), InspectionError> {
        remove_owned_workspace_path(
            &workspace.workspace_root,
            &workspace.path,
            workspace.workspace_root_identity,
            &workspace.workspace_identity,
        )
        .map_err(|reason| {
            InspectionError::new(
                InspectionPhase::Cleanup,
                format!("remove {}: {reason}", workspace.path.display()),
            )
        })
    }

    /// Run runtime callback with only disposable checkout path, then apply
    /// dirty-tree detection and cleanup on every normal terminal path. The
    /// synchronous callback is fully awaited before cleanup; adapters must
    /// stop/await child work before returning after cancellation.
    pub fn inspect<F>(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
        runtime: F,
    ) -> Result<InspectionResult, InspectionError>
    where
        F: FnOnce(&Path) -> Result<(), String>,
    {
        let cancellation = InspectionCancellation::new();
        self.inspect_with_cancellation(request, authorization, &cancellation, |path, _| {
            runtime(path)
        })
    }

    pub fn inspect_authorized<F>(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
        runtime: F,
    ) -> Result<InspectionResult, InspectionError>
    where
        F: FnOnce(&Path) -> Result<(), String>,
    {
        self.inspect(request, authorization, runtime)
    }

    /// Run an authorized callback while prepared checkout remains alive.
    /// Cleanup and post-inspection revision/dirty checks happen after callback
    /// returns, including cancellation paths.
    pub fn inspect_prepared_with_cancellation<F>(
        &self,
        workspace: PreparedWorkspace,
        cancellation: &InspectionCancellation,
        runtime: F,
    ) -> Result<InspectionResult, InspectionError>
    where
        F: FnOnce(&PreparedWorkspace, &InspectionCancellation) -> Result<(), String>,
    {
        let primary_failure = if cancellation.is_cancelled() {
            Some((
                InspectionPhase::Cancellation,
                "inspection cancelled before runtime dispatch".to_owned(),
            ))
        } else {
            match catch_unwind(AssertUnwindSafe(|| runtime(&workspace, cancellation))) {
                Ok(Ok(())) if cancellation.is_cancelled() => Some((
                    InspectionPhase::Cancellation,
                    "inspection cancelled".to_owned(),
                )),
                Ok(Ok(())) => None,
                Ok(Err(reason)) if cancellation.is_cancelled() => Some((
                    InspectionPhase::Cancellation,
                    non_empty_reason(reason, "inspection cancelled"),
                )),
                Ok(Err(reason)) => Some((
                    InspectionPhase::Runtime,
                    non_empty_reason(reason, "runtime inspection failed"),
                )),
                Err(_) => Some((
                    InspectionPhase::Runtime,
                    "runtime inspection panicked".to_owned(),
                )),
            }
        };
        self.finish(workspace, primary_failure)
    }

    /// Cancellation is cooperative at this synchronous boundary: no cleanup
    /// starts while the runtime callback is still using the checkout.
    pub fn inspect_with_cancellation<F>(
        &self,
        request: &InspectionRequest,
        authorization: &RunAuthorization,
        cancellation: &InspectionCancellation,
        runtime: F,
    ) -> Result<InspectionResult, InspectionError>
    where
        F: FnOnce(&Path, &InspectionCancellation) -> Result<(), String>,
    {
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before repository authorization",
        )?;
        validate_repository_selection(request, authorization)?;
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled after repository authorization",
        )?;
        let workspace = self.prepare_unchecked(request, cancellation)?;
        self.inspect_prepared_with_cancellation(
            workspace,
            cancellation,
            |workspace, cancellation| runtime(workspace.path(), cancellation),
        )
    }

    fn lock_for(&self, repository_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks
            .entry(repository_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn ensure_cache(
        &self,
        source: &RepositorySource,
        cancellation: &InspectionCancellation,
    ) -> Result<PathBuf, InspectionError> {
        ensure_not_cancelled(
            cancellation,
            "inspection cancelled before cache preparation",
        )?;
        self.validate_cache_root()?;
        let repository_root = create_owned_child(
            &self.cache_root,
            self.cache_root_identity,
            path_component(&source.repository_id),
            InspectionPhase::Cache,
        )?;
        let namespace_identity =
            validate_cache_namespace(&self.cache_root, self.cache_root_identity, &repository_root)
                .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
        let cache = repository_root.join("source.git");
        if fs::symlink_metadata(&cache)
            .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
            .unwrap_or(false)
        {
            return Err(InspectionError::new(
                InspectionPhase::Cache,
                format!("repository cache {} is not a directory", cache.display()),
            ));
        }
        if cache.exists() {
            validate_expected_cache_namespace(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
            )
            .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
            validate_cache_path(&self.cache_root, self.cache_root_identity, &cache)
                .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
            ensure_not_cancelled(cancellation, "inspection cancelled before cache validation")?;
            sanitize_local_git_config(
                &cache,
                &self.cache_root,
                self.cache_root_identity,
                None,
                InspectionPhase::Cache,
                cancellation,
            )?;
            let remote = git_output_for_phase(
                &cache,
                &self.cache_root,
                self.cache_root_identity,
                None,
                None,
                vec!["remote", "get-url", "origin"],
                InspectionPhase::Cache,
                Some(cancellation),
            )?;
            if remote.trim() != source.url.trim() {
                return Err(InspectionError::new(
                    InspectionPhase::Cache,
                    "repository cache origin does not match configured source",
                ));
            }
            validate_cache_path(&self.cache_root, self.cache_root_identity, &cache)
                .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
            git_output_for_phase(
                &cache,
                &self.cache_root,
                self.cache_root_identity,
                None,
                None,
                vec!["remote", "update", "--prune"],
                InspectionPhase::Cache,
                Some(cancellation),
            )?;
            ensure_not_cancelled(cancellation, "inspection cancelled after cache update")?;
            return Ok(cache);
        }

        let temporary = repository_root.join(format!(
            "{CACHE_STAGING_PREFIX}{}-{}",
            std::process::id(),
            next_workspace_sequence()
        ));
        let staging_identity = create_owned_cache_staging_path(
            &self.cache_root,
            self.cache_root_identity,
            &repository_root,
            &temporary,
            namespace_identity,
        )?;
        let staged_cache = temporary.join("source.git");
        if let Err(reason) = ensure_path_absent(&staged_cache) {
            let mut error = InspectionError::new(InspectionPhase::Cache, reason);
            if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
                staging_identity.staging,
                &temporary,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        if cancellation.is_cancelled() {
            let mut error = InspectionError::new(
                InspectionPhase::Cancellation,
                "inspection cancelled before repository clone",
            );
            if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
                staging_identity.staging,
                &temporary,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        let clone = git_output_for_phase(
            &self.cache_root,
            &self.cache_root,
            self.cache_root_identity,
            None,
            Some((&self.workspace_root, self.workspace_root_identity)),
            vec![
                "clone".to_owned(),
                "--mirror".to_owned(),
                "--".to_owned(),
                source.url.clone(),
                staged_cache.to_string_lossy().into_owned(),
            ],
            InspectionPhase::Cache,
            Some(cancellation),
        );
        if let Err(mut error) = clone {
            if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
                staging_identity.staging,
                &temporary,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        if cancellation.is_cancelled() {
            let mut error = InspectionError::new(
                InspectionPhase::Cancellation,
                "inspection cancelled after repository clone",
            );
            if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
                staging_identity.staging,
                &temporary,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        let current_staging_identity = match validate_cache_staging_path(
            &self.cache_root,
            self.cache_root_identity,
            &repository_root,
            &temporary,
        ) {
            Ok(identity) => identity,
            Err(reason) => {
                let mut error = InspectionError::new(InspectionPhase::Cache, reason);
                if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                    &self.cache_root,
                    self.cache_root_identity,
                    &repository_root,
                    namespace_identity,
                    staging_identity.staging,
                    &temporary,
                ) {
                    error.cleanup_failure = Some(cleanup_failure);
                }
                return Err(error);
            }
        };
        if current_staging_identity != staging_identity {
            let mut error = InspectionError::new(
                InspectionPhase::Cache,
                "temporary repository cache identity changed during clone",
            );
            if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                namespace_identity,
                staging_identity.staging,
                &temporary,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        let staged_cache_identity =
            match validate_staged_cache_path(&temporary, staging_identity.staging, &staged_cache) {
                Ok(identity) => identity,
                Err(reason) => {
                    let mut error = InspectionError::new(InspectionPhase::Cache, reason);
                    if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
                        &self.cache_root,
                        self.cache_root_identity,
                        &repository_root,
                        namespace_identity,
                        staging_identity.staging,
                        &temporary,
                    ) {
                        error.cleanup_failure = Some(cleanup_failure);
                    }
                    return Err(error);
                }
            };
        if let Err(reason) = validate_expected_cache_namespace(
            &self.cache_root,
            self.cache_root_identity,
            &repository_root,
            namespace_identity,
        )
        .and_then(|()| ensure_path_absent(&cache))
        {
            let mut error = InspectionError::new(InspectionPhase::Cache, reason);
            if let Err(cleanup_failure) = remove_owned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                &temporary,
                staging_identity,
            ) {
                error.cleanup_failure = Some(cleanup_failure);
            }
            return Err(error);
        }
        if let Err(error) = fs::rename(&staged_cache, &cache) {
            let mut failure = InspectionError::new(
                InspectionPhase::Cache,
                format!("install repository cache {}: {error}", cache.display()),
            );
            if let Err(cleanup_failure) = remove_owned_cache_staging_path(
                &self.cache_root,
                self.cache_root_identity,
                &repository_root,
                &temporary,
                staging_identity,
            ) {
                failure.cleanup_failure = Some(cleanup_failure);
            }
            return Err(failure);
        }
        let cleanup_staging_after_install =
            |mut error: InspectionError| -> Result<PathBuf, InspectionError> {
                if let Err(cleanup_failure) = remove_owned_cache_staging_path(
                    &self.cache_root,
                    self.cache_root_identity,
                    &repository_root,
                    &temporary,
                    staging_identity,
                ) {
                    error.cleanup_failure = Some(cleanup_failure);
                }
                Err(error)
            };
        if let Err(reason) = validate_expected_cache_namespace(
            &self.cache_root,
            self.cache_root_identity,
            &repository_root,
            namespace_identity,
        )
        .and_then(|()| validate_cache_path(&self.cache_root, self.cache_root_identity, &cache))
        {
            return cleanup_staging_after_install(InspectionError::new(
                InspectionPhase::Cache,
                reason,
            ));
        }
        let installed_identity = match fs::symlink_metadata(&cache) {
            Ok(metadata) => metadata_identity(&metadata),
            Err(error) => {
                return cleanup_staging_after_install(InspectionError::new(
                    InspectionPhase::Cache,
                    error.to_string(),
                ));
            }
        };
        if installed_identity != staged_cache_identity {
            return cleanup_staging_after_install(InspectionError::new(
                InspectionPhase::Cache,
                "installed repository cache identity changed",
            ));
        }
        if let Err(reason) = remove_owned_cache_staging_path(
            &self.cache_root,
            self.cache_root_identity,
            &repository_root,
            &temporary,
            staging_identity,
        ) {
            let mut error = InspectionError::new(
                InspectionPhase::Cache,
                "temporary repository cache cleanup failed",
            );
            error.cleanup_failure = Some(reason);
            return Err(error);
        }
        // Canonical source.git is never disposable cleanup; failed post-install
        // validation leaves it for the next identity-checked recovery attempt.
        ensure_not_cancelled(cancellation, "inspection cancelled before cache completion")?;
        sanitize_local_git_config(
            &cache,
            &self.cache_root,
            self.cache_root_identity,
            None,
            InspectionPhase::Cache,
            cancellation,
        )?;
        ensure_not_cancelled(cancellation, "inspection cancelled after cache preparation")?;
        Ok(cache)
    }

    fn allocate_workspace(&self, request: &InspectionRequest) -> Result<PathBuf, InspectionError> {
        self.validate_workspace_root()?;
        let prefix = format!(
            "{WORKSPACE_PREFIX}{}-{}-{}-",
            path_component(&request.session_id),
            path_component(&request.task_id),
            path_component(&request.repository.repository_id)
        );
        loop {
            let path = self.workspace_root.join(format!(
                "{prefix}{}-{}",
                std::process::id(),
                next_workspace_sequence()
            ));
            match fs::symlink_metadata(&path) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path),
                Err(error) => {
                    return Err(InspectionError::new(
                        InspectionPhase::Workspace,
                        format!("inspect workspace destination {}: {error}", path.display()),
                    ));
                }
            }
        }
    }

    fn finish(
        &self,
        workspace: PreparedWorkspace,
        primary_failure: Option<(InspectionPhase, String)>,
    ) -> Result<InspectionResult, InspectionError> {
        let result = InspectionResult {
            repository_id: workspace.repository_id.clone(),
            commit_sha: workspace.commit_sha.clone(),
        };
        let mut failure =
            primary_failure.map(|(phase, reason)| InspectionError::new(phase, reason));
        record_revision_failure(&mut failure, &workspace);
        record_dirty_failure(&mut failure, &workspace);
        if let Err(reason) = remove_owned_workspace_path(
            &workspace.workspace_root,
            &workspace.path,
            workspace.workspace_root_identity,
            &workspace.workspace_identity,
        ) {
            if let Some(error) = failure.as_mut() {
                error.cleanup_failure = Some(reason);
            } else {
                failure = Some(InspectionError::new(
                    InspectionPhase::Cleanup,
                    format!("remove {}: {reason}", workspace.path.display()),
                ));
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(result),
        }
    }
}

/// Reject an inspection identity not present in the immutable run context.
pub fn validate_repository_selection(
    request: &InspectionRequest,
    authorization: &RunAuthorization,
) -> Result<(), InspectionError> {
    validate_authorized_source(&request.session_id, &request.repository, authorization)
}

fn validate_authorized_source(
    session_id: &str,
    source: &RepositorySource,
    authorization: &RunAuthorization,
) -> Result<(), InspectionError> {
    if session_id != authorization.session_id {
        return Err(InspectionError::new(
            InspectionPhase::Authorization,
            "inspection session does not match run authorization",
        ));
    }
    match authorization
        .repositories
        .iter()
        .find(|repository| repository.repository_id == source.repository_id)
    {
        None => Err(InspectionError::new(
            InspectionPhase::Authorization,
            format!(
                "repository {:?} is not authorized for this run",
                source.repository_id
            ),
        )),
        Some(repository) if repository.url != source.url => Err(InspectionError::new(
            InspectionPhase::Authorization,
            "repository URL does not match run authorization",
        )),
        Some(_) => Ok(()),
    }
}

/// Execute one Git read operation from the disposable checkout. Runtime code
/// cannot use this helper for checkout mutation, push, fetch, or worktrees.
fn run_read_git(workspace: &PreparedWorkspace, args: &[String]) -> Result<String, InspectionError> {
    if !read_git_allowed(args) {
        return Err(InspectionError::new(
            InspectionPhase::Runtime,
            "Git command is outside the read-only allowlist",
        ));
    }
    git_output(
        workspace.path(),
        &workspace.workspace_root,
        workspace.workspace_root_identity,
        Some(&workspace.workspace_identity),
        None,
        args.to_vec(),
    )
    .map_err(|reason| InspectionError::new(InspectionPhase::Runtime, reason))
}

fn read_git_allowed(args: &[String]) -> bool {
    let Some(command) = args.first().map(String::as_str) else {
        return false;
    };
    READ_ONLY_GIT_COMMANDS.contains(&command)
        && !args.iter().skip(1).any(|argument| {
            argument.starts_with("-c")
                || argument == "--config"
                || argument.starts_with("--config=")
                || argument == "--config-env"
                || argument.starts_with("--config-env=")
                || argument.starts_with("-C")
                || argument.starts_with("--git-dir")
                || argument.starts_with("--work-tree")
                || argument.starts_with("--super-prefix")
                || argument.starts_with("--exec-path")
                || argument.starts_with("--upload-pack")
                || argument == "--ext-diff"
                || argument == "--textconv"
                || argument == "--no-index"
                || argument.starts_with("--output")
                || argument == "-o"
                || argument.starts_with("-o")
        })
}

const GIT_OBJECT_ID_HEX_WIDTHS: &[usize] = &[20 * 2, 32 * 2];

fn complete_commit_sha(value: &str) -> bool {
    GIT_OBJECT_ID_HEX_WIDTHS.contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn server_repository_location(url: &str) -> bool {
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("ssh://"))
    {
        let Some((authority, path)) = rest.split_once('/') else {
            return false;
        };
        return !authority.is_empty() && !path.is_empty() && credential_free_location(url);
    }
    let Some(rest) = url.strip_prefix("git@") else {
        return false;
    };
    let Some((host, path)) = rest.split_once(':') else {
        return false;
    };
    !host.is_empty() && !path.is_empty() && !host.contains(['/', '@'])
}

fn credential_free_location(url: &str) -> bool {
    if url.contains(['?', '#']) {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return url
            .split_once('@')
            .is_none_or(|(user, host)| user == "git" && !host.contains('@'));
    };
    let Some((authority, _)) = rest.split_once('/') else {
        return false;
    };
    match scheme {
        "https" => !authority.contains('@'),
        "ssh" => authority
            .split_once('@')
            .is_none_or(|(user, host)| user == "git" && !host.contains('@')),
        _ => !authority.contains('@'),
    }
}

fn resolve_commit(
    repository: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    cancellation: Option<&InspectionCancellation>,
) -> Result<String, InspectionError> {
    let sha = git_output_for_phase(
        repository,
        root,
        expected_root_identity,
        workspace_identity,
        None,
        ["rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"],
        InspectionPhase::Revision,
        cancellation,
    )?
    .trim()
    .to_owned();
    // Git returns its canonical object spelling here. Re-resolving the
    // captured value below rejects an abbreviated result without assuming a
    // particular object-ID width.
    if !complete_commit_sha(&sha) {
        return Err(InspectionError::new(
            InspectionPhase::Revision,
            "Git returned an invalid full commit object ID",
        ));
    }
    let canonical = git_output_for_phase(
        repository,
        root,
        expected_root_identity,
        workspace_identity,
        None,
        vec![
            "rev-parse".to_owned(),
            "--verify".to_owned(),
            "--end-of-options".to_owned(),
            format!("{sha}^{{commit}}"),
        ],
        InspectionPhase::Revision,
        cancellation,
    )?
    .trim()
    .to_owned();
    if canonical != sha || !canonical.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(InspectionError::new(
            InspectionPhase::Revision,
            "Git did not return a complete canonical commit object ID",
        ));
    }
    Ok(canonical)
}

fn metadata_identity(metadata: &fs::Metadata) -> FsIdentity {
    #[cfg(unix)]
    {
        FsIdentity {
            first: metadata.dev(),
            second: metadata.ino(),
        }
    }
    #[cfg(windows)]
    {
        FsIdentity {
            first: metadata.volume_serial_number().unwrap_or_default() as u64,
            second: metadata.file_index(),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        panic!("filesystem identity unsupported on this platform")
    }
}

fn directory_identity(path: &Path) -> Result<FsIdentity, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("configured root is not a real directory".into());
    }
    Ok(metadata_identity(&metadata))
}

fn validate_root_identity(root: &Path, expected: FsIdentity) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("configured root was replaced by a non-directory or symlink".into());
    }
    if metadata_identity(&metadata) != expected {
        return Err("configured root was replaced".into());
    }
    let canonical = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if canonical != root {
        return Err("configured root was redirected".into());
    }
    Ok(())
}

fn validate_cache_namespace(
    root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
) -> Result<FsIdentity, String> {
    validate_root_identity(root, expected_root_identity)?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if repository_root.parent() != Some(root.as_path()) {
        return Err("repository cache namespace is not a direct child of daemon-owned root".into());
    }
    let metadata = fs::symlink_metadata(repository_root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("repository cache namespace is not a real directory".into());
    }
    let canonical = fs::canonicalize(repository_root).map_err(|error| error.to_string())?;
    if canonical.parent() != Some(root.as_path()) {
        return Err("repository cache namespace is outside daemon-owned root".into());
    }
    Ok(metadata_identity(&metadata))
}

fn validate_expected_cache_namespace(
    root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    expected_namespace_identity: FsIdentity,
) -> Result<(), String> {
    let identity = validate_cache_namespace(root, expected_root_identity, repository_root)?;
    if identity != expected_namespace_identity {
        return Err("repository cache namespace identity changed".into());
    }
    Ok(())
}

fn ensure_path_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err("Git destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn create_owned_cache_staging_path(
    cache_root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    staging: &Path,
    expected_namespace_identity: FsIdentity,
) -> Result<CacheStagingIdentity, InspectionError> {
    if staging.parent() != Some(repository_root)
        || !staging.file_name().is_some_and(is_cache_staging_name)
    {
        return Err(InspectionError::new(
            InspectionPhase::Cache,
            "temporary repository cache is not a direct child of its namespace",
        ));
    }
    let namespace_identity =
        validate_cache_namespace(cache_root, expected_root_identity, repository_root)
            .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
    if namespace_identity != expected_namespace_identity {
        return Err(InspectionError::new(
            InspectionPhase::Cache,
            "repository cache namespace identity changed",
        ));
    }
    ensure_path_absent(staging)
        .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
    fs::create_dir(staging).map_err(|error| {
        InspectionError::new(
            InspectionPhase::Cache,
            format!(
                "create temporary repository cache {}: {error}",
                staging.display()
            ),
        )
    })?;
    let created_staging_identity = match fs::symlink_metadata(staging) {
        Ok(metadata) => metadata_identity(&metadata),
        Err(error) => {
            let mut inspection_error = InspectionError::new(
                InspectionPhase::Cache,
                format!(
                    "temporary repository cache {} ownership could not be verified; path retained: {error}",
                    staging.display()
                ),
            );
            inspection_error.cleanup_failure = Some(
                "temporary repository cache ownership was not established; path retained".into(),
            );
            return Err(inspection_error);
        }
    };
    let cleanup_created = |reason: String| -> Result<CacheStagingIdentity, InspectionError> {
        let mut error = InspectionError::new(InspectionPhase::Cache, reason);
        if let Err(cleanup_failure) = cleanup_abandoned_cache_staging_path(
            cache_root,
            expected_root_identity,
            repository_root,
            expected_namespace_identity,
            created_staging_identity,
            staging,
        ) {
            error.cleanup_failure = Some(cleanup_failure);
        }
        Err(error)
    };
    if let Err(error) = restrict_root_permissions(staging) {
        return cleanup_created(format!(
            "restrict temporary repository cache {}: {error}",
            staging.display()
        ));
    }
    let identity = match validate_cache_staging_path(
        cache_root,
        expected_root_identity,
        repository_root,
        staging,
    ) {
        Ok(identity) => identity,
        Err(reason) => return cleanup_created(reason),
    };
    if identity.namespace != expected_namespace_identity {
        return cleanup_created("repository cache namespace identity changed".into());
    }
    Ok(identity)
}

fn validate_staged_cache_path(
    staging_root: &Path,
    expected_staging_identity: FsIdentity,
    cache: &Path,
) -> Result<FsIdentity, String> {
    let staging_metadata = fs::symlink_metadata(staging_root).map_err(|error| error.to_string())?;
    if staging_metadata.file_type().is_symlink() || !staging_metadata.is_dir() {
        return Err("temporary repository cache is not an owned directory".into());
    }
    if metadata_identity(&staging_metadata) != expected_staging_identity {
        return Err("temporary repository cache identity changed".into());
    }
    if cache.parent() != Some(staging_root) || cache.file_name() != Some(OsStr::new("source.git")) {
        return Err("staged repository cache has an invalid destination".into());
    }
    let metadata = fs::symlink_metadata(cache).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("staged repository cache is not a real directory".into());
    }
    let canonical_root = fs::canonicalize(staging_root).map_err(|error| error.to_string())?;
    let canonical = fs::canonicalize(cache).map_err(|error| error.to_string())?;
    if canonical.parent() != Some(canonical_root.as_path())
        || canonical.file_name() != Some(OsStr::new("source.git"))
    {
        return Err("staged repository cache resolves outside staging".into());
    }
    Ok(metadata_identity(&metadata))
}

fn validate_cache_staging_path(
    cache_root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    staging: &Path,
) -> Result<CacheStagingIdentity, String> {
    let namespace = validate_cache_namespace(cache_root, expected_root_identity, repository_root)?;
    let metadata = fs::symlink_metadata(staging).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("temporary repository cache is not an owned directory".into());
    }
    if staging.parent() != Some(repository_root) {
        return Err("temporary repository cache is not a direct child of its namespace".into());
    }
    let canonical_root = fs::canonicalize(repository_root).map_err(|error| error.to_string())?;
    let canonical = fs::canonicalize(staging).map_err(|error| error.to_string())?;
    if canonical.parent() != Some(canonical_root.as_path())
        || !is_cache_staging_name(
            canonical
                .file_name()
                .ok_or_else(|| "temporary repository cache has no name".to_owned())?,
        )
    {
        return Err("temporary repository cache resolves outside daemon-owned root".into());
    }
    Ok(CacheStagingIdentity {
        namespace,
        staging: metadata_identity(&metadata),
    })
}

fn remove_owned_cache_staging_path(
    cache_root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    path: &Path,
    expected_identity: CacheStagingIdentity,
) -> Result<(), String> {
    validate_root_identity(cache_root, expected_root_identity)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let current_identity = validate_cache_staging_path(
                cache_root,
                expected_root_identity,
                repository_root,
                path,
            )?;
            if current_identity != expected_identity {
                return Err("temporary repository cache identity changed".into());
            }
            remove_owned_cache_path(
                cache_root,
                expected_root_identity,
                repository_root,
                path,
                expected_identity,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn validate_cache_path(
    root: &Path,
    expected_root_identity: FsIdentity,
    cache: &Path,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let repository_root = cache
        .parent()
        .ok_or_else(|| "cache path has no repository namespace".to_owned())?;
    let cache_metadata = fs::symlink_metadata(cache).map_err(|error| error.to_string())?;
    let repository_metadata =
        fs::symlink_metadata(repository_root).map_err(|error| error.to_string())?;
    if cache_metadata.file_type().is_symlink()
        || repository_metadata.file_type().is_symlink()
        || !cache_metadata.is_dir()
        || !repository_metadata.is_dir()
    {
        return Err("cache path is not a real owned directory".into());
    }
    let canonical_repository =
        fs::canonicalize(repository_root).map_err(|error| error.to_string())?;
    let canonical_cache = fs::canonicalize(cache).map_err(|error| error.to_string())?;
    if canonical_repository.parent() != Some(root.as_path())
        || canonical_cache.parent() != Some(canonical_repository.as_path())
        || cache.file_name() != Some(OsStr::new("source.git"))
    {
        return Err("cache path is outside its daemon-owned namespace".into());
    }
    Ok(())
}

fn write_workspace_identity(workspace: &Path, identity: &str) -> Result<(), String> {
    let marker = workspace.join(".git").join(WORKSPACE_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(_) => return Err("workspace identity marker already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    fs::write(marker, identity.as_bytes()).map_err(|error| error.to_string())
}

fn capture_workspace_identity(path: &Path) -> Result<WorkspaceIdentity, String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "workspace has no valid identity name".to_owned())?
        .to_owned();
    let workspace_metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if workspace_metadata.file_type().is_symlink() || !workspace_metadata.is_dir() {
        return Err("workspace is not an owned directory".into());
    }
    let git_directory = path.join(".git");
    let git_metadata = fs::symlink_metadata(&git_directory).map_err(|error| error.to_string())?;
    if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
        return Err("checkout Git directory is not an owned directory".into());
    }
    let marker = git_directory.join(WORKSPACE_MARKER);
    let marker_metadata = fs::symlink_metadata(&marker).map_err(|error| error.to_string())?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err("workspace identity marker is not an owned file".into());
    }
    let value = fs::read(&marker).map_err(|error| error.to_string())?;
    if value != name.as_bytes() {
        return Err("workspace identity marker has unexpected value".into());
    }
    Ok(WorkspaceIdentity {
        name,
        workspace: metadata_identity(&workspace_metadata),
        git_directory: metadata_identity(&git_metadata),
        marker: metadata_identity(&marker_metadata),
    })
}

fn validate_workspace_identity(workspace: &PreparedWorkspace) -> Result<(), String> {
    validate_workspace_identity_at(&workspace.path, &workspace.workspace_identity)
}

fn validate_workspace_identity_at(path: &Path, identity: &WorkspaceIdentity) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "workspace has no valid identity name".to_owned())?;
    if name != identity.name {
        return Err("workspace identity name changed".into());
    }
    let workspace_metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if workspace_metadata.file_type().is_symlink() || !workspace_metadata.is_dir() {
        return Err("workspace is not an owned directory".into());
    }
    if metadata_identity(&workspace_metadata) != identity.workspace {
        return Err("workspace filesystem identity changed".into());
    }
    let git_directory = path.join(".git");
    let git_metadata = fs::symlink_metadata(&git_directory).map_err(|error| error.to_string())?;
    if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
        return Err("checkout Git directory is not an owned directory".into());
    }
    if metadata_identity(&git_metadata) != identity.git_directory {
        return Err("checkout Git directory identity changed".into());
    }
    let marker = git_directory.join(WORKSPACE_MARKER);
    let marker_metadata = fs::symlink_metadata(&marker).map_err(|error| error.to_string())?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err("workspace identity marker is not an owned file".into());
    }
    if metadata_identity(&marker_metadata) != identity.marker {
        return Err("workspace identity marker identity changed".into());
    }
    let value = fs::read(marker).map_err(|error| error.to_string())?;
    if value != identity.name.as_bytes() {
        return Err("workspace identity marker changed".into());
    }
    Ok(())
}

fn read_git_config(workspace: &Path) -> Result<Vec<u8>, String> {
    let git_directory = workspace.join(".git");
    let git_metadata = fs::symlink_metadata(&git_directory).map_err(|error| error.to_string())?;
    if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
        return Err("checkout Git directory is not an owned directory".into());
    }
    let config = git_directory.join("config");
    let metadata = fs::symlink_metadata(&config).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("checkout Git config is not an owned file".into());
    }
    fs::read(config).map_err(|error| error.to_string())
}

fn validate_git_config(workspace: &PreparedWorkspace) -> Result<(), String> {
    validate_workspace_identity(workspace)?;
    let current = read_git_config(&workspace.path)?;
    if current != workspace.git_config {
        return Err("checkout Git config changed after preparation".into());
    }
    Ok(())
}

fn dirty_details_for_workspace(workspace: &PreparedWorkspace) -> Result<Option<String>, String> {
    validate_workspace_path(
        &workspace.workspace_root,
        workspace.workspace_root_identity,
        &workspace.path,
    )?;
    validate_git_config(workspace)?;
    dirty_details(
        &workspace.path,
        &workspace.workspace_root,
        workspace.workspace_root_identity,
        &workspace.workspace_identity,
    )
}

fn record_revision_failure(failure: &mut Option<InspectionError>, workspace: &PreparedWorkspace) {
    let revision_failure = match resolve_commit(
        &workspace.path,
        &workspace.workspace_root,
        workspace.workspace_root_identity,
        Some(&workspace.workspace_identity),
        None,
    ) {
        Ok(commit) if commit == workspace.commit_sha => None,
        Ok(_) => Some(InspectionError::new(
            InspectionPhase::Revision,
            "checkout revision changed after inspection",
        )),
        Err(error) => Some(error),
    };
    if let Some(revision_failure) = revision_failure {
        if let Some(error) = failure.as_mut() {
            error.reason = format!("{}; {}", error.reason, revision_failure.reason);
        } else {
            *failure = Some(revision_failure);
        }
    }
}

fn record_dirty_failure(failure: &mut Option<InspectionError>, workspace: &PreparedWorkspace) {
    match dirty_details_for_workspace(workspace) {
        Ok(Some(details)) => {
            if let Some(error) = failure.as_mut() {
                error.contamination = Some(details);
            } else {
                *failure = Some(InspectionError {
                    phase: InspectionPhase::DirtyTree,
                    reason: "unexpected changes detected in disposable checkout".into(),
                    contamination: Some(details),
                    cleanup_failure: None,
                });
            }
        }
        Ok(None) => {}
        Err(reason) => {
            if let Some(error) = failure.as_mut() {
                error.contamination = Some(format!("workspace integrity check failed: {reason}"));
            } else {
                *failure = Some(InspectionError::new(
                    InspectionPhase::DirtyTree,
                    format!("workspace integrity check failed: {reason}"),
                ));
            }
        }
    }
}

fn create_checkout(
    context: CheckoutContext<'_>,
    workspace_identity: &mut Option<WorkspaceIdentity>,
    cancellation: &InspectionCancellation,
) -> Result<(), InspectionError> {
    let CheckoutContext {
        cache_root,
        cache_root_identity,
        cache,
        workspace_root,
        workspace_root_identity,
        workspace,
        commit_sha,
    } = context;
    ensure_not_cancelled(
        cancellation,
        "inspection cancelled before checkout creation",
    )?;
    validate_cache_path(cache_root, cache_root_identity, cache)
        .map_err(|reason| InspectionError::new(InspectionPhase::Cache, reason))?;
    validate_workspace_destination(workspace_root, workspace_root_identity, workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    let command_directory = cache.parent().unwrap_or_else(|| Path::new("."));
    git_output_for_phase(
        command_directory,
        cache_root,
        cache_root_identity,
        None,
        Some((workspace_root, workspace_root_identity)),
        vec![
            "clone".to_owned(),
            "--no-local".to_owned(),
            "--no-checkout".to_owned(),
            "--".to_owned(),
            cache.to_string_lossy().into_owned(),
            workspace.to_string_lossy().into_owned(),
        ],
        InspectionPhase::Workspace,
        Some(cancellation),
    )?;
    ensure_not_cancelled(cancellation, "inspection cancelled after checkout clone")?;
    validate_workspace_path(workspace_root, workspace_root_identity, workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    restrict_root_permissions(workspace)
        .map_err(|error| InspectionError::new(InspectionPhase::Workspace, error.to_string()))?;
    sanitize_local_git_config(
        workspace,
        workspace_root,
        workspace_root_identity,
        None,
        InspectionPhase::Workspace,
        cancellation,
    )?;
    ensure_not_cancelled(
        cancellation,
        "inspection cancelled after checkout sanitization",
    )?;
    validate_workspace_path(workspace_root, workspace_root_identity, workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    write_workspace_identity(
        workspace,
        workspace
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or(WORKSPACE_PREFIX),
    )
    .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    let identity = capture_workspace_identity(workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    *workspace_identity = Some(identity);
    validate_workspace_path(workspace_root, workspace_root_identity, workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    git_output_for_phase(
        workspace,
        workspace_root,
        workspace_root_identity,
        workspace_identity.as_ref(),
        None,
        vec![
            "checkout".to_owned(),
            "--detach".to_owned(),
            "--force".to_owned(),
            commit_sha.to_owned(),
        ],
        InspectionPhase::Revision,
        Some(cancellation),
    )?;
    ensure_not_cancelled(cancellation, "inspection cancelled after checkout revision")?;
    validate_workspace_path(workspace_root, workspace_root_identity, workspace)
        .map_err(|reason| InspectionError::new(InspectionPhase::Workspace, reason))?;
    let actual = resolve_commit(
        workspace,
        workspace_root,
        workspace_root_identity,
        workspace_identity.as_ref(),
        Some(cancellation),
    )?;
    if actual != commit_sha {
        return Err(InspectionError::new(
            InspectionPhase::Revision,
            format!("detached checkout resolved to {actual:?}, expected {commit_sha:?}"),
        ));
    }
    Ok(())
}

fn dirty_details(
    workspace: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    identity: &WorkspaceIdentity,
) -> Result<Option<String>, String> {
    let output = git_output(
        workspace,
        root,
        expected_root_identity,
        Some(identity),
        None,
        [
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    let output = output.trim();
    Ok((!output.is_empty()).then(|| output.to_owned()))
}

fn sanitize_git_path_environment(command: &mut Command) {
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        // SSH agent/command and host proxy are operator-owned Git access.
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_DIFF_OPTS");
}

fn remove_git_config_environment(command: &mut Command) {
    command
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env_remove("GIT_CONFIG_NOSYSTEM")
        .env_remove("GIT_CONFIG_WORKTREE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_CONFIG_KEY_")
            || key.to_string_lossy().starts_with("GIT_CONFIG_VALUE_")
        {
            command.env_remove(key);
        }
    }
}

fn sanitize_git_environment(command: &mut Command) {
    sanitize_git_path_environment(command);
    remove_git_config_environment(command);
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty_git_config());
}

fn git_host_config_overrides(
    cancellation: Option<&InspectionCancellation>,
) -> Result<Vec<(String, String)>, GitCommandError> {
    let mut overrides = Vec::new();
    for scope in ["--system", "--global"] {
        let mut command = Command::new("git");
        sanitize_git_path_environment(&mut command);
        remove_git_config_environment(&mut command);
        command.args([
            "config",
            scope,
            "--get-regexp",
            r"^(credential\..*|core\.sshcommand|ssh\.variant)$",
        ]);
        let output = match run_git_child(
            command,
            format!("config {scope}"),
            cancellation,
            Some(MAX_GIT_COMMAND_OUTPUT_BYTES),
            false,
        ) {
            Ok(output) => output,
            Err(GitCommandError::Cancelled) => return Err(GitCommandError::Cancelled),
            Err(_) => continue,
        };
        for (key, value) in String::from_utf8_lossy(&output.bytes)
            .lines()
            .filter_map(|line| line.split_once(char::is_whitespace))
            .filter(|(key, _)| {
                *key == "core.sshcommand"
                    || *key == "ssh.variant"
                    || *key == "credential.helper"
                    || *key == "credential.usehttppath"
                    || key.ends_with(".helper")
            })
        {
            overrides.push((key.to_owned(), value.to_owned()));
        }
    }
    Ok(overrides)
}

fn apply_git_config_overrides(command: &mut Command, overrides: &[(String, String)]) {
    command.env("GIT_CONFIG_COUNT", overrides.len().to_string());
    for (index, (key, value)) in overrides.iter().enumerate() {
        command.env(format!("GIT_CONFIG_KEY_{index}"), key);
        command.env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
}

fn empty_git_config() -> &'static OsStr {
    #[cfg(windows)]
    {
        OsStr::new("NUL")
    }
    #[cfg(not(windows))]
    {
        OsStr::new("/dev/null")
    }
}

const LOCAL_GIT_CONFIG_BLOCKLIST: &[&str] = &[
    r"^url\..*\.insteadof$",
    r"^url\..*\.pushinsteadof$",
    r"^protocol\.allow$",
    r"^protocol\..*\.allow$",
    r"^protocol\..*\.version$",
    r"^include.*\.path$",
    r"^credential\..*$",
    r"^remote\..*\.uploadpack$",
    r"^remote\..*\.receivepack$",
    r"^remote\..*\.vcs$",
    r"^remote\..*\.proxy$",
    r"^http\.proxy$",
    r"^http\..*\.proxy$",
    r"^https\.proxy$",
    r"^https\..*\.proxy$",
    r"^core\.gitdir$",
    r"^core\.worktree$",
    r"^core\.sshcommand$",
    r"^ssh\.variant$",
    r"^core\.gitproxy$",
    r"^core\.hookspath$",
    r"^core\.fsmonitor$",
    r"^core\.fsmonitorhookpath$",
    r"^filter\..*\.process$",
    r"^filter\..*\.clean$",
    r"^filter\..*\.smudge$",
    r"^diff\..*\.external$",
];

fn validate_git_spawn_scope(
    current_dir: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let canonical_current = fs::canonicalize(current_dir).map_err(|error| error.to_string())?;
    if canonical_current.strip_prefix(&canonical_root).is_err() {
        return Err("Git current directory resolves outside daemon-owned root".into());
    }
    if current_dir.file_name() == Some(OsStr::new("source.git")) {
        validate_cache_path(root, expected_root_identity, current_dir)?;
    }
    if let Some(identity) = workspace_identity {
        validate_workspace_identity_at(current_dir, identity)?;
    }
    Ok(())
}

fn sanitize_local_git_config(
    repository: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    phase: InspectionPhase,
    cancellation: &InspectionCancellation,
) -> Result<(), InspectionError> {
    ensure_not_cancelled(
        cancellation,
        "inspection cancelled before local Git config inspection",
    )?;
    for pattern in LOCAL_GIT_CONFIG_BLOCKLIST {
        let mut query = Command::new("git");
        sanitize_git_path_environment(&mut query);
        remove_git_config_environment(&mut query);
        validate_git_spawn_scope(repository, root, expected_root_identity, workspace_identity)
            .map_err(|reason| InspectionError::new(phase, reason))?;
        query.current_dir(repository).args([
            "config",
            "--local",
            "--no-includes",
            "--get-regexp",
            pattern,
        ]);
        let output = match run_git_child(
            query,
            format!("config --local --get-regexp {pattern}"),
            Some(cancellation),
            Some(MAX_GIT_COMMAND_OUTPUT_BYTES),
            false,
        ) {
            Ok(output) => output,
            Err(GitCommandError::Cancelled) => {
                return Err(InspectionError::new(
                    InspectionPhase::Cancellation,
                    "inspection cancelled during local Git config inspection",
                ));
            }
            Err(GitCommandError::Exited { status, .. }) if matches!(status.code(), Some(1 | 5)) => {
                continue;
            }
            Err(error) => {
                return Err(InspectionError::new(
                    phase,
                    format!("inspect local Git config: {error}"),
                ));
            }
        };
        let keys = String::from_utf8_lossy(&output.bytes)
            .lines()
            .filter_map(|line| line.split_once(char::is_whitespace))
            .map(|(key, _)| key.to_owned())
            .collect::<HashSet<_>>();
        for key in keys {
            ensure_not_cancelled(
                cancellation,
                "inspection cancelled before local Git config cleanup",
            )?;
            let mut unset = Command::new("git");
            sanitize_git_path_environment(&mut unset);
            remove_git_config_environment(&mut unset);
            validate_git_spawn_scope(repository, root, expected_root_identity, workspace_identity)
                .map_err(|reason| InspectionError::new(phase, reason))?;
            unset.current_dir(repository).args([
                "config",
                "--local",
                "--no-includes",
                "--unset-all",
                &key,
            ]);
            match run_git_child(
                unset,
                format!("config --local --unset-all {key}"),
                Some(cancellation),
                Some(MAX_GIT_COMMAND_OUTPUT_BYTES),
                false,
            ) {
                Ok(_) => {}
                Err(GitCommandError::Cancelled) => {
                    return Err(InspectionError::new(
                        InspectionPhase::Cancellation,
                        "inspection cancelled during local Git config cleanup",
                    ));
                }
                Err(error) => {
                    return Err(InspectionError::new(
                        phase,
                        format!("remove local Git config {key}: {error}"),
                    ));
                }
            }
        }
    }
    ensure_not_cancelled(
        cancellation,
        "inspection cancelled after local Git config cleanup",
    )
}

#[derive(Debug)]
enum GitCommandError {
    Cancelled,
    TimedOut {
        operation: String,
    },
    OutputExceeded {
        operation: String,
        max: usize,
    },
    Exited {
        operation: String,
        status: ExitStatus,
    },
    Failed(String),
}

impl fmt::Display for GitCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Git operation cancelled"),
            Self::TimedOut { operation } => {
                write!(formatter, "git {operation} timed out")
            }
            Self::OutputExceeded { operation, max } => write!(
                formatter,
                "git {operation} output exceeded bounded size of {max} bytes"
            ),
            Self::Exited { operation, status } => {
                write!(formatter, "git {operation} failed with {status}")
            }
            Self::Failed(reason) => formatter.write_str(reason),
        }
    }
}

fn inspection_error_from_git(phase: InspectionPhase, error: GitCommandError) -> InspectionError {
    match error {
        GitCommandError::Cancelled => InspectionError::new(
            InspectionPhase::Cancellation,
            "inspection cancelled during Git operation",
        ),
        other => InspectionError::new(phase, other.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn git_output_bounded_with_cancellation<I, S>(
    current_dir: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    additional_root: Option<(&Path, FsIdentity)>,
    args: I,
    max_output_bytes: usize,
    cancellation: Option<&InspectionCancellation>,
) -> Result<BoundedGitOutput, GitCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect::<Vec<_>>();
    git_output_managed(
        current_dir,
        root,
        expected_root_identity,
        workspace_identity,
        additional_root,
        args,
        cancellation,
        Some(max_output_bytes),
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn git_output_for_phase<I, S>(
    current_dir: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    additional_root: Option<(&Path, FsIdentity)>,
    args: I,
    phase: InspectionPhase,
    cancellation: Option<&InspectionCancellation>,
) -> Result<String, InspectionError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let output = git_output_managed(
        current_dir,
        root,
        expected_root_identity,
        workspace_identity,
        additional_root,
        args.into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect(),
        cancellation,
        Some(MAX_GIT_COMMAND_OUTPUT_BYTES),
        false,
    )
    .map_err(|error| inspection_error_from_git(phase, error))?;
    Ok(String::from_utf8_lossy(&output.bytes).into_owned())
}

fn git_output<I, S>(
    current_dir: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    additional_root: Option<(&Path, FsIdentity)>,
    args: I,
) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let output = git_output_managed(
        current_dir,
        root,
        expected_root_identity,
        workspace_identity,
        additional_root,
        args.into_iter()
            .map(|argument| argument.as_ref().to_owned())
            .collect(),
        None,
        None,
        false,
    )
    .map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&output.bytes).into_owned())
}

#[allow(clippy::too_many_arguments)]
fn git_output_managed(
    current_dir: &Path,
    root: &Path,
    expected_root_identity: FsIdentity,
    workspace_identity: Option<&WorkspaceIdentity>,
    additional_root: Option<(&Path, FsIdentity)>,
    args: Vec<String>,
    cancellation: Option<&InspectionCancellation>,
    max_output_bytes: Option<usize>,
    allow_truncation: bool,
) -> Result<BoundedGitOutput, GitCommandError> {
    validate_git_spawn_scope(
        current_dir,
        root,
        expected_root_identity,
        workspace_identity,
    )
    .map_err(GitCommandError::Failed)?;
    if let Some((additional_root, identity)) = additional_root {
        validate_root_identity(additional_root, identity).map_err(GitCommandError::Failed)?;
    }
    let operation = args.first().cloned().unwrap_or_else(|| "command".into());
    let host_config_overrides = git_host_config_overrides(cancellation)?;
    let mut command = Command::new("git");
    // Keep host SSH agents, SSH commands, and helpers. Isolate global/system
    // Git config so URL rewrites cannot redirect a source, then restore only
    // authentication settings through environment variables.
    sanitize_git_environment(&mut command);
    apply_git_config_overrides(&mut command, &host_config_overrides);
    command.current_dir(current_dir).args(&args);
    validate_git_spawn_scope(
        current_dir,
        root,
        expected_root_identity,
        workspace_identity,
    )
    .map_err(GitCommandError::Failed)?;
    if let Some((additional_root, identity)) = additional_root {
        validate_root_identity(additional_root, identity).map_err(GitCommandError::Failed)?;
    }
    run_git_child(
        command,
        operation,
        cancellation,
        max_output_bytes,
        allow_truncation,
    )
}

fn run_git_child(
    mut command: Command,
    operation: String,
    cancellation: Option<&InspectionCancellation>,
    max_output_bytes: Option<usize>,
    allow_truncation: bool,
) -> Result<BoundedGitOutput, GitCommandError> {
    if cancellation.is_some_and(InspectionCancellation::is_cancelled) {
        return Err(GitCommandError::Cancelled);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| GitCommandError::Failed(format!("run git {operation}: {error}")))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            stop_git_process(&mut child);
            return Err(GitCommandError::Failed(format!(
                "run git {operation}: stdout pipe was unavailable"
            )));
        }
    };
    let output_overflow = Arc::new(AtomicBool::new(false));
    let reader_overflow = output_overflow.clone();
    let reader = thread::spawn(move || read_git_output(stdout, max_output_bytes, reader_overflow));
    let deadline = Instant::now() + GIT_OPERATION_TIMEOUT;
    let wait_result: Result<Option<ExitStatus>, GitCommandError> = loop {
        if cancellation.is_some_and(InspectionCancellation::is_cancelled) {
            stop_git_process(&mut child);
            break Err(GitCommandError::Cancelled);
        }
        if output_overflow.load(Ordering::Acquire) {
            stop_git_process(&mut child);
            if allow_truncation {
                break Ok(None);
            }
            break Err(GitCommandError::OutputExceeded {
                operation: operation.clone(),
                max: max_output_bytes.unwrap_or_default(),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(Some(status)),
            Ok(None) if Instant::now() >= deadline => {
                stop_git_process(&mut child);
                break Err(GitCommandError::TimedOut {
                    operation: operation.clone(),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                stop_git_process(&mut child);
                break Err(GitCommandError::Failed(format!(
                    "wait for git {operation}: {error}"
                )));
            }
        }
    };
    let mut output = match reader.join() {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            if wait_result.is_ok() {
                stop_git_process(&mut child);
            }
            return Err(GitCommandError::Failed(format!(
                "read git {operation} output: {error}"
            )));
        }
        Err(_) => {
            if wait_result.is_ok() {
                stop_git_process(&mut child);
            }
            return Err(GitCommandError::Failed(format!(
                "read git {operation} panicked"
            )));
        }
    };
    let truncated = output_overflow.load(Ordering::Acquire);
    let status = wait_result?;
    if cancellation.is_some_and(InspectionCancellation::is_cancelled) {
        return Err(GitCommandError::Cancelled);
    }
    if truncated && !allow_truncation {
        return Err(GitCommandError::OutputExceeded {
            operation,
            max: max_output_bytes.unwrap_or_default(),
        });
    }
    if truncated {
        if let Some(max) = max_output_bytes {
            output.truncate(max);
        }
    }
    if let Some(status) = status {
        if !status.success() {
            return Err(GitCommandError::Exited { operation, status });
        }
    }
    Ok(BoundedGitOutput {
        bytes: output,
        truncated,
    })
}

fn stop_git_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_git_output<R: Read>(
    mut reader: R,
    max_output_bytes: Option<usize>,
    output_overflow: Arc<AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    match max_output_bytes {
        Some(max) => {
            reader
                .take(max.saturating_add(1) as u64)
                .read_to_end(&mut output)?;
        }
        None => {
            reader.read_to_end(&mut output)?;
        }
    }
    if max_output_bytes.is_some_and(|max| output.len() > max) {
        output_overflow.store(true, Ordering::Release);
    }
    Ok(output)
}

fn is_cache_staging_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with(CACHE_STAGING_PREFIX))
}

fn is_cache_namespace_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(encoded) = name.strip_prefix("id-") else {
        return false;
    };
    !encoded.is_empty()
        && encoded.len().is_multiple_of(2)
        && encoded.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn cleanup_abandoned_cache_staging_path(
    cache_root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    expected_namespace_identity: FsIdentity,
    expected_staging_identity: FsIdentity,
    path: &Path,
) -> Result<(), String> {
    validate_root_identity(cache_root, expected_root_identity)?;
    let current_namespace_identity =
        validate_cache_namespace(cache_root, expected_root_identity, repository_root)?;
    if current_namespace_identity != expected_namespace_identity {
        return Err("repository cache namespace identity changed".into());
    }
    let staging_identity = match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_cache_staging_path(cache_root, expected_root_identity, repository_root, path)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let expected_identity = CacheStagingIdentity {
        namespace: expected_namespace_identity,
        staging: expected_staging_identity,
    };
    if staging_identity != expected_identity {
        return Err("temporary repository cache identity changed".into());
    }
    remove_owned_cache_staging_path(
        cache_root,
        expected_root_identity,
        repository_root,
        path,
        expected_identity,
    )
}

fn cleanup_stale_cache_staging(root: &Path, expected_root_identity: FsIdentity) -> CleanupReport {
    let mut report = CleanupReport::default();
    if let Err(reason) = validate_root_identity(root, expected_root_identity) {
        report.failures.push(CleanupFailure {
            path: root.to_owned(),
            reason,
        });
        return report;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            report.failures.push(CleanupFailure {
                path: root.to_owned(),
                reason: error.to_string(),
            });
            return report;
        }
    };
    for entry in entries {
        if let Err(reason) = validate_root_identity(root, expected_root_identity) {
            report.failures.push(CleanupFailure {
                path: root.to_owned(),
                reason,
            });
            return report;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push(CleanupFailure {
                    path: root.to_owned(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let namespace = entry.path();
        let name = namespace.file_name();
        if !name.is_some_and(is_cache_namespace_name) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&namespace) {
            Ok(metadata) => metadata,
            Err(error) => {
                report.failures.push(CleanupFailure {
                    path: namespace,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            report.failures.push(CleanupFailure {
                path: namespace,
                reason: "repository cache namespace is not a real directory".into(),
            });
            continue;
        }
        let observed_namespace_identity = metadata_identity(&metadata);
        let namespace_identity =
            match validate_cache_namespace(root, expected_root_identity, &namespace) {
                Ok(identity) if identity == observed_namespace_identity => identity,
                Ok(_) => {
                    report.failures.push(CleanupFailure {
                        path: namespace,
                        reason: "repository cache namespace identity changed".into(),
                    });
                    continue;
                }
                Err(reason) => {
                    report.failures.push(CleanupFailure {
                        path: namespace,
                        reason,
                    });
                    continue;
                }
            };
        visit_cache_namespace(
            root,
            expected_root_identity,
            &namespace,
            namespace_identity,
            &mut report,
        );
    }
    report
}

fn visit_cache_namespace(
    root: &Path,
    expected_root_identity: FsIdentity,
    namespace: &Path,
    expected_namespace_identity: FsIdentity,
    report: &mut CleanupReport,
) {
    match validate_cache_namespace(root, expected_root_identity, namespace) {
        Ok(identity) if identity == expected_namespace_identity => {}
        Ok(_) => {
            report.failures.push(CleanupFailure {
                path: namespace.to_owned(),
                reason: "repository cache namespace identity changed".into(),
            });
            return;
        }
        Err(reason) => {
            report.failures.push(CleanupFailure {
                path: namespace.to_owned(),
                reason,
            });
            return;
        }
    }
    let entries = match fs::read_dir(namespace) {
        Ok(entries) => entries,
        Err(error) => {
            report.failures.push(CleanupFailure {
                path: namespace.to_owned(),
                reason: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        if let Err(reason) = validate_root_identity(root, expected_root_identity) {
            report.failures.push(CleanupFailure {
                path: root.to_owned(),
                reason,
            });
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push(CleanupFailure {
                    path: namespace.to_owned(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if !is_cache_staging_name(name) {
            continue;
        }
        let staging_identity =
            match validate_cache_staging_path(root, expected_root_identity, namespace, &path) {
                Ok(identity) => identity,
                Err(reason) => {
                    report.failures.push(CleanupFailure { path, reason });
                    continue;
                }
            };
        match remove_owned_cache_staging_path(
            root,
            expected_root_identity,
            namespace,
            &path,
            CacheStagingIdentity {
                namespace: expected_namespace_identity,
                staging: staging_identity.staging,
            },
        ) {
            Ok(()) => report.removed.push(path),
            Err(reason) => report.failures.push(CleanupFailure { path, reason }),
        }
    }
}

fn cleanup_stale_workspaces(root: &Path, expected_root_identity: FsIdentity) -> CleanupReport {
    let mut report = CleanupReport::default();
    if let Err(reason) = validate_root_identity(root, expected_root_identity) {
        report.failures.push(CleanupFailure {
            path: root.to_owned(),
            reason,
        });
        return report;
    }
    visit_workspace_root(root, expected_root_identity, root, &mut report);
    report
}

fn visit_workspace_root(
    root: &Path,
    expected_root_identity: FsIdentity,
    directory: &Path,
    report: &mut CleanupReport,
) {
    if let Err(reason) = validate_root_identity(root, expected_root_identity) {
        report.failures.push(CleanupFailure {
            path: root.to_owned(),
            reason,
        });
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            report.failures.push(CleanupFailure {
                path: directory.to_owned(),
                reason: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.failures.push(CleanupFailure {
                    path: directory.to_owned(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                report.failures.push(CleanupFailure {
                    path,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(WORKSPACE_PREFIX))
        {
            let canonical = match fs::canonicalize(&path) {
                Ok(canonical) => canonical,
                Err(error) => {
                    report.failures.push(CleanupFailure {
                        path,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if canonical.parent() != Some(directory) {
                report.failures.push(CleanupFailure {
                    path,
                    reason: "workspace resolves outside dedicated root".into(),
                });
                continue;
            }
            let identity = match capture_workspace_identity(&path) {
                Ok(identity) => identity,
                Err(reason) => {
                    report.failures.push(CleanupFailure { path, reason });
                    continue;
                }
            };
            match remove_owned_workspace_path(root, &path, expected_root_identity, &identity) {
                Ok(()) => report.removed.push(path),
                Err(error) => report.failures.push(CleanupFailure {
                    path,
                    reason: error,
                }),
            }
        }
    }
}

fn validate_workspace_destination(
    root: &Path,
    expected_root_identity: FsIdentity,
    path: &Path,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    if path.parent() != Some(root)
        || !path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(WORKSPACE_PREFIX))
    {
        return Err("workspace destination is outside the dedicated root".into());
    }
    match fs::symlink_metadata(path) {
        Ok(_) => Err("workspace destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn validate_workspace_path(
    root: &Path,
    expected_root_identity: FsIdentity,
    path: &Path,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("workspace path is not an owned directory".into());
    }
    if path.parent() != Some(root) {
        return Err("workspace path is not a direct child of the dedicated root".into());
    }
    let canonical = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if canonical.parent() != Some(root)
        || !canonical
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(WORKSPACE_PREFIX))
    {
        return Err("workspace path resolves outside the dedicated root".into());
    }
    Ok(())
}

fn remove_owned_workspace_path(
    root: &Path,
    path: &Path,
    expected_root_identity: FsIdentity,
    expected_identity: &WorkspaceIdentity,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    validate_workspace_path(root, expected_root_identity, path)?;
    validate_workspace_identity_at(path, expected_identity)?;
    remove_owned_child_path(
        root,
        path,
        expected_root_identity,
        expected_identity.workspace,
        WORKSPACE_PREFIX,
    )
}

fn remove_owned_cache_path(
    cache_root: &Path,
    expected_root_identity: FsIdentity,
    repository_root: &Path,
    path: &Path,
    expected_identity: CacheStagingIdentity,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    let current_identity =
        validate_cache_staging_path(cache_root, expected_root_identity, repository_root, path)?;
    if current_identity != expected_identity {
        return Err("temporary repository cache identity changed".into());
    }

    // Rename into a fresh, still-recognized child first. This makes ownership
    // transfer atomic; recursive deletion never targets caller-visible staging.
    let quarantine = repository_root.join(format!(
        "{CACHE_STAGING_PREFIX}cleanup-{}-{}",
        std::process::id(),
        next_cache_cleanup_sequence()
    ));
    ensure_path_absent(&quarantine)?;
    fs::rename(path, &quarantine).map_err(|error| {
        format!(
            "move temporary repository cache {} into cleanup quarantine: {error}",
            path.display()
        )
    })?;

    let quarantined_identity = validate_cache_staging_path(
        cache_root,
        expected_root_identity,
        repository_root,
        &quarantine,
    )?;
    if quarantined_identity != expected_identity {
        return Err(format!(
            "temporary repository cache quarantine {} identity changed during cleanup",
            quarantine.display()
        ));
    }
    remove_owned_child_path(
        repository_root,
        &quarantine,
        expected_identity.namespace,
        expected_identity.staging,
        CACHE_STAGING_PREFIX,
    )
    .map_err(|reason| format!("cleanup quarantine {}: {reason}", quarantine.display()))
}

fn remove_owned_child_path(
    root: &Path,
    path: &Path,
    expected_root_identity: FsIdentity,
    expected_identity: FsIdentity,
    prefix: &str,
) -> Result<(), String> {
    validate_root_identity(root, expected_root_identity)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if path.parent() != Some(root)
        || !path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(prefix))
    {
        return Err("refusing cleanup outside daemon-owned direct child".into());
    }
    if metadata_identity(&metadata) != expected_identity {
        return Err("owned cleanup child identity changed".into());
    }
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("owned cleanup child is not a real directory".into());
    }
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let canonical = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if canonical.parent() != Some(canonical_root.as_path()) {
        return Err("refusing cleanup outside daemon-owned root".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = fs::metadata(root)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode();
        if mode & 0o200 == 0 {
            return Err("daemon-owned root is not writable for cleanup".into());
        }
    }
    validate_root_identity(root, expected_root_identity)?;
    let final_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if final_metadata.file_type().is_symlink()
        || !final_metadata.is_dir()
        || metadata_identity(&final_metadata) != expected_identity
    {
        return Err("owned cleanup child identity changed before removal".into());
    }
    let final_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let final_path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if final_path.parent() != Some(final_root.as_path()) {
        return Err("refusing cleanup outside daemon-owned root".into());
    }
    fs::remove_dir_all(path).map_err(|error| error.to_string())
}

/// Direct child creation is bounded to canonical daemon-owned roots; this is
/// process-level path hygiene, not a kernel sandbox.
fn reject_symlink_root(path: &Path, phase: InspectionPhase) -> Result<(), InspectionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(InspectionError::new(
            phase,
            format!("refusing symlinked configured root {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InspectionError::new(
            phase,
            format!("inspect configured root {}: {error}", path.display()),
        )),
    }
}

/// Direct child creation is bounded to canonical daemon-owned roots; this is
/// process-level path hygiene, not a kernel sandbox.
fn create_owned_child(
    root: &Path,
    expected_root_identity: FsIdentity,
    component: String,
    phase: InspectionPhase,
) -> Result<PathBuf, InspectionError> {
    validate_root_identity(root, expected_root_identity)
        .map_err(|reason| InspectionError::new(phase, reason))?;
    let path = root.join(component);
    loop {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(InspectionError::new(
                    phase,
                    format!("refusing symlinked directory component {}", path.display()),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(InspectionError::new(
                    phase,
                    format!("directory component {} is not a directory", path.display()),
                ));
            }
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&path) {
                    Ok(()) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(InspectionError::new(
                            phase,
                            format!("create directory {}: {error}", path.display()),
                        ));
                    }
                }
            }
            Err(error) => {
                return Err(InspectionError::new(
                    phase,
                    format!("inspect directory {}: {error}", path.display()),
                ));
            }
        }
    }
    validate_root_identity(root, expected_root_identity)
        .map_err(|reason| InspectionError::new(phase, reason))?;
    let canonical = fs::canonicalize(&path).map_err(|error| {
        InspectionError::new(
            phase,
            format!("resolve directory {}: {error}", path.display()),
        )
    })?;
    if canonical.parent() != Some(root) {
        return Err(InspectionError::new(
            phase,
            format!(
                "directory {} resolves outside configured root",
                path.display()
            ),
        ));
    }
    restrict_root_permissions(&path).map_err(|error| {
        InspectionError::new(
            phase,
            format!("restrict directory {}: {error}", path.display()),
        )
    })?;
    Ok(path)
}

#[cfg(unix)]
fn restrict_root_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn restrict_root_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn roots_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.strip_prefix(right).is_ok() || right.strip_prefix(left).is_ok()
}

fn path_component(value: &str) -> String {
    // Hex-encode every value so encoded and already-safe identities cannot
    // collide, and no component can introduce a path separator.
    let mut encoded = String::from("id-");
    for byte in value.as_bytes() {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn next_workspace_sequence() -> u64 {
    WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

fn next_cache_cleanup_sequence() -> u64 {
    CACHE_CLEANUP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

fn non_empty_reason(reason: String, fallback: &str) -> String {
    if reason.trim().is_empty() {
        fallback.to_owned()
    } else {
        reason
    }
}

/// Public helper for callers that receive a server-assembled context.
pub fn source_from_context(
    context: &RepositoryContext,
) -> Result<RepositorySource, InspectionError> {
    RepositorySource::from_context(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_identity_components_cannot_escape_root() {
        assert_ne!(path_component("repo-1"), "repo-1");
        assert_ne!(path_component("../cache"), path_component("id-2e2e"));
        assert_ne!(path_component("."), path_component(".."));
    }

    #[test]
    fn read_git_allowlist_rejects_mutation_and_output_flags() {
        assert!(read_git_allowed(&["status".into(), "--porcelain".into()]));
        assert!(read_git_allowed(&["rev-parse".into(), "HEAD".into()]));
        assert!(!read_git_allowed(&["push".into()]));
        assert!(!read_git_allowed(&[
            "diff".into(),
            "--output=secret".into()
        ]));
        assert!(!read_git_allowed(&[
            "status".into(),
            "--git-dir=../cache".into()
        ]));
        assert!(!read_git_allowed(&[
            "status".into(),
            "--work-tree=../cache".into()
        ]));
        assert!(!read_git_allowed(&[
            "status".into(),
            "-C".into(),
            "../cache".into()
        ]));
        assert!(!read_git_allowed(&[
            "status".into(),
            "--super-prefix=../cache".into()
        ]));
        assert!(!read_git_allowed(&[
            "diff".into(),
            "-o".into(),
            "../cache".into()
        ]));
        assert!(!read_git_allowed(&["status".into(), "-cfoo=bar".into()]));
        assert!(!read_git_allowed(&[
            "status".into(),
            "--config-env=foo=BAR".into()
        ]));
    }

    #[test]
    fn protocol_context_has_no_credential_or_workspace_fields() {
        let context = RepositoryContext {
            repository_id: "repo".into(),
            name: "Repository".into(),
            url: "https://example.test/repo.git".into(),
            description: "Read-only source".into(),
        };
        let json = match serde_json::to_string(&context) {
            Ok(json) => json,
            Err(error) => panic!("encode repository context: {error}"),
        };
        for field in [
            "credential",
            "password",
            "token",
            "checkout_path",
            "cache_path",
        ] {
            assert!(!json.contains(field), "wire context contains {field}");
        }
        assert!(
            source_from_context(&context).is_ok(),
            "credential-free context"
        );
        let local = RepositoryContext {
            url: "file:///tmp/repo.git".into(),
            ..context
        };
        assert!(source_from_context(&local).is_err());
    }

    #[test]
    fn repository_selection_rejects_unknown_identity() {
        let request = InspectionRequest::new(
            "session-1",
            "task-1",
            RepositorySource::new("unknown", "https://example.test/repo.git"),
        );
        let authorization = match RunAuthorization::new(
            "session-1",
            vec![RepositorySource::new(
                "known",
                "https://example.test/repo.git",
            )],
        ) {
            Ok(authorization) => authorization,
            Err(error) => panic!("authorization: {error}"),
        };
        let error = match validate_repository_selection(&request, &authorization) {
            Ok(()) => panic!("unknown repository must not be authorized"),
            Err(error) => error,
        };
        assert_eq!(error.phase, InspectionPhase::Authorization);
    }

    #[test]
    fn authorization_binds_session_and_source_url() {
        let request = InspectionRequest::new(
            "session-1",
            "task-1",
            RepositorySource::new("repo", "https://example.test/one.git"),
        );
        let different_url = match RunAuthorization::new(
            "session-1",
            vec![RepositorySource::new(
                "repo",
                "https://example.test/two.git",
            )],
        ) {
            Ok(authorization) => authorization,
            Err(error) => panic!("authorization: {error}"),
        };
        let url_error = match validate_repository_selection(&request, &different_url) {
            Ok(()) => panic!("source URL mismatch"),
            Err(error) => error,
        };
        assert_eq!(
            url_error,
            InspectionError {
                phase: InspectionPhase::Authorization,
                reason: "repository URL does not match run authorization".into(),
                contamination: None,
                cleanup_failure: None,
            }
        );
        let different_session = match RunAuthorization::new(
            "session-2",
            vec![RepositorySource::new(
                "repo",
                "https://example.test/one.git",
            )],
        ) {
            Ok(authorization) => authorization,
            Err(error) => panic!("authorization: {error}"),
        };
        let session_error = match validate_repository_selection(&request, &different_session) {
            Ok(()) => panic!("session mismatch"),
            Err(error) => error,
        };
        assert_eq!(session_error.phase, InspectionPhase::Authorization);
    }

    #[test]
    fn cache_cleanup_retains_replaced_identity() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|error| panic!("canonical temporary root: {error}"))
            .join(format!(
                "north-cache-identity-{}-{}",
                std::process::id(),
                next_cache_cleanup_sequence()
            ));
        let namespace = root.join(path_component("repo-1"));
        let staging = namespace.join(".source-original");
        fs::create_dir_all(&staging).unwrap();
        let root_identity = directory_identity(&root).unwrap();
        let expected =
            validate_cache_staging_path(&root, root_identity, &namespace, &staging).unwrap();

        let displaced_staging = namespace.join(".source-displaced");
        fs::rename(&staging, &displaced_staging).unwrap();
        fs::create_dir(&staging).unwrap();
        let error =
            remove_owned_cache_staging_path(&root, root_identity, &namespace, &staging, expected)
                .expect_err("replaced staging must be retained");
        assert!(error.contains("identity changed"));
        assert!(staging.is_dir());
        assert!(displaced_staging.is_dir());

        fs::remove_dir_all(&staging).unwrap();
        fs::rename(&displaced_staging, &staging).unwrap();
        let displaced_namespace = root.join("namespace-displaced");
        fs::rename(&namespace, &displaced_namespace).unwrap();
        fs::create_dir_all(&staging).unwrap();
        let error =
            remove_owned_cache_staging_path(&root, root_identity, &namespace, &staging, expected)
                .expect_err("replaced namespace must be retained");
        assert!(error.contains("repository cache namespace") || error.contains("identity changed"));
        assert!(staging.is_dir());
        assert!(displaced_namespace.join(".source-original").is_dir());

        fs::remove_dir_all(&namespace).unwrap();
        fs::rename(displaced_namespace, namespace).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credential_bearing_locations_are_rejected() {
        for url in [
            "https://token@example.test/repo.git",
            "ssh://git:password@example.test/repo.git",
            "deploy@example.test:org/repo.git",
            "https://example.test/repo.git?token=secret",
        ] {
            let source = RepositorySource::new("repo", url);
            assert!(source.validate().is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn public_contracts_and_failure_states_are_exercised() {
        for (phase, name) in [
            (InspectionPhase::Authorization, "authorization"),
            (InspectionPhase::Cache, "cache"),
            (InspectionPhase::Workspace, "workspace"),
            (InspectionPhase::Revision, "revision"),
            (InspectionPhase::Runtime, "runtime"),
            (InspectionPhase::Cancellation, "cancellation"),
            (InspectionPhase::DirtyTree, "dirty-tree"),
            (InspectionPhase::Cleanup, "cleanup"),
        ] {
            assert_eq!(phase.to_string(), name);
        }

        let mut error = InspectionError::new(InspectionPhase::Cache, "cache failed");
        assert_eq!(
            error.to_string(),
            "repository inspection cache: cache failed"
        );
        assert!(!error.is_contaminated());
        assert!(!error.cleanup_failed());
        error.contamination = Some("dirty files".into());
        error.cleanup_failure = Some("staging retained".into());
        assert!(error.is_contaminated());
        assert!(error.cleanup_failed());
        assert_eq!(
            error.to_string(),
            "repository inspection cache: cache failed; contamination: dirty files; cleanup failed: staging retained"
        );
        assert!(InspectionError::new(InspectionPhase::DirtyTree, "dirty").is_contaminated());
        assert!(InspectionError::new(InspectionPhase::Cleanup, "cleanup").cleanup_failed());

        let mut report = CleanupReport::default();
        assert!(report.is_clean());
        report.failures.push(CleanupFailure {
            path: PathBuf::from("retained"),
            reason: "unsafe".into(),
        });
        assert!(!report.is_clean());

        let cancellation = InspectionCancellation::new();
        assert!(!cancellation.is_cancelled());
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn authorization_and_context_boundaries_are_exercised() {
        let source = RepositorySource::new("repo", "https://example.test/repo.git");
        assert!(source.validate().is_ok());
        for invalid in [
            RepositorySource::new(" ", source.url.clone()),
            RepositorySource::new("repo", " "),
            RepositorySource::new("repo", "https://example.test/repo .git"),
            RepositorySource::new("repo", "https://user@example.test/repo.git"),
        ] {
            assert!(invalid.validate().is_err());
        }

        let context = RepositoryContext {
            repository_id: source.repository_id.clone(),
            name: "Repository".into(),
            url: source.url.clone(),
            description: "Read-only source".into(),
        };
        let start = north_protocol::SessionStart {
            requirement: north_protocol::RequirementContext {
                id: "requirement".into(),
                revision: 1,
                title: "Title".into(),
                description: "Description".into(),
                summary: "Summary".into(),
                acceptance_criteria: vec![],
                assumptions: vec![],
                open_questions: vec![],
            },
            conversation: north_protocol::ConversationContext { excerpt: vec![] },
            repositories: vec![context.clone()],
        };
        let authorization =
            RunAuthorization::from_session_start("session", &start).expect("session authorization");
        assert_eq!(authorization.session_id(), "session");
        assert_eq!(authorization.repositories(), std::slice::from_ref(&source));
        assert!(RunAuthorization::new(" ", vec![]).is_err());
        assert!(RunAuthorization::new("session", vec![source.clone(), source.clone()]).is_err());

        let invalid_context = RepositoryContext {
            url: "file:///tmp/repository.git".into(),
            ..context
        };
        assert!(RepositorySource::from_context(&invalid_context).is_err());

        assert!(InspectionRequest::new("", "task", source.clone())
            .validate()
            .is_err());
        assert!(InspectionRequest::new("session", " ", source)
            .validate()
            .is_err());
        assert!(InspectionRequest::new(
            "session",
            "task",
            RepositorySource::new("repo", "https://example.test/repo.git",)
        )
        .validate()
        .is_ok());
    }

    #[test]
    fn cleanup_gate_rejects_active_and_duplicate_cleanup() {
        let gate = Arc::new(CleanupGate::default());
        let operation = gate.enter_operation();
        assert!(gate.begin_cleanup().is_err());
        drop(operation);
        let cleanup = gate.begin_cleanup().expect("cleanup permit");
        assert!(gate.begin_cleanup().is_err());
        drop(cleanup);
        let operation = gate.enter_operation();
        drop(operation);
    }

    #[test]
    fn prepared_workspace_contracts_and_result_wire_are_exercised() {
        let gate = Arc::new(CleanupGate::default());
        let prepared = PreparedWorkspace {
            session_id: "session".into(),
            repository_id: "repo".into(),
            repository_url: "https://example.test/repo.git".into(),
            commit_sha: "abcdef".into(),
            workspace_root: PathBuf::from("workspace-root"),
            workspace_root_identity: FsIdentity {
                first: 1,
                second: 2,
            },
            workspace_identity: WorkspaceIdentity {
                name: "workspace".into(),
                workspace: FsIdentity {
                    first: 3,
                    second: 4,
                },
                git_directory: FsIdentity {
                    first: 5,
                    second: 6,
                },
                marker: FsIdentity {
                    first: 7,
                    second: 8,
                },
            },
            git_config: Vec::new(),
            path: PathBuf::from("workspace-root/session/task/repo"),
            _cleanup_permit: OperationPermit {
                _lease: Arc::new(OperationLease { gate }),
            },
        };
        assert_eq!(prepared.repository_id(), "repo");
        assert_eq!(prepared.commit_sha(), "abcdef");
        assert_eq!(
            prepared.path(),
            Path::new("workspace-root/session/task/repo")
        );
        assert_eq!(prepared, prepared.clone());
        let mut changed = prepared.clone();
        changed.path.push("changed");
        assert_ne!(prepared, changed);

        let authorization = RunAuthorization::new(
            "session",
            vec![RepositorySource::new(
                "repo",
                "https://example.test/repo.git",
            )],
        )
        .expect("authorization");
        let error = prepared
            .read_git(&authorization, &["status".into()])
            .expect_err("missing workspace must be rejected");
        assert_eq!(error.phase, InspectionPhase::Workspace);

        let result = InspectionResult {
            repository_id: "repo".into(),
            commit_sha: "abcdef".into(),
        };
        assert_eq!(
            result.reviewed_repository(),
            north_protocol::ReviewedRepositoryWire {
                repository_id: "repo".into(),
                commit_sha: "abcdef".into(),
            }
        );
    }

    #[test]
    fn inspector_roots_and_name_validators_cover_boundaries() {
        assert!(roots_overlap(
            Path::new("/tmp/root"),
            Path::new("/tmp/root")
        ));
        assert!(roots_overlap(
            Path::new("/tmp/root"),
            Path::new("/tmp/root/nested")
        ));
        assert!(!roots_overlap(
            Path::new("/tmp/root"),
            Path::new("/tmp/other")
        ));

        let root = std::env::temp_dir().join(format!(
            "north-overlap-{}-{}",
            std::process::id(),
            next_cache_cleanup_sequence()
        ));
        let nested = root.join("nested");
        let error = RepositoryInspector::new(&root, &nested).expect_err("overlapping roots");
        assert_eq!(error.phase, InspectionPhase::Workspace);
        fs::remove_dir_all(root).expect("remove overlap roots");

        assert!(is_cache_staging_name(OsStr::new(".source-1")));
        assert!(!is_cache_staging_name(OsStr::new("source.git")));
        assert!(is_cache_namespace_name(OsStr::new("id-6162")));
        assert!(!is_cache_namespace_name(OsStr::new("id-")));
        assert!(!is_cache_namespace_name(OsStr::new("id-6")));
        assert!(!is_cache_namespace_name(OsStr::new("id-gg")));
        assert!(!is_cache_namespace_name(OsStr::new("repo")));
        assert_eq!(non_empty_reason(" ".into(), "fallback"), "fallback");
        assert_eq!(non_empty_reason("specific".into(), "fallback"), "specific");

        assert!(server_repository_location("https://example.test/repo.git"));
        assert!(server_repository_location(
            "ssh://git@example.test/repo.git"
        ));
        assert!(server_repository_location("git@example.test:repo.git"));
        assert!(!server_repository_location("file:///tmp/repo.git"));
        assert!(!server_repository_location("https:///repo.git"));
        assert!(!server_repository_location("ssh:///repo.git"));
        assert!(!server_repository_location("git@/etc"));
        assert!(!server_repository_location("git@example.test"));
        assert!(credential_free_location("https://example.test/repo.git"));
        assert!(credential_free_location("ssh://git@example.test/repo.git"));
        assert!(credential_free_location("git@example.test:repo.git"));
        assert!(!credential_free_location(
            "https://example.test/repo.git?token=x"
        ));
        assert!(!credential_free_location("https://example.test"));
        assert!(!credential_free_location(
            "ssh://user@example.test/repo.git"
        ));
        assert!(!credential_free_location(
            "file://user@example.test/repo.git"
        ));
        assert!(complete_commit_sha(&"a".repeat(40)));
        assert!(complete_commit_sha(&"A".repeat(64)));
        assert!(!complete_commit_sha(&"a".repeat(39)));
        assert!(!complete_commit_sha(&format!("{}g", "a".repeat(39))));
    }
}
