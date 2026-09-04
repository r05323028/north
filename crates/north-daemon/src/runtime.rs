//! North-owned clarification runtime seam and Pi Agent adapter.
//!
//! The seam carries only immutable North context and North-neutral facts. Pi
//! process/session details stay in `PiClarificationAdapter`; this module never
//! writes Requirement state or server persistence.

use crate::{
    journal::RuntimeControl,
    repository_inspection::{
        InspectionCancellation, InspectionError, InspectionPhase, InspectionRequest,
        PreparedWorkspace, RepositoryInspector, RepositorySource, RunAuthorization,
    },
};
#[cfg(not(test))]
use north_protocol::RepositoryContext;
use north_protocol::{
    AgentActivity, AgentMessage, Command, Event, MessageSend, ReadinessVerdictWire,
    RequirementAssessed, ReviewedRepositoryWire, SessionCompleted, SessionFailed, SessionStarted,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fmt, fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_PI_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SESSION_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_EVIDENCE_LIST_BYTES: usize = 64 * 1024;
const MAX_EVIDENCE_BYTES: usize = 32 * 1024;
const MAX_EVIDENCE_FILE_BYTES: usize = 8 * 1024;
const MAX_EVIDENCE_FILES: usize = 32;
const MAX_TOTAL_EVIDENCE_BYTES: usize = 64 * 1024;
const MAX_TOTAL_EVIDENCE_FILES: usize = 64;
const PI_PROCESS_TIMEOUT: Duration = Duration::from_secs(120);
const PI_SYSTEM_PROMPT: &str = "You are North's requirement clarification agent. Use only context in this prompt. Do not claim repository facts not supplied. Return only one JSON object with keys message, verdict, blockers, assumptions. verdict must be ready or needs_clarification. Keep message concise and user-facing. Do not include reasoning, tool output, or markdown.";

#[cfg(test)]
type PreparationCheckpoint = Arc<dyn Fn(&str, &std::path::Path) + Send + Sync>;

/// Provider-neutral requirement snapshot copied from one server command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementSnapshot {
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnRole {
    Requester,
    Agent,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub message_id: String,
    pub role: TurnRole,
    pub content: String,
}

/// Repository metadata and authorization copied from one `session.start`.
/// No checkout path or credential crosses this seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedRepository {
    pub repository_id: String,
    pub name: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryReview {
    pub repository_id: String,
    pub commit_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommand {
    Start {
        requirement: RequirementSnapshot,
        conversation: Vec<ConversationTurn>,
        repositories: Vec<AuthorizedRepository>,
    },
    Message {
        message_id: String,
        content: String,
    },
    Cancel {
        reason: String,
    },
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInput {
    pub operation_id: String,
    pub session_id: String,
    pub command: RuntimeCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessVerdict {
    Ready,
    NeedsClarification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFact {
    SessionStarted {
        runtime_id: String,
    },
    AgentMessage {
        message_id: String,
        content: String,
    },
    Activity {
        activity: String,
    },
    Assessed {
        requirement_id: String,
        requirement_revision: u64,
        verdict: ReadinessVerdict,
        blockers: Vec<String>,
        assumptions: Vec<String>,
        repositories_reviewed: Vec<RepositoryReview>,
    },
    Completed {
        summary: String,
    },
    Failed {
        recoverable: bool,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError(String);

impl RuntimeError {
    fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug)]
enum RuntimeOperationError {
    Cancelled,
    Failed(RuntimeError),
}

impl RuntimeOperationError {
    fn into_runtime_error(self) -> RuntimeError {
        match self {
            Self::Cancelled => RuntimeError::new("Pi clarification operation cancelled"),
            Self::Failed(error) => error,
        }
    }
}

impl fmt::Display for RuntimeOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Pi clarification operation cancelled"),
            Self::Failed(error) => error.fmt(formatter),
        }
    }
}

impl From<RuntimeError> for RuntimeOperationError {
    fn from(error: RuntimeError) -> Self {
        Self::Failed(error)
    }
}

impl From<InspectionError> for RuntimeOperationError {
    fn from(error: InspectionError) -> Self {
        Self::Failed(error.into())
    }
}

impl From<InspectionError> for RuntimeError {
    fn from(error: InspectionError) -> Self {
        Self::new(error.to_string())
    }
}

/// Small North-owned seam. It does not mirror Pi lifecycle or SDK types.
pub trait ClarificationRuntime: Send + Sync {
    fn dispatch(&self, input: RuntimeInput) -> Result<Vec<RuntimeFact>, RuntimeError>;
}

/// Pi Agent is the only concrete runtime in North 0.1.
pub struct PiClarificationAdapter {
    inspector: RepositoryInspector,
    agent_command: PathBuf,
    session_dir: PathBuf,
    sessions: Mutex<HashMap<String, AssessmentContext>>,
    pending_events: Mutex<HashMap<String, Vec<Event>>>,
    cancellation_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
    #[cfg(test)]
    preparation_checkpoint: Mutex<Option<PreparationCheckpoint>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssessmentContext {
    requirement_id: String,
    requirement_revision: u64,
    repositories_reviewed: Vec<RepositoryReview>,
    #[serde(default)]
    evidence: Vec<RepositoryEvidence>,
    context: Option<StartInput>,
    #[serde(skip)]
    in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RepositoryEvidence {
    repository_id: String,
    commit_sha: String,
    files: Vec<SourceFileEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SourceFileEvidence {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct PiAssessment {
    #[serde(default, alias = "content", alias = "answer", alias = "summary")]
    message: String,
    #[serde(default)]
    verdict: Option<String>,
    #[serde(default)]
    blockers: Vec<String>,
    #[serde(default)]
    assumptions: Vec<String>,
}

impl PiClarificationAdapter {
    pub fn new(
        inspector: RepositoryInspector,
        session_dir: impl Into<PathBuf>,
    ) -> Result<Self, RuntimeError> {
        let session_dir = session_dir.into();
        fs::create_dir_all(&session_dir)
            .map_err(|error| RuntimeError::new(format!("create Pi session directory: {error}")))?;
        let agent_command = env::var_os("NORTH_PI_AGENT_COMMAND")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pi"));
        Ok(Self {
            inspector,
            agent_command,
            session_dir,
            sessions: Mutex::new(HashMap::new()),
            pending_events: Mutex::new(HashMap::new()),
            cancellation_flags: Mutex::new(HashMap::new()),
            #[cfg(test)]
            preparation_checkpoint: Mutex::new(None),
        })
    }

    /// Test/deployment seam for selecting executable path, not provider logic.
    pub fn with_agent_command(mut self, command: impl Into<PathBuf>) -> Self {
        self.agent_command = command.into();
        self
    }

    fn context_path(&self, session_id: &str) -> PathBuf {
        let encoded = session_id
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.session_dir
            .join(format!("clarification-{encoded}.json"))
    }

    fn persist_context(
        &self,
        session_id: &str,
        context: &AssessmentContext,
    ) -> Result<(), RuntimeError> {
        let payload = serde_json::to_vec(context)
            .map_err(|error| RuntimeError::new(format!("serialize Pi session context: {error}")))?;
        if payload.len() > MAX_SESSION_CONTEXT_BYTES {
            return Err(RuntimeError::new(
                "Pi session context exceeded bounded size",
            ));
        }
        let path = self.context_path(session_id);
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, payload)
            .and_then(|_| fs::rename(&temporary, &path))
            .map_err(|error| RuntimeError::new(format!("persist Pi session context: {error}")))
    }

    fn load_context(&self, session_id: &str) -> Result<Option<AssessmentContext>, RuntimeError> {
        if let Some(context) = self
            .sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
            .get(session_id)
            .cloned()
        {
            return Ok(Some(context));
        }
        let path = self.context_path(session_id);
        let payload = match fs::read(&path) {
            Ok(payload) => payload,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(RuntimeError::new(format!(
                    "read Pi session context: {error}"
                )))
            }
        };
        if payload.len() > MAX_SESSION_CONTEXT_BYTES {
            return Err(RuntimeError::new(
                "Pi session context exceeded bounded size",
            ));
        }
        let context: AssessmentContext = serde_json::from_slice(&payload)
            .map_err(|error| RuntimeError::new(format!("parse Pi session context: {error}")))?;
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
            .insert(session_id.to_owned(), context.clone());
        Ok(Some(context))
    }

    fn clear_context(&self, session_id: &str) -> Result<(), RuntimeError> {
        match fs::remove_file(self.context_path(session_id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RuntimeError::new(format!(
                "remove Pi session context: {error}"
            ))),
        }
    }

    fn clear_session(&self, session_id: &str) -> Result<(), RuntimeError> {
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
            .remove(session_id);
        self.clear_context(session_id)
    }

    fn preparation_checkpoint(&self, repository_id: &str, path: &std::path::Path) {
        #[cfg(test)]
        {
            let hook = self
                .preparation_checkpoint
                .lock()
                .ok()
                .and_then(|hook| hook.clone());
            if let Some(hook) = hook {
                hook(repository_id, path);
            }
        }
        #[cfg(not(test))]
        {
            let _ = (repository_id, path);
        }
    }

    #[cfg(test)]
    fn set_preparation_checkpoint<F>(&self, hook: F)
    where
        F: Fn(&str, &std::path::Path) + Send + Sync + 'static,
    {
        let mut checkpoint = match self.preparation_checkpoint.lock() {
            Ok(checkpoint) => checkpoint,
            Err(_) => panic!("preparation checkpoint lock"),
        };
        *checkpoint = Some(Arc::new(hook));
    }

    fn start(
        &self,
        input: &RuntimeInput,
        start: StartInput,
        control: &RuntimeControl,
    ) -> Result<Vec<RuntimeFact>, RuntimeOperationError> {
        check_cancellation(control)?;
        let sources = start
            .repositories
            .iter()
            .map(source_for_repository)
            .collect::<Result<Vec<_>, _>>()?;
        let authorization = RunAuthorization::new(input.session_id.clone(), sources.clone())
            .map_err(RuntimeOperationError::from)?;
        let cancellation = InspectionCancellation::from_shared_flag(control.cancellation_flag());
        let mut reviewed = Vec::with_capacity(start.repositories.len());
        let mut evidence = Vec::with_capacity(start.repositories.len());
        let mut total_evidence_bytes = 0usize;
        let mut total_evidence_files = 0usize;
        for source in sources {
            check_cancellation(control)?;
            let (review, bundle) =
                self.inspect_repository(input, source, &authorization, control, &cancellation)?;
            let (bundle_bytes, bundle_files) = evidence_size(&bundle);
            total_evidence_bytes = total_evidence_bytes.saturating_add(bundle_bytes);
            total_evidence_files = total_evidence_files.saturating_add(bundle_files);
            if total_evidence_bytes > MAX_TOTAL_EVIDENCE_BYTES
                || total_evidence_files > MAX_TOTAL_EVIDENCE_FILES
            {
                return Err(RuntimeOperationError::Failed(RuntimeError::new(
                    "clarification repository evidence exceeded run bounds",
                )));
            }
            reviewed.push(review);
            evidence.push(bundle);
        }
        check_cancellation(control)?;

        let mut assessment = AssessmentContext {
            requirement_id: start.requirement.id.clone(),
            requirement_revision: start.requirement.revision,
            repositories_reviewed: reviewed.clone(),
            evidence: evidence.clone(),
            context: Some(start.clone()),
            in_flight: true,
        };
        self.persist_context(&input.session_id, &assessment)
            .map_err(RuntimeOperationError::from)?;
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeOperationError::from(RuntimeError::new("Pi session state lock poisoned"))
            })?
            .insert(input.session_id.clone(), assessment.clone());
        check_cancellation_or_cleanup(self, &input.session_id, control)?;
        let prompt = start_prompt(&start, &reviewed, &evidence);
        let response = match self.invoke_pi(&input.session_id, &prompt, control) {
            Ok(response) => response,
            Err(error) => return Err(self.cleanup_after_error(&input.session_id, error)),
        };
        if control.is_cancellation_requested() {
            return Err(
                self.cleanup_after_error(&input.session_id, RuntimeOperationError::Cancelled)
            );
        }
        let parsed = parse_assessment(&response);
        assessment.in_flight = false;
        let completes = parsed.verdict == ReadinessVerdict::Ready;
        if completes {
            self.clear_context(&input.session_id)
                .map_err(RuntimeOperationError::from)?;
        }
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeOperationError::from(RuntimeError::new("Pi session state lock poisoned"))
            })?
            .insert(input.session_id.clone(), assessment.clone());
        let facts = facts_for_response(
            &input.operation_id,
            format!("pi-{}", input.session_id),
            parsed,
            Some(assessment),
            true,
            completes,
        );
        if completes {
            self.clear_session(&input.session_id)
                .map_err(RuntimeOperationError::from)?;
        }
        Ok(facts)
    }

    fn inspect_repository(
        &self,
        input: &RuntimeInput,
        source: RepositorySource,
        authorization: &RunAuthorization,
        control: &RuntimeControl,
        cancellation: &InspectionCancellation,
    ) -> Result<(RepositoryReview, RepositoryEvidence), RuntimeOperationError> {
        check_cancellation(control)?;
        let request = InspectionRequest::new(&input.session_id, &input.operation_id, source);
        let workspace = self
            .inspector
            .prepare_authorized_with_cancellation(&request, authorization, cancellation)
            .map_err(|error| classify_inspection_error(control, error))?;
        if control.is_cancellation_requested() {
            cancellation.cancel();
        }
        let mut evidence = None;
        let inspection = self
            .inspector
            .inspect_prepared_with_cancellation(
                workspace,
                cancellation,
                |workspace, inspection_cancellation| {
                    self.preparation_checkpoint(workspace.repository_id(), workspace.path());
                    if control.is_cancellation_requested() {
                        inspection_cancellation.cancel();
                        return Err("inspection cancelled".into());
                    }
                    let bundle = extract_repository_evidence(
                        workspace,
                        authorization,
                        control,
                        inspection_cancellation,
                    )?;
                    if control.is_cancellation_requested() {
                        inspection_cancellation.cancel();
                        return Err("inspection cancelled".into());
                    }
                    evidence = Some(bundle);
                    Ok(())
                },
            )
            .map_err(|error| classify_inspection_error(control, error))?;
        let evidence = evidence.ok_or_else(|| {
            RuntimeOperationError::Failed(RuntimeError::new(
                "repository source inspection produced no evidence",
            ))
        })?;
        if evidence.repository_id != inspection.repository_id
            || evidence.commit_sha != inspection.commit_sha
        {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "repository evidence identity did not match inspected revision",
            )));
        }
        Ok((
            RepositoryReview {
                repository_id: inspection.repository_id,
                commit_sha: inspection.commit_sha,
            },
            evidence,
        ))
    }

    fn cleanup_after_error(
        &self,
        session_id: &str,
        error: RuntimeOperationError,
    ) -> RuntimeOperationError {
        match self.clear_session(session_id) {
            Ok(()) => error,
            Err(cleanup) => {
                RuntimeOperationError::Failed(RuntimeError::new(format!("{error}; {cleanup}",)))
            }
        }
    }

    fn message(
        &self,
        input: &RuntimeInput,
        message: MessageSend,
        control: &RuntimeControl,
    ) -> Result<Vec<RuntimeFact>, RuntimeOperationError> {
        check_cancellation(control)?;
        let context = self
            .load_context(&input.session_id)
            .map_err(RuntimeOperationError::from)?;
        let prompt =
            message_prompt(context.as_ref(), &message).map_err(RuntimeOperationError::from)?;
        let mut running_context = context.ok_or_else(|| {
            RuntimeOperationError::Failed(RuntimeError::new(
                "Pi clarification message has no active session context",
            ))
        })?;
        if running_context.in_flight {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "Pi clarification session is already running",
            )));
        }
        running_context.in_flight = true;
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeOperationError::from(RuntimeError::new("Pi session state lock poisoned"))
            })?
            .insert(input.session_id.clone(), running_context.clone());
        check_cancellation_or_cleanup(self, &input.session_id, control)?;
        let response = match self.invoke_pi(&input.session_id, &prompt, control) {
            Ok(response) => response,
            Err(error) => return Err(self.cleanup_after_error(&input.session_id, error)),
        };
        if control.is_cancellation_requested() {
            return Err(
                self.cleanup_after_error(&input.session_id, RuntimeOperationError::Cancelled)
            );
        }
        running_context.in_flight = false;
        self.sessions
            .lock()
            .map_err(|_| {
                RuntimeOperationError::from(RuntimeError::new("Pi session state lock poisoned"))
            })?
            .insert(input.session_id.clone(), running_context.clone());
        let parsed = parse_assessment(&response);
        let completes = parsed.verdict == ReadinessVerdict::Ready;
        let facts = facts_for_response(
            &input.operation_id,
            format!("pi-{}", input.session_id),
            parsed,
            Some(running_context),
            false,
            completes,
        );
        if completes {
            self.clear_session(&input.session_id)
                .map_err(RuntimeOperationError::from)?;
        }
        Ok(facts)
    }

    fn cancel(
        &self,
        input: &RuntimeInput,
        reason: &str,
        _control: &RuntimeControl,
    ) -> Result<Vec<RuntimeFact>, RuntimeOperationError> {
        // Pi invocation is synchronous and waits for its child. Retained state
        // represents a live clarification turn, not an in-flight child.
        let Some(context) = self
            .load_context(&input.session_id)
            .map_err(RuntimeOperationError::from)?
        else {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "Pi clarification session cancellation was not confirmed",
            )));
        };
        if context.in_flight {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "Pi clarification cancellation could not interrupt active execution",
            )));
        }
        self.clear_session(&input.session_id)
            .map_err(RuntimeOperationError::from)?;
        Ok(vec![RuntimeFact::Completed {
            summary: format!("Pi clarification cancelled: {reason}"),
        }])
    }

    fn dispatch_with_control(
        &self,
        input: RuntimeInput,
        control: &RuntimeControl,
    ) -> Result<Vec<RuntimeFact>, RuntimeOperationError> {
        let cancellation_command = matches!(&input.command, RuntimeCommand::Cancel { .. });
        let result = match input.command.clone() {
            RuntimeCommand::Start {
                requirement,
                conversation,
                repositories,
            } => self.start(
                &input,
                StartInput {
                    requirement,
                    conversation,
                    repositories,
                },
                control,
            ),
            RuntimeCommand::Message {
                message_id,
                content,
            } => self.message(
                &input,
                MessageSend {
                    message_id,
                    content,
                },
                control,
            ),
            RuntimeCommand::Cancel { reason } => self.cancel(&input, &reason, control),
            RuntimeCommand::Resume => {
                check_cancellation(control)?;
                Ok(Vec::new())
            }
        };
        if control.is_cancellation_requested() && !cancellation_command {
            Err(RuntimeOperationError::Cancelled)
        } else {
            result
        }
    }

    fn invoke_pi(
        &self,
        session_id: &str,
        prompt: &str,
        control: &RuntimeControl,
    ) -> Result<String, RuntimeOperationError> {
        check_cancellation(control)?;
        let mut command = ProcessCommand::new(&self.agent_command);
        for variable in [
            "GIT_ASKPASS",
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "GIT_SSH_VARIANT",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_CONFIG_PARAMETERS",
            "SSH_ASKPASS",
            "SSH_AUTH_SOCK",
        ] {
            command.env_remove(variable);
        }
        command
            .args([
                "--mode",
                "json",
                "--print",
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-context-files",
                "--system-prompt",
                PI_SYSTEM_PROMPT,
                "--session-id",
                session_id,
                "--session-dir",
            ])
            .arg(&self.session_dir)
            .arg(prompt);
        if control.is_cancellation_requested() {
            return Err(RuntimeOperationError::Cancelled);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            RuntimeOperationError::Failed(RuntimeError::new(format!("run Pi Agent: {error}")))
        })?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                stop_process(&mut child);
                return Err(RuntimeOperationError::Failed(RuntimeError::new(
                    "Pi Agent stdout pipe was unavailable",
                )));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                stop_process(&mut child);
                return Err(RuntimeOperationError::Failed(RuntimeError::new(
                    "Pi Agent stderr pipe was unavailable",
                )));
            }
        };
        let cancellation = Arc::new(AtomicBool::new(control.is_cancellation_requested()));
        if self
            .cancellation_flags
            .lock()
            .map(|mut flags| {
                flags.insert(session_id.to_owned(), cancellation.clone());
            })
            .is_err()
        {
            stop_process(&mut child);
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "Pi cancellation state lock poisoned",
            )));
        }
        let output_overflow = Arc::new(AtomicBool::new(false));
        let stdout_overflow = output_overflow.clone();
        let stderr_overflow = output_overflow.clone();
        let stdout_reader = thread::spawn(move || read_process_output(stdout, stdout_overflow));
        let stderr_reader = thread::spawn(move || read_process_output(stderr, stderr_overflow));
        let deadline = Instant::now() + PI_PROCESS_TIMEOUT;
        let wait_result = loop {
            if control.is_cancellation_requested() || cancellation.load(Ordering::Acquire) {
                stop_process(&mut child);
                break Err(RuntimeOperationError::Cancelled);
            }
            if output_overflow.load(Ordering::Acquire) {
                stop_process(&mut child);
                break Err(RuntimeOperationError::Failed(RuntimeError::new(
                    "Pi Agent response exceeded bounded output",
                )));
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() >= deadline => {
                    stop_process(&mut child);
                    break Err(RuntimeOperationError::Failed(RuntimeError::new(
                        "Pi Agent process timed out",
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    stop_process(&mut child);
                    break Err(RuntimeOperationError::Failed(RuntimeError::new(format!(
                        "wait for Pi Agent: {error}"
                    ))));
                }
            }
        };
        if let Ok(mut flags) = self.cancellation_flags.lock() {
            flags.remove(session_id);
        }
        let stdout = stdout_reader
            .join()
            .map_err(|_| {
                RuntimeOperationError::Failed(RuntimeError::new("read Pi Agent stdout panicked"))
            })?
            .map_err(|error| {
                RuntimeOperationError::Failed(RuntimeError::new(format!(
                    "read Pi Agent stdout: {error}"
                )))
            })?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| {
                RuntimeOperationError::Failed(RuntimeError::new("read Pi Agent stderr panicked"))
            })?
            .map_err(|error| {
                RuntimeOperationError::Failed(RuntimeError::new(format!(
                    "read Pi Agent stderr: {error}"
                )))
            })?;
        if control.is_cancellation_requested() || cancellation.load(Ordering::Acquire) {
            return Err(RuntimeOperationError::Cancelled);
        }
        let status = wait_result?;
        if !status.success() {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(format!(
                "Pi Agent exited with {status}"
            ))));
        }
        if output_overflow.load(Ordering::Acquire)
            || stdout.len() > MAX_PI_OUTPUT_BYTES
            || stderr.len() > MAX_PI_OUTPUT_BYTES
        {
            return Err(RuntimeOperationError::Failed(RuntimeError::new(
                "Pi Agent response exceeded bounded output",
            )));
        }
        extract_assistant_text(&stdout).map_err(RuntimeOperationError::from)
    }
}

