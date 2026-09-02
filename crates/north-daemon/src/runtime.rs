//! North-owned clarification runtime seam and Pi Agent adapter.
//!
//! The seam carries only immutable North context and North-neutral facts. Pi
//! process/session details stay in `PiClarificationAdapter`; this module never
//! writes Requirement state or server persistence.

use crate::repository_inspection::{
    InspectionError, InspectionRequest, RepositoryInspector, RepositorySource, RunAuthorization,
};
use north_protocol::{
    AgentActivity, AgentMessage, Command, Event, MessageSend, ReadinessVerdictWire,
    RepositoryContext, RequirementAssessed, ReviewedRepositoryWire, SessionCompleted,
    SessionFailed, SessionStarted,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const MAX_PI_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SESSION_CONTEXT_BYTES: usize = 128 * 1024;
const PI_PROCESS_TIMEOUT: Duration = Duration::from_secs(120);
const PI_PROCESS_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const PI_SYSTEM_PROMPT: &str = "You are North's requirement clarification agent. Use only context in this prompt. Do not claim repository facts not supplied. Return only one JSON object with keys message, verdict, blockers, assumptions. verdict must be ready or needs_clarification. Keep message concise and user-facing. Do not include reasoning, tool output, or markdown.";

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssessmentContext {
    requirement_id: String,
    requirement_revision: u64,
    repositories_reviewed: Vec<RepositoryReview>,
    context: Option<StartInput>,
    #[serde(skip)]
    in_flight: bool,
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

    fn start(
        &self,
        input: &RuntimeInput,
        start: StartInput,
    ) -> Result<Vec<RuntimeFact>, RuntimeError> {
        let sources = start
            .repositories
            .iter()
            .map(|repository| {
                RepositorySource::from_context(&RepositoryContext {
                    repository_id: repository.repository_id.clone(),
                    name: repository.name.clone(),
                    url: repository.url.clone(),
                    description: repository.description.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let authorization = RunAuthorization::new(input.session_id.clone(), sources.clone())?;
        let mut reviewed = Vec::with_capacity(start.repositories.len());
        for source in sources {
            let request = InspectionRequest::new(&input.session_id, &input.operation_id, source);
            let inspection = self
                .inspector
                .inspect(&request, &authorization, |_| Ok(()))?;
            reviewed.push(RepositoryReview {
                repository_id: inspection.repository_id,
                commit_sha: inspection.commit_sha,
            });
        }

        let mut assessment = AssessmentContext {
            requirement_id: start.requirement.id.clone(),
            requirement_revision: start.requirement.revision,
            repositories_reviewed: reviewed.clone(),
            context: Some(start.clone()),
            in_flight: true,
        };
        self.persist_context(&input.session_id, &assessment)?;
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
            .insert(input.session_id.clone(), assessment.clone());
        let prompt = start_prompt(&start, &reviewed);
        let response = match self.invoke_pi(&input.session_id, &prompt, None) {
            Ok(response) => response,
            Err(error) => {
                if let Err(cleanup) = self.clear_session(&input.session_id) {
                    return Err(RuntimeError::new(format!("{error}; {cleanup}")));
                }
                return Err(error);
            }
        };
        let parsed = parse_assessment(&response);
        assessment.in_flight = false;
        let completes = parsed.verdict == ReadinessVerdict::Ready;
        if completes {
            self.clear_context(&input.session_id)?;
        }
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
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
            self.clear_session(&input.session_id)?;
        }
        Ok(facts)
    }

    fn message(
        &self,
        input: &RuntimeInput,
        message: MessageSend,
    ) -> Result<Vec<RuntimeFact>, RuntimeError> {
        let context = self.load_context(&input.session_id)?;
        let prompt = message_prompt(context.as_ref(), &message)?;
        let mut running_context = context.ok_or_else(|| {
            RuntimeError::new("Pi clarification message has no active session context")
        })?;
        if running_context.in_flight {
            return Err(RuntimeError::new(
                "Pi clarification session is already running",
            ));
        }
        running_context.in_flight = true;
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
            .insert(input.session_id.clone(), running_context.clone());
        let response = match self.invoke_pi(&input.session_id, &prompt, None) {
            Ok(response) => response,
            Err(error) => {
                if let Err(cleanup) = self.clear_session(&input.session_id) {
                    return Err(RuntimeError::new(format!("{error}; {cleanup}")));
                }
                return Err(error);
            }
        };
        running_context.in_flight = false;
        self.sessions
            .lock()
            .map_err(|_| RuntimeError::new("Pi session state lock poisoned"))?
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
            self.clear_session(&input.session_id)?;
        }
        Ok(facts)
    }

    fn cancel(&self, input: &RuntimeInput, reason: &str) -> Result<Vec<RuntimeFact>, RuntimeError> {
        // Pi invocation is synchronous and waits for its child. Retained state
        // represents a live clarification turn, not an in-flight child.
        let Some(context) = self.load_context(&input.session_id)? else {
            return Err(RuntimeError::new(
                "Pi clarification session cancellation was not confirmed",
            ));
        };
        if context.in_flight {
            return Err(RuntimeError::new(
                "Pi clarification cancellation could not interrupt active execution",
            ));
        }
        self.clear_session(&input.session_id)?;
        Ok(vec![RuntimeFact::Completed {
            summary: format!("Pi clarification cancelled: {reason}"),
        }])
    }

    fn invoke_pi(
        &self,
        session_id: &str,
        prompt: &str,
        checkout: Option<&Path>,
    ) -> Result<String, RuntimeError> {
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
        if let Some(checkout) = checkout {
            command.current_dir(checkout);
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| RuntimeError::new(format!("run Pi Agent: {error}")))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            stop_process(&mut child);
            RuntimeError::new("Pi Agent stdout pipe was unavailable")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            stop_process(&mut child);
            RuntimeError::new("Pi Agent stderr pipe was unavailable")
        })?;
        let stdout_reader = thread::spawn(|| read_process_output(stdout));
        let stderr_reader = thread::spawn(|| read_process_output(stderr));
        let deadline = Instant::now() + PI_PROCESS_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    stop_process(&mut child);
                    return Err(RuntimeError::new("Pi Agent process timed out"));
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    stop_process(&mut child);
                    return Err(RuntimeError::new(format!("wait for Pi Agent: {error}")));
                }
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| RuntimeError::new("read Pi Agent stdout panicked"))?
            .map_err(|error| RuntimeError::new(format!("read Pi Agent stdout: {error}")))?;
        let _stderr = stderr_reader
            .join()
            .map_err(|_| RuntimeError::new("read Pi Agent stderr panicked"))?
            .map_err(|error| RuntimeError::new(format!("read Pi Agent stderr: {error}")))?;
        if !status.success() {
            return Err(RuntimeError::new(format!("Pi Agent exited with {status}")));
        }
        if stdout.len() > MAX_PI_OUTPUT_BYTES {
            return Err(RuntimeError::new(
                "Pi Agent response exceeded bounded output",
            ));
        }
        extract_assistant_text(&stdout)
    }
}

fn stop_process(child: &mut Child) {
    let _ = child.kill();
    let deadline = Instant::now() + PI_PROCESS_REAP_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn read_process_output<R: Read>(reader: R) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take(MAX_PI_OUTPUT_BYTES as u64 + 1)
        .read_to_end(&mut output)?;
    Ok(output)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StartInput {
    requirement: RequirementSnapshot,
    conversation: Vec<ConversationTurn>,
    repositories: Vec<AuthorizedRepository>,
}

impl ClarificationRuntime for PiClarificationAdapter {
    fn dispatch(&self, input: RuntimeInput) -> Result<Vec<RuntimeFact>, RuntimeError> {
        match input.command.clone() {
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
            ),
            RuntimeCommand::Cancel { reason } => self.cancel(&input, &reason),
            RuntimeCommand::Resume => Ok(Vec::new()),
        }
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
        let input = RuntimeInput {
            operation_id: runtime_operation_id.to_owned(),
            session_id: session_id.to_owned(),
            command: command_to_runtime(command),
        };
        let result = ClarificationRuntime::dispatch(self, input);
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
            Err(error) => {
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

fn start_prompt(start: &StartInput, reviewed: &[RepositoryReview]) -> String {
    let context = serde_json::json!({
        "requirement": start.requirement,
        "conversation": start.conversation,
        "authorized_repositories": start.repositories.iter().map(|repository| serde_json::json!({
            "repository_id": repository.repository_id,
        })).collect::<Vec<_>>(),
        "repository_revisions_reviewed": reviewed,
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
        start_prompt(start, &context.repositories_reviewed),
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
}