fn stop_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_process_output<R: Read>(
    reader: R,
    output_overflow: Arc<AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take(MAX_PI_OUTPUT_BYTES as u64 + 1)
        .read_to_end(&mut output)?;
    if output.len() > MAX_PI_OUTPUT_BYTES {
        output_overflow.store(true, Ordering::Release);
    }
    Ok(output)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StartInput {
    requirement: RequirementSnapshot,
    conversation: Vec<ConversationTurn>,
    repositories: Vec<AuthorizedRepository>,
}

fn source_for_repository(
    repository: &AuthorizedRepository,
) -> Result<RepositorySource, RuntimeOperationError> {
    #[cfg(test)]
    {
        // Unit fixtures use local Git paths. Production always validates the
        // server wire URL through RepositorySource::from_context below.
        Ok(RepositorySource::new(
            repository.repository_id.clone(),
            repository.url.clone(),
        ))
    }
    #[cfg(not(test))]
    {
        RepositorySource::from_context(&RepositoryContext {
            repository_id: repository.repository_id.clone(),
            name: repository.name.clone(),
            url: repository.url.clone(),
            description: repository.description.clone(),
        })
        .map_err(RuntimeOperationError::from)
    }
}

fn check_cancellation(control: &RuntimeControl) -> Result<(), RuntimeOperationError> {
    if control.is_cancellation_requested() {
        Err(RuntimeOperationError::Cancelled)
    } else {
        Ok(())
    }
}

fn check_cancellation_or_cleanup(
    adapter: &PiClarificationAdapter,
    session_id: &str,
    control: &RuntimeControl,
) -> Result<(), RuntimeOperationError> {
    if control.is_cancellation_requested() {
        Err(adapter.cleanup_after_error(session_id, RuntimeOperationError::Cancelled))
    } else {
        Ok(())
    }
}

fn classify_inspection_error(
    control: &RuntimeControl,
    error: InspectionError,
) -> RuntimeOperationError {
    if control.is_cancellation_requested() || matches!(error.phase, InspectionPhase::Cancellation) {
        RuntimeOperationError::Cancelled
    } else {
        RuntimeOperationError::from(error)
    }
}

fn evidence_size(evidence: &RepositoryEvidence) -> (usize, usize) {
    let bytes = evidence.repository_id.len()
        + evidence.commit_sha.len()
        + evidence
            .files
            .iter()
            .map(|file| file.path.len().saturating_add(file.content.len()))
            .sum::<usize>();
    (bytes, evidence.files.len())
}

fn extract_repository_evidence(
    workspace: &PreparedWorkspace,
    authorization: &RunAuthorization,
    control: &RuntimeControl,
    cancellation: &InspectionCancellation,
) -> Result<RepositoryEvidence, String> {
    if control.is_cancellation_requested() {
        cancellation.cancel();
        return Err("inspection cancelled".into());
    }
    let listing = workspace
        .read_git_bounded_with_cancellation(
            authorization,
            &["ls-files".into(), "--cached".into(), "-z".into()],
            MAX_EVIDENCE_LIST_BYTES,
            cancellation,
        )
        .map_err(|error| error.to_string())?;
    let mut listing_records = listing.bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if listing.truncated {
        // A bounded listing may end in the middle of one NUL-delimited path.
        // Ignore that incomplete record and retain deterministic complete ones.
        listing_records.pop();
    }
    let mut paths = listing_records
        .into_iter()
        .filter(|path| !path.is_empty())
        .filter_map(|path| String::from_utf8(path.to_vec()).ok())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    let had_paths = !paths.is_empty();
    let mut files = Vec::new();
    let mut evidence_bytes = 0usize;
    for path in paths.into_iter().take(MAX_EVIDENCE_FILES) {
        if control.is_cancellation_requested() {
            cancellation.cancel();
            return Err("inspection cancelled".into());
        }
        if path.bytes().any(|byte| byte == 0) {
            return Err("repository file path contains NUL".into());
        }
        let object = format!("{}:{path}", workspace.commit_sha());
        let content = workspace
            .read_git_bounded_with_cancellation(
                authorization,
                &[
                    "show".into(),
                    "--format=".into(),
                    "--no-ext-diff".into(),
                    object,
                ],
                MAX_EVIDENCE_FILE_BYTES,
                cancellation,
            )
            .map_err(|error| error.to_string())?;
        // Oversized and binary/non-UTF-8 files are not valid bounded source
        // evidence. Skip them and continue searching for a usable file.
        if content.truncated || content.bytes.contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(content.bytes) else {
            continue;
        };
        let entry_bytes = path.len().saturating_add(content.len());
        if evidence_bytes.saturating_add(entry_bytes) > MAX_EVIDENCE_BYTES {
            break;
        }
        evidence_bytes = evidence_bytes.saturating_add(entry_bytes);
        files.push(SourceFileEvidence { path, content });
    }
    if !had_paths {
        return Err("repository contains no tracked source files".into());
    }
    if files.is_empty() {
        return Err("repository source evidence exceeded bounded size".into());
    }
    if control.is_cancellation_requested() {
        cancellation.cancel();
        return Err("inspection cancelled".into());
    }
    Ok(RepositoryEvidence {
        repository_id: workspace.repository_id().to_owned(),
        commit_sha: workspace.commit_sha().to_owned(),
        files,
    })
}

impl ClarificationRuntime for PiClarificationAdapter {
    fn dispatch(&self, input: RuntimeInput) -> Result<Vec<RuntimeFact>, RuntimeError> {
        let control = RuntimeControl::new(&input.session_id, &input.operation_id);
        self.dispatch_with_control(input, &control)
            .map_err(RuntimeOperationError::into_runtime_error)
    }
}

impl crate::journal::RuntimeExecutor for PiClarificationAdapter {
    fn dispatch(
        &self,
        _runtime_operation_id: &str,
        _command_id: &str,
        _command: &Command,
    ) -> crate::journal::DispatchOutcome {
        crate::journal::DispatchOutcome::DispatchFailed(
            "session identity required by clarification runtime".into(),
        )
    }

    fn dispatch_for_session(
        &self,
        session_id: &str,
        runtime_operation_id: &str,
        command_id: &str,
        command: &Command,
    ) -> crate::journal::DispatchOutcome {
        self.dispatch_for_session_with_control(
            session_id,
            runtime_operation_id,
            command_id,
            command,
            RuntimeControl::new(session_id, command_id),
        )
    }

    fn dispatch_for_session_with_control(
        &self,
        session_id: &str,
        runtime_operation_id: &str,
        command_id: &str,
        command: &Command,
        control: RuntimeControl,
    ) -> crate::journal::DispatchOutcome {
        let input = RuntimeInput {
            operation_id: runtime_operation_id.to_owned(),
            session_id: session_id.to_owned(),
            command: command_to_runtime(command),
        };
        let result = self.dispatch_with_control(input, &control);
        let outcome = match result {
            Ok(facts) => {
                let events = facts.into_iter().map(fact_to_event).collect::<Vec<_>>();
                if let Ok(mut pending) = self.pending_events.lock() {
                    pending
                        .entry(session_id.to_owned())
                        .or_default()
                        .extend(events);
                }
                crate::journal::DispatchOutcome::DispatchSucceeded
            }
            Err(RuntimeOperationError::Cancelled) => {
                if let Ok(mut pending) = self.pending_events.lock() {
                    pending.entry(session_id.to_owned()).or_default().push(
                        Event::SessionCompleted(SessionCompleted {
                            summary: "Pi clarification cancelled".into(),
                        }),
                    );
                }
                crate::journal::DispatchOutcome::DispatchSucceeded
            }
            Err(RuntimeOperationError::Failed(error)) => {
                if let Ok(mut pending) = self.pending_events.lock() {
                    pending
                        .entry(session_id.to_owned())
                        .or_default()
                        .push(Event::SessionFailed(SessionFailed {
                            recoverable: false,
                            reason: error.to_string(),
                        }));
                }
                crate::journal::DispatchOutcome::DispatchFailed(error.to_string())
            }
        };
        let _ = command_id;
        outcome
    }

    fn take_events(&self, session_id: &str) -> Vec<Event> {
        self.pending_events
            .lock()
            .map(|mut pending| pending.remove(session_id).unwrap_or_default())
            .unwrap_or_default()
    }

    fn cancel_for_session(&self, session_id: &str) -> bool {
        let Ok(flags) = self.cancellation_flags.lock() else {
            return false;
        };
        let Some(flag) = flags.get(session_id) else {
            return false;
        };
        flag.store(true, Ordering::SeqCst);
        true
    }

    fn recover(
        &self,
        _runtime_operation_id: &str,
        _command_id: &str,
        _command: &Command,
    ) -> crate::journal::RecoveryOutcome {
        crate::journal::RecoveryOutcome::Unknown
    }
}

fn command_to_runtime(command: &Command) -> RuntimeCommand {
    match command {
        Command::SessionStart(start) => RuntimeCommand::Start {
            requirement: RequirementSnapshot {
                id: start.requirement.id.clone(),
                revision: start.requirement.revision,
                title: start.requirement.title.clone(),
                description: start.requirement.description.clone(),
                summary: start.requirement.summary.clone(),
                acceptance_criteria: start.requirement.acceptance_criteria.clone(),
                assumptions: start.requirement.assumptions.clone(),
                open_questions: start.requirement.open_questions.clone(),
            },
            conversation: start
                .conversation
                .excerpt
                .iter()
                .map(|message| ConversationTurn {
                    message_id: message.message_id.clone(),
                    role: match message.role {
                        north_protocol::ConversationRoleWire::Requester => TurnRole::Requester,
                        north_protocol::ConversationRoleWire::Agent => TurnRole::Agent,
                        north_protocol::ConversationRoleWire::System => TurnRole::System,
                    },
                    content: message.content.clone(),
                })
                .collect(),
            repositories: start
                .repositories
                .iter()
                .map(|repository| AuthorizedRepository {
                    repository_id: repository.repository_id.clone(),
                    name: repository.name.clone(),
                    url: repository.url.clone(),
                    description: repository.description.clone(),
                })
                .collect(),
        },
        Command::MessageSend(message) => RuntimeCommand::Message {
            message_id: message.message_id.clone(),
            content: message.content.clone(),
        },
        Command::SessionCancel(cancel) => RuntimeCommand::Cancel {
            reason: cancel.reason.clone(),
        },
        Command::SessionResume(_) => RuntimeCommand::Resume,
    }
}

fn fact_to_event(fact: RuntimeFact) -> Event {
    match fact {
        RuntimeFact::SessionStarted { runtime_id } => {
            Event::SessionStarted(SessionStarted { runtime_id })
        }
        RuntimeFact::AgentMessage {
            message_id,
            content,
        } => Event::AgentMessage(AgentMessage {
            message_id,
            content,
        }),
        RuntimeFact::Activity { activity } => Event::AgentActivity(AgentActivity { activity }),
        RuntimeFact::Assessed {
            requirement_id,
            requirement_revision,
            verdict,
            blockers,
            assumptions,
            repositories_reviewed,
        } => Event::RequirementAssessed(RequirementAssessed {
            requirement_id,
            requirement_revision,
            verdict: match verdict {
                ReadinessVerdict::Ready => ReadinessVerdictWire::Ready,
                ReadinessVerdict::NeedsClarification => ReadinessVerdictWire::NeedsClarification,
            },
            blockers,
            assumptions,
            repositories_reviewed: repositories_reviewed
                .into_iter()
                .map(|repository| ReviewedRepositoryWire {
                    repository_id: repository.repository_id,
                    commit_sha: repository.commit_sha,
                })
                .collect(),
        }),
        RuntimeFact::Completed { summary } => Event::SessionCompleted(SessionCompleted { summary }),
        RuntimeFact::Failed {
            recoverable,
            reason,
        } => Event::SessionFailed(SessionFailed {
            recoverable,
            reason,
        }),
    }
}

fn facts_for_response(
    operation_id: &str,
    runtime_id: String,
    response: ParsedAssessment,
    context: Option<AssessmentContext>,
    include_started: bool,
    include_completion: bool,
) -> Vec<RuntimeFact> {
    let mut facts = Vec::new();
    if include_started {
        facts.push(RuntimeFact::SessionStarted { runtime_id });
    }
    facts.push(RuntimeFact::Activity {
        activity: "Pi clarification response received".into(),
    });
    facts.push(RuntimeFact::AgentMessage {
        message_id: format!("agent-response-{operation_id}"),
        content: response.message,
    });
    if let Some(context) = context {
        facts.push(RuntimeFact::Assessed {
            requirement_id: context.requirement_id,
            requirement_revision: context.requirement_revision,
            verdict: response.verdict,
            blockers: non_empty_facts(response.blockers, "No blockers identified"),
            assumptions: non_empty_facts(response.assumptions, "North context is authoritative"),
            repositories_reviewed: context.repositories_reviewed,
        });
    }
    if include_completion {
        facts.push(RuntimeFact::Completed {
            summary: "Pi clarification completed".into(),
        });
    }
    facts
}

#[derive(Debug)]
struct ParsedAssessment {
    message: String,
    verdict: ReadinessVerdict,
    blockers: Vec<String>,
    assumptions: Vec<String>,
}

fn parse_assessment(text: &str) -> ParsedAssessment {
    let trimmed = text.trim();
    let parsed = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    if let Ok(value) = serde_json::from_str::<PiAssessment>(parsed) {
        let message = if value.message.trim().is_empty() {
            "Pi returned no user-facing clarification.".into()
        } else {
            value.message.trim().into()
        };
        return ParsedAssessment {
            message,
            verdict: verdict(value.verdict.as_deref()),
            blockers: value.blockers,
            assumptions: value.assumptions,
        };
    }
    ParsedAssessment {
        message: if trimmed.is_empty() {
            "Pi returned no user-facing clarification.".into()
        } else {
            trimmed.into()
        },
        verdict: ReadinessVerdict::NeedsClarification,
        blockers: vec!["Pi response was not a structured readiness assessment".into()],
        assumptions: vec!["North server remains readiness authority".into()],
    }
}

fn verdict(value: Option<&str>) -> ReadinessVerdict {
    if value.is_some_and(|value| value.eq_ignore_ascii_case("ready")) {
        ReadinessVerdict::Ready
    } else {
        ReadinessVerdict::NeedsClarification
    }
}

fn non_empty_facts(values: Vec<String>, fallback: &str) -> Vec<String> {
    let values = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        vec![fallback.into()]
    } else {
        values
    }
}

fn start_prompt(
    start: &StartInput,
    reviewed: &[RepositoryReview],
    evidence: &[RepositoryEvidence],
) -> String {
    let context = serde_json::json!({
        "requirement": start.requirement,
        "conversation": start.conversation,
        "authorized_repositories": start.repositories.iter().map(|repository| serde_json::json!({
            "repository_id": repository.repository_id,
        })).collect::<Vec<_>>(),
        "repository_revisions_reviewed": reviewed,
        "repository_source_evidence": evidence,
    });
    format!(
        "Clarify this Requirement from North-authorized context.\n{}",
        context
    )
}

fn message_prompt(
    context: Option<&AssessmentContext>,
    message: &MessageSend,
) -> Result<String, RuntimeError> {
    let context = context.ok_or_else(|| {
        RuntimeError::new("Pi clarification message has no active session context")
    })?;
    let start = context
        .context
        .as_ref()
        .ok_or_else(|| RuntimeError::new("Pi clarification context is unavailable"))?;
    Ok(format!(
        "{}\nRespond to this requester message in the established clarification session.\nMessage ID: {}\nMessage: {}",
        start_prompt(start, &context.repositories_reviewed, &context.evidence),
        message.message_id,
        message.content,
    ))
}

fn extract_assistant_text(output: &[u8]) -> Result<String, RuntimeError> {
    let mut text = String::new();
    for line in String::from_utf8_lossy(output).lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("message_update") {
            continue;
        }
        let Some(event) = value.get("assistantMessageEvent") else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) == Some("text_delta") {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                text.push_str(delta);
            }
        }
    }
    if text.trim().is_empty() {
        return Err(RuntimeError::new("Pi Agent returned no assistant text"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_protocol::{
        CommandEnvelope, DaemonFrame, ServerFrame, SessionCancel, SCHEMA_VERSION,
    };

    #[test]
    fn pi_lifecycle_and_tool_records_are_not_north_facts() {
        let output = br#"{"type":"tool_execution_start","toolName":"read"}
{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"{\"message\":\"Done\",\"verdict\":\"ready\"}"}}
{"type":"message_end","message":{"role":"assistant"}}"#;
        assert_eq!(
            extract_assistant_text(output).expect("assistant text"),
            r#"{"message":"Done","verdict":"ready"}"#
        );
    }

    #[test]
    fn unstructured_pi_result_stays_needs_clarification() {
        let result = parse_assessment("not JSON");
        assert_eq!(result.verdict, ReadinessVerdict::NeedsClarification);
        assert!(!result.blockers.is_empty());
    }

    #[test]
    fn process_output_sets_overflow_flag_at_limit() -> Result<(), Box<dyn std::error::Error>> {
        let overflow = Arc::new(AtomicBool::new(false));
        let output = read_process_output(
            std::io::Cursor::new(vec![b'x'; MAX_PI_OUTPUT_BYTES + 1]),
            overflow.clone(),
        )?;
        assert_eq!(output.len(), MAX_PI_OUTPUT_BYTES + 1);
        assert!(overflow.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn start_prompt_contains_only_server_review_context() {
        let prompt = start_prompt(
            &StartInput {
                requirement: RequirementSnapshot {
                    id: "requirement-1".into(),
                    revision: 1,
                    title: "Title".into(),
                    description: "Description".into(),
                    summary: "Summary".into(),
                    acceptance_criteria: vec!["Criterion".into()],
                    assumptions: vec![],
                    open_questions: vec![],
                },
                conversation: vec![ConversationTurn {
                    message_id: "message-1".into(),
                    role: TurnRole::Requester,
                    content: "Clarify".into(),
                }],
                repositories: vec![AuthorizedRepository {
                    repository_id: "repository-1".into(),
                    name: "Repository".into(),
                    url: "https://example.test/repository.git".into(),
                    description: "Authorized source".into(),
                }],
            },
            &[RepositoryReview {
                repository_id: "repository-1".into(),
                commit_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            }],
            &[],
        );
        assert!(prompt.contains("repository-1"));
        assert!(prompt.contains("0123456789abcdef0123456789abcdef01234567"));
        assert!(!prompt.contains("example.test"));
        assert!(!prompt.contains("Authorized source"));
        assert!(!prompt.contains("checkout"));
        assert!(!prompt.contains("credential"));
    }

    #[cfg(unix)]
    #[test]
    fn adapter_maps_pi_response_to_existing_facts() -> Result<(), Box<dyn std::error::Error>> {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::tempdir()?;
        let inspector = RepositoryInspector::new(
            directory.path().join("cache"),
            directory.path().join("workspaces"),
        )?;
        let command = directory.path().join("fake-pi");
        fs::write(
            &command,
            r##"#!/bin/sh
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"{\"message\":\"Need scope\",\"verdict\":\"ready\",\"blockers\":[\"scope\"],\"assumptions\":[\"context\"]}"}}'
"##,
        )?;
        let mut permissions = fs::metadata(&command)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&command, permissions)?;
        let adapter = PiClarificationAdapter::new(inspector, directory.path().join("sessions"))?
            .with_agent_command(command);
        let start = Command::SessionStart(north_protocol::SessionStart {
            requirement: north_protocol::RequirementContext {
                id: "requirement-1".into(),
                revision: 1,
                title: "Title".into(),
                description: "Description".into(),
                summary: "Summary".into(),
                acceptance_criteria: vec!["Criterion".into()],
                assumptions: vec!["Assumption".into()],
                open_questions: vec!["Question".into()],
            },
            conversation: north_protocol::ConversationContext {
                excerpt: vec![north_protocol::ConversationMessageContext {
                    message_id: "message-1".into(),
                    role: north_protocol::ConversationRoleWire::Requester,
                    content: "Clarify".into(),
                }],
            },
            repositories: Vec::new(),
        });
        let coordinator = crate::coordination::DaemonCoordinator::new(
            crate::journal::Journal::open(directory.path().join("journal.json"), "daemon-1")?,
            adapter,
        );
        let start_frames =
            coordinator.process_server_frame(ServerFrame::Command(CommandEnvelope {
                command_id: "command-1".into(),
                session_id: "session-1".into(),
                server_command_seq: 1,
                sent_at: "2026-01-01T00:00:00Z".into(),
                schema_version: SCHEMA_VERSION,
                command: start,
            }))?;
        let events = start_frames
            .into_iter()
            .filter_map(|frame| match frame {
                DaemonFrame::Event(event) => Some(event.event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(matches!(events[0], Event::SessionStarted(_)));
        assert!(matches!(events[1], Event::AgentActivity(_)));
        assert!(matches!(events[2], Event::AgentMessage(_)));
        assert!(matches!(events[3], Event::RequirementAssessed(_)));
        assert!(matches!(events[4], Event::SessionCompleted(_)));
        assert_eq!(events.len(), 5);

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn adapter_rehydrates_context_after_restart() -> Result<(), Box<dyn std::error::Error>> {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::tempdir()?;
        let cache = directory.path().join("cache");
        let workspaces = directory.path().join("workspaces");
        let session_dir = directory.path().join("sessions");
        let command = directory.path().join("fake-pi");
        fs::write(
            &command,
            r##"#!/bin/sh
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"{\"message\":\"Need scope\",\"verdict\":\"needs_clarification\",\"blockers\":[\"scope\"],\"assumptions\":[\"context\"]}"}}'
"##,
        )?;
        let mut permissions = fs::metadata(&command)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&command, permissions)?;
        let start = RuntimeInput {
            operation_id: "start-operation".into(),
            session_id: "session-1".into(),
            command: RuntimeCommand::Start {
                requirement: RequirementSnapshot {
                    id: "requirement-1".into(),
                    revision: 3,
                    title: "Title".into(),
                    description: "Description".into(),
                    summary: "Summary".into(),
                    acceptance_criteria: vec!["Criterion".into()],
                    assumptions: vec!["Assumption".into()],
                    open_questions: vec!["Question".into()],
                },
                conversation: vec![ConversationTurn {
                    message_id: "message-1".into(),
                    role: TurnRole::Requester,
                    content: "Clarify".into(),
                }],
                repositories: Vec::new(),
            },
        };
        let first = PiClarificationAdapter::new(
            RepositoryInspector::new(&cache, &workspaces)?,
            &session_dir,
        )?
        .with_agent_command(command.clone());
        let facts = first.dispatch(start)?;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            RuntimeFact::Assessed {
                requirement_id,
                requirement_revision: 3,
                verdict: ReadinessVerdict::NeedsClarification,
                ..
            } if requirement_id == "requirement-1"
        )));
        drop(first);

        let second = PiClarificationAdapter::new(
            RepositoryInspector::new(&cache, &workspaces)?,
            &session_dir,
        )?
        .with_agent_command(command);
        let facts = second.dispatch(RuntimeInput {
            operation_id: "message-operation".into(),
            session_id: "session-1".into(),
            command: RuntimeCommand::Message {
                message_id: "message-2".into(),
                content: "More detail".into(),
            },
        })?;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            RuntimeFact::Assessed {
                requirement_id,
                requirement_revision: 3,
                ..
            } if requirement_id == "requirement-1"
        )));
        assert!(second.context_path("session-1").is_file());
        Ok(())
    }

    #[test]
    fn runtime_events_are_scoped_and_unconfirmed_cancellation_fails(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let inspector = RepositoryInspector::new(
            directory.path().join("cache"),
            directory.path().join("workspaces"),
        )?;
        let adapter = PiClarificationAdapter::new(inspector, directory.path().join("sessions"))?;
        let mut sessions = adapter
            .sessions
            .lock()
            .map_err(|_| std::io::Error::other("Pi session state lock poisoned"))?;
        for session_id in ["session-a", "session-b"] {
            sessions.insert(
                session_id.into(),
                AssessmentContext {
                    requirement_id: "requirement-1".into(),
                    requirement_revision: 1,
                    repositories_reviewed: Vec::new(),
                    evidence: Vec::new(),
                    context: None,
                    in_flight: false,
                },
            );
        }
        drop(sessions);

        for (session_id, command_id) in [("session-a", "command-a"), ("session-b", "command-b")] {
            let outcome = crate::journal::RuntimeExecutor::dispatch_for_session(
                &adapter,
                session_id,
                command_id,
                command_id,
                &Command::SessionCancel(SessionCancel {
                    reason: "requester_cancelled".into(),
                }),
            );
            assert_eq!(outcome, crate::journal::DispatchOutcome::DispatchSucceeded);
        }
        assert!(matches!(
            crate::journal::RuntimeExecutor::take_events(&adapter, "session-a").as_slice(),
            [Event::SessionCompleted(_)]
        ));
        assert!(matches!(
            crate::journal::RuntimeExecutor::take_events(&adapter, "session-b").as_slice(),
            [Event::SessionCompleted(_)]
        ));

        let outcome = crate::journal::RuntimeExecutor::dispatch_for_session(
            &adapter,
            "session-a",
            "command-c",
            "command-c",
            &Command::SessionCancel(SessionCancel {
                reason: "requester_cancelled".into(),
            }),
        );
        assert!(matches!(
            outcome,
            crate::journal::DispatchOutcome::DispatchFailed(_)
        ));
        assert!(matches!(
            crate::journal::RuntimeExecutor::take_events(&adapter, "session-a").as_slice(),
            [Event::SessionFailed(_)]
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_interrupts_active_pi_child() -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            sync::{mpsc, Arc},
        };

        let directory = tempfile::tempdir()?;
        let command = directory.path().join("fake-pi");
        fs::write(&command, "#!/bin/sh\nwhile :; do :; done\n")?;
        let mut permissions = fs::metadata(&command)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&command, permissions)?;
        let adapter = Arc::new(
            PiClarificationAdapter::new(
                RepositoryInspector::new(
                    directory.path().join("cache"),
                    directory.path().join("workspaces"),
                )?,
                directory.path().join("sessions"),
            )?
            .with_agent_command(command),
        );
        let (sender, receiver) = mpsc::channel();
        let worker_adapter = adapter.clone();
        let worker = thread::spawn(move || {
            let outcome = crate::journal::RuntimeExecutor::dispatch_for_session(
                worker_adapter.as_ref(),
                "session-1",
                "operation-1",
                "command-1",
                &Command::SessionStart(north_protocol::SessionStart {
                    requirement: north_protocol::RequirementContext {
                        id: "requirement-1".into(),
                        revision: 1,
                        title: "Title".into(),
                        description: "Description".into(),
                        summary: "Summary".into(),
                        acceptance_criteria: Vec::new(),
                        assumptions: Vec::new(),
                        open_questions: Vec::new(),
                    },
                    conversation: north_protocol::ConversationContext {
                        excerpt: Vec::new(),
                    },
                    repositories: Vec::new(),
                }),
            );
            let events =
                crate::journal::RuntimeExecutor::take_events(worker_adapter.as_ref(), "session-1");
            sender
                .send((outcome, events))
                .expect("send cancellation result");
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if adapter
                .cancellation_flags
                .lock()
                .expect("cancellation state")
                .contains_key("session-1")
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(adapter
            .cancellation_flags
            .lock()
            .expect("cancellation state")
            .contains_key("session-1"));
        assert!(crate::journal::RuntimeExecutor::cancel_for_session(
            adapter.as_ref(),
            "session-1"
        ));
        let (outcome, events) = receiver.recv_timeout(Duration::from_secs(2))?;
        assert_eq!(outcome, crate::journal::DispatchOutcome::DispatchSucceeded);
        assert!(matches!(events.as_slice(), [Event::SessionCompleted(_)]));
        worker.join().expect("join cancellation worker");
        Ok(())
    }

    #[cfg(unix)]
    fn git_fixture(
        root: &std::path::Path,
        name: &str,
        marker: &str,
    ) -> Result<(std::path::PathBuf, String), Box<dyn std::error::Error>> {
        use std::{fs, process::Command as GitCommand};

        let repository = root.join(name);
        fs::create_dir_all(repository.join("src"))?;
        fs::write(
            repository.join("src/domain.rs"),
            format!("// {marker}\nfn domain() {{}}\n"),
        )?;
        let run = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
            let output = GitCommand::new("git")
                .current_dir(&repository)
                .args(args)
                .output()?;
            if !output.status.success() {
                return Err(std::io::Error::other(format!(
                    "git {args:?} failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
                .into());
            }
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        };
        run(&["init", "--quiet"])?;
        run(&["config", "user.email", "north-test@example.test"])?;
        run(&["config", "user.name", "North Test"])?;
        run(&["add", "."])?;
        run(&["commit", "--quiet", "-m", "fixture"])?;
        let sha = run(&["rev-parse", "HEAD"])?.trim().to_owned();
        Ok((repository, sha))
    }

    #[cfg(unix)]
    fn fake_pi(root: &std::path::Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        use std::{fs, os::unix::fs::PermissionsExt};

        let command = root.join("fake-pi");
        fs::write(
            &command,
            r##"#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
printf '%s' started > "$base/pi-started"
last=
for arg in "$@"; do last="$arg"; done
printf '%s' "$last" > "$base/pi-prompt"
case "$last" in
  *NORTH_AUTHORIZED_REPOSITORY_MARKER*) ;;
  *) exit 41 ;;
esac
case "$last" in
  *FORBIDDEN_MARKER*) exit 42 ;;
esac
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"{\"message\":\"reviewed\",\"verdict\":\"ready\",\"blockers\":[],\"assumptions\":[]}"}}'
"##,
        )?;
        let mut permissions = fs::metadata(&command)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&command, permissions)?;
        Ok(command)
    }

    #[cfg(unix)]
    fn start_command(repository_id: &str, url: String) -> Command {
        Command::SessionStart(north_protocol::SessionStart {
            requirement: north_protocol::RequirementContext {
                id: "requirement-1".into(),
                revision: 7,
                title: "Title".into(),
                description: "Description".into(),
                summary: "Summary".into(),
                acceptance_criteria: vec!["Criterion".into()],
                assumptions: vec![],
                open_questions: vec![],
            },
            conversation: north_protocol::ConversationContext {
                excerpt: vec![north_protocol::ConversationMessageContext {
                    message_id: "message-1".into(),
                    role: north_protocol::ConversationRoleWire::Requester,
                    content: "Clarify source behavior".into(),
                }],
            },
            repositories: vec![north_protocol::RepositoryContext {
                repository_id: repository_id.into(),
                name: "Authorized repository".into(),
                url,
                description: "Server-selected source".into(),
            }],
        })
    }

    #[cfg(unix)]
    #[test]
    fn authorized_source_reaches_pi_and_unauthorized_source_stays_out(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let directory = tempfile::tempdir()?;
        let (authorized, sha) = git_fixture(
            directory.path(),
            "authorized-repository",
            "NORTH_AUTHORIZED_REPOSITORY_MARKER",
        )?;
        let (_unauthorized, _) = git_fixture(
            directory.path(),
            "unauthorized-repository",
            "FORBIDDEN_MARKER",
        )?;
        let (second_authorized, second_sha) = git_fixture(
            directory.path(),
            "second-authorized-repository",
            "SECOND_AUTHORIZED_REPOSITORY_MARKER",
        )?;
        let adapter = PiClarificationAdapter::new(
            RepositoryInspector::new(
                directory.path().join("cache"),
                directory.path().join("workspaces"),
            )?,
            directory.path().join("sessions"),
        )?
        .with_agent_command(fake_pi(directory.path())?);
        let mut start = start_command(
            "repository-authorized",
            authorized.to_string_lossy().into_owned(),
        );
        if let Command::SessionStart(context) = &mut start {
            context
                .repositories
                .push(north_protocol::RepositoryContext {
                    repository_id: "repository-second".into(),
                    name: "Second authorized repository".into(),
                    url: second_authorized.to_string_lossy().into_owned(),
                    description: "Second server-selected source".into(),
                });
        }
        let outcome = crate::journal::RuntimeExecutor::dispatch_for_session_with_control(
            &adapter,
            "session-authorized",
            "operation-authorized",
            "command-authorized",
            &start,
            RuntimeControl::new("session-authorized", "command-authorized"),
        );
        assert_eq!(outcome, crate::journal::DispatchOutcome::DispatchSucceeded);
        let events = crate::journal::RuntimeExecutor::take_events(&adapter, "session-authorized");
        let assessment = events.iter().find_map(|event| match event {
            Event::RequirementAssessed(assessment) => Some(assessment),
            _ => None,
        });
        let assessment = assessment.ok_or("missing assessment")?;
        assert_eq!(
            assessment.repositories_reviewed,
            vec![
                ReviewedRepositoryWire {
                    repository_id: "repository-authorized".into(),
                    commit_sha: sha,
                },
                ReviewedRepositoryWire {
                    repository_id: "repository-second".into(),
                    commit_sha: second_sha,
                },
            ]
        );
        let prompt = fs::read_to_string(directory.path().join("pi-prompt"))?;
        assert!(prompt.contains("NORTH_AUTHORIZED_REPOSITORY_MARKER"));
        assert!(prompt.contains("SECOND_AUTHORIZED_REPOSITORY_MARKER"));
        assert!(!prompt.contains("FORBIDDEN_MARKER"));
        assert!(!prompt.contains("authorized-repository"));
        assert!(!prompt.contains("unauthorized-repository"));
        assert!(fs::read_dir(directory.path().join("workspaces"))?
            .next()
            .is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_during_repository_preparation_skips_pi_and_cleans_workspace(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            fs,
            sync::{atomic::AtomicBool, Arc},
        };

        let directory = tempfile::tempdir()?;
        let (authorized, _) = git_fixture(
            directory.path(),
            "cancel-repository",
            "NORTH_AUTHORIZED_REPOSITORY_MARKER",
        )?;
        let adapter = Arc::new(
            PiClarificationAdapter::new(
                RepositoryInspector::new(
                    directory.path().join("cache"),
                    directory.path().join("workspaces"),
                )?,
                directory.path().join("sessions"),
            )?
            .with_agent_command(fake_pi(directory.path())?),
        );
        let entered = Arc::new(AtomicBool::new(false));
        let observed_workspace = Arc::new(Mutex::new(None));
        let release = Arc::new(AtomicBool::new(false));
        {
            let entered = entered.clone();
            let observed_workspace = observed_workspace.clone();
            let release = release.clone();
            adapter.set_preparation_checkpoint(move |_repository_id, path| {
                {
                    let mut workspace = match observed_workspace.lock() {
                        Ok(workspace) => workspace,
                        Err(_) => panic!("workspace checkpoint"),
                    };
                    *workspace = Some(path.to_owned());
                }
                entered.store(true, Ordering::Release);
                while !release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            });
        }
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let scheduler = crate::scheduler::RuntimeScheduler::new(adapter, sender);
        let start = CommandEnvelope {
            command_id: "command-prep".into(),
            session_id: "session-prep".into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: start_command(
                "repository-cancel",
                authorized.to_string_lossy().into_owned(),
            ),
        };
        scheduler.schedule(start)?;
        tokio::time::timeout(Duration::from_secs(30), async {
            while !entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "preparation checkpoint not reached")?;
        let workspace = observed_workspace
            .lock()
            .expect("workspace checkpoint")
            .clone()
            .ok_or("missing workspace checkpoint")?;
        assert!(!directory.path().join("pi-started").exists());
        scheduler.schedule(CommandEnvelope {
            command_id: "cancel-prep".into(),
            session_id: "session-prep".into(),
            server_command_seq: 2,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::SessionCancel(SessionCancel {
                reason: "requester_cancelled".into(),
            }),
        })?;
        release.store(true, Ordering::Release);
        let completion = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| "runtime completion timeout")?
            .ok_or("runtime completion channel closed")?;
        assert_eq!(completion.command_id, "command-prep");
        assert!(matches!(
            completion.events.as_slice(),
            [Event::SessionCompleted(_)]
        ));
        assert!(!completion
            .events
            .iter()
            .any(|event| matches!(event, Event::RequirementAssessed(_))));
        let finished = scheduler.finish_active(&completion)?;
        let followup = match finished.followup {
            Some(crate::scheduler::RuntimeFollowup::FinishCancellation(command)) => command,
            other => return Err(format!("unexpected cancellation follow-up: {other:?}").into()),
        };
        scheduler.finish_followup(&followup)?;
        assert!(!workspace.exists());
        assert!(!directory.path().join("pi-started").exists());
        assert!(receiver.try_recv().is_err());
        assert_eq!(scheduler.control_count(), 0);
        let _ = fs::read_dir(directory.path().join("workspaces"))?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_during_git_preparation_kills_git_before_prepared_workspace(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            net::{Shutdown, TcpListener},
            sync::{
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
        };

        let directory = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (accepted_sender, accepted_receiver) = mpsc::channel();
        let release_server = Arc::new(AtomicBool::new(false));
        let server_release = release_server.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("Git connection");
            accepted_sender
                .send(())
                .expect("Git connection notification");
            while !server_release.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(10));
            }
            let _ = stream.shutdown(Shutdown::Both);
        });

        let prepared = Arc::new(AtomicBool::new(false));
        let adapter = Arc::new(
            PiClarificationAdapter::new(
                RepositoryInspector::new(
                    directory.path().join("cache"),
                    directory.path().join("workspaces"),
                )?,
                directory.path().join("sessions"),
            )?
            .with_agent_command(fake_pi(directory.path())?),
        );
        let prepared_flag = prepared.clone();
        adapter.set_preparation_checkpoint(move |_repository_id, _path| {
            prepared_flag.store(true, Ordering::Release);
        });

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let scheduler = crate::scheduler::RuntimeScheduler::new(adapter.clone(), sender);
        scheduler.schedule(CommandEnvelope {
            command_id: "command-git-prep".into(),
            session_id: "session-git-prep".into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: start_command("repository-git-prep", format!("http://{address}/repo.git")),
        })?;
        tokio::task::spawn_blocking(move || {
            accepted_receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| std::io::Error::other(format!("Git did not connect: {error}")))
        })
        .await??;

        scheduler.schedule(CommandEnvelope {
            command_id: "cancel-git-prep".into(),
            session_id: "session-git-prep".into(),
            server_command_seq: 2,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::SessionCancel(SessionCancel {
                reason: "requester_cancelled".into(),
            }),
        })?;
        let completion = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .map_err(|_| "Git preparation cancellation timed out")?
            .ok_or("runtime completion channel closed")?;
        release_server.store(true, Ordering::Release);
        server.join().expect("Git server thread");

        assert_eq!(completion.command_id, "command-git-prep");
        assert!(matches!(
            completion.events.as_slice(),
            [Event::SessionCompleted(_)]
        ));
        assert!(!prepared.load(Ordering::Acquire));
        assert!(!directory.path().join("pi-started").exists());
        assert!(fs::read_dir(directory.path().join("workspaces"))?
            .next()
            .is_none());
        let finished = scheduler.finish_active(&completion)?;
        let followup = match finished.followup {
            Some(crate::scheduler::RuntimeFollowup::FinishCancellation(command)) => command,
            other => return Err(format!("unexpected cancellation follow-up: {other:?}").into()),
        };
        scheduler.finish_followup(&followup)?;
        assert_eq!(scheduler.control_count(), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn bounded_evidence_skips_oversized_binary_and_truncated_listing(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::process::Command as GitCommand;

        let directory = tempfile::tempdir()?;
        let (repository, _) = git_fixture(
            directory.path(),
            "bounded-evidence-repository",
            "NORTH_AUTHORIZED_REPOSITORY_MARKER",
        )?;
        fs::write(repository.join("src/binary.bin"), [0_u8, 1, 2, 3])?;
        fs::write(
            repository.join("src/oversized.txt"),
            vec![b'x'; MAX_EVIDENCE_FILE_BYTES + 1],
        )?;
        fs::create_dir_all(repository.join("zz"))?;
        for index in 0..5_000 {
            fs::write(
                repository.join("zz").join(format!("file-{index:04}.txt")),
                b"listing filler",
            )?;
        }
        let output = GitCommand::new("git")
            .current_dir(&repository)
            .args(["add", "."])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("stage bounded evidence fixture").into());
        }
        let output = GitCommand::new("git")
            .current_dir(&repository)
            .args(["commit", "--quiet", "-m", "bounded evidence"])
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("commit bounded evidence fixture").into());
        }

        let inspector = RepositoryInspector::new(
            directory.path().join("cache"),
            directory.path().join("workspaces"),
        )?;
        let source = RepositorySource::new(
            "bounded-evidence",
            repository.to_string_lossy().into_owned(),
        );
        let authorization = RunAuthorization::new("session-bounded", vec![source.clone()])?;
        let request = InspectionRequest::new("session-bounded", "task-bounded", source);
        let workspace = inspector.prepare_authorized(&request, &authorization)?;
        let cancellation = InspectionCancellation::new();
        let listing = workspace.read_git_bounded_with_cancellation(
            &authorization,
            &["ls-files".into(), "--cached".into(), "-z".into()],
            MAX_EVIDENCE_LIST_BYTES,
            &cancellation,
        )?;
        assert!(listing.truncated, "fixture must exceed listing bound");
        assert!(listing.bytes.len() <= MAX_EVIDENCE_LIST_BYTES);
        let binary = workspace.read_git_bounded_with_cancellation(
            &authorization,
            &[
                "show".into(),
                "--format=".into(),
                "--no-ext-diff".into(),
                format!("{}:src/binary.bin", workspace.commit_sha()),
            ],
            MAX_EVIDENCE_FILE_BYTES,
            &cancellation,
        )?;
        assert!(binary.bytes.contains(&0));
        let oversized = workspace.read_git_bounded_with_cancellation(
            &authorization,
            &[
                "show".into(),
                "--format=".into(),
                "--no-ext-diff".into(),
                format!("{}:src/oversized.txt", workspace.commit_sha()),
            ],
            MAX_EVIDENCE_FILE_BYTES,
            &cancellation,
        )?;
        assert!(oversized.truncated);

        let control = RuntimeControl::new("session-bounded", "task-bounded");
        let result =
            extract_repository_evidence(&workspace, &authorization, &control, &cancellation);
        inspector.dispose(workspace)?;
        let evidence = result.map_err(std::io::Error::other)?;
        assert!(evidence
            .files
            .iter()
            .any(|file| file.path == "src/domain.rs"));
        assert!(!evidence
            .files
            .iter()
            .any(|file| file.path == "src/binary.bin"));
        assert!(!evidence
            .files
            .iter()
            .any(|file| file.path == "src/oversized.txt"));
        Ok(())
    }
}
