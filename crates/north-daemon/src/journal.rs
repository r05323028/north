//! Crash-safe daemon-side command inbox and event journal.
//!
//! This module deliberately owns only local delivery/recovery mechanics. The
//! runtime seam receives stable operation identity; it does not decide North
//! business retry or Requirement state.

use north_protocol::{
    Command, CommandAck, CommandEnvelope, DaemonFrame, Event, EventAck, EventAckStatus,
    EventEnvelope, ReconcileSnapshot, ServerFrame, SessionReconcileState, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub const MAX_GAP_BUFFER_ENTRIES_PER_SESSION: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalConfig {
    pub max_gap_buffer_entries_per_session: usize,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            max_gap_buffer_entries_per_session: MAX_GAP_BUFFER_ENTRIES_PER_SESSION,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    Io(String),
    Format(String),
    Protocol(String),
    IdentityConflict(String),
    GapOverflow { session_id: String, limit: usize },
    SequenceGap { expected: u64, received: u64 },
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "daemon journal I/O error: {reason}"),
            Self::Format(reason) => write!(f, "daemon journal format error: {reason}"),
            Self::Protocol(reason) => write!(f, "daemon protocol error: {reason}"),
            Self::IdentityConflict(reason) => write!(f, "daemon identity conflict: {reason}"),
            Self::GapOverflow { session_id, limit } => {
                write!(f, "session {session_id} exceeded gap buffer limit {limit}")
            }
            Self::SequenceGap { expected, received } => {
                write!(f, "sequence gap: expected {expected}, received {received}")
            }
        }
    }
}

impl Error for JournalError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandJournalState {
    Received,
    DispatchStarted,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "reason")]
pub enum DispatchOutcome {
    DispatchSucceeded,
    DispatchFailed(String),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    DispatchSucceeded,
    DispatchFailed(String),
    Unknown,
}

pub trait RuntimeExecutor: Send + Sync {
    /// Cross the side-effecting runtime boundary once, using the command ID as
    /// the stable runtime operation identity.
    fn dispatch(
        &self,
        runtime_operation_id: &str,
        command_id: &str,
        command: &Command,
    ) -> DispatchOutcome;

    /// Resolve an operation after a crash that occurred after dispatch_started.
    /// Returning Unknown is safe: the coordinator will never blindly invoke it.
    fn recover(
        &self,
        runtime_operation_id: &str,
        command_id: &str,
        command: &Command,
    ) -> RecoveryOutcome;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandProcessResult {
    pub acknowledgement: CommandAck,
    pub state: CommandJournalState,
    pub outcome: Option<DispatchOutcome>,
    pub duplicate: bool,
    pub buffered: bool,
    pub emitted_events: Vec<EventEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedEvent {
    pub envelope: EventEnvelope,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalSnapshot {
    pub command_count: usize,
    pub command_tombstone_count: usize,
    pub event_count: usize,
    pub event_tombstone_count: usize,
    pub processed_through: BTreeMap<String, u64>,
    pub event_ack_through: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalFile {
    format_version: u16,
    sessions: BTreeMap<String, SessionJournal>,
    commands: Vec<CommandRecord>,
    command_tombstones: Vec<CommandTombstone>,
    events: Vec<EventRecord>,
    event_tombstones: Vec<EventTombstone>,
}

impl Default for JournalFile {
    fn default() -> Self {
        Self {
            format_version: 1,
            sessions: BTreeMap::new(),
            commands: Vec::new(),
            command_tombstones: Vec::new(),
            events: Vec::new(),
            event_tombstones: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionJournal {
    processed_through_seq: u64,
    event_ack_through_seq: u64,
    event_ack_sparse: Vec<u64>,
    server_command_ack_through_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandRecord {
    daemon_id: String,
    command_id: String,
    session_id: String,
    server_command_seq: u64,
    #[serde(default)]
    sent_at: String,
    payload_digest: String,
    command: Command,
    message_id: Option<String>,
    state: CommandJournalState,
    outcome: Option<DispatchOutcome>,
    runtime_operation_id: String,
    reason: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommandTombstone {
    daemon_id: String,
    command_id: String,
    session_id: String,
    server_command_seq: u64,
    payload_digest: String,
    #[serde(default)]
    message_id: Option<String>,
    outcome: DispatchOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventRecord {
    daemon_id: String,
    envelope: EventEnvelope,
    payload_digest: String,
    acknowledged: bool,
    acknowledgement: Option<EventAckStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventTombstone {
    daemon_id: String,
    event_id: String,
    session_id: String,
    daemon_event_seq: u64,
    payload_digest: String,
    #[serde(default)]
    event_identity_digest: String,
    #[serde(default)]
    acknowledgement: Option<EventAckStatus>,
}

#[derive(Clone)]
pub struct Journal {
    daemon_id: String,
    path: Arc<PathBuf>,
    state: Arc<Mutex<JournalFile>>,
    config: JournalConfig,
}

impl fmt::Debug for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Journal")
            .field("daemon_id", &self.daemon_id)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Journal {
    pub fn open(
        path: impl AsRef<Path>,
        daemon_id: impl Into<String>,
    ) -> Result<Self, JournalError> {
        Self::open_with_config(path, daemon_id, JournalConfig::default())
    }

    pub fn open_with_config(
        path: impl AsRef<Path>,
        daemon_id: impl Into<String>,
        config: JournalConfig,
    ) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let daemon_id = daemon_id.into();
        let state = if path.exists() {
            let bytes = fs::read(&path).map_err(io_error)?;
            serde_json::from_slice(&bytes)
                .map_err(|error| JournalError::Format(error.to_string()))?
        } else {
            JournalFile::default()
        };
        if state.format_version != 1 {
            return Err(JournalError::Format(format!(
                "unsupported format version {}",
                state.format_version
            )));
        }
        let wrong_identity = state
            .commands
            .iter()
            .any(|record| record.daemon_id != daemon_id)
            || state
                .command_tombstones
                .iter()
                .any(|record| record.daemon_id != daemon_id)
            || state
                .events
                .iter()
                .any(|record| record.daemon_id != daemon_id)
            || state
                .event_tombstones
                .iter()
                .any(|record| record.daemon_id != daemon_id);
        if wrong_identity {
            return Err(JournalError::Format(
                "journal belongs to another daemon identity".into(),
            ));
        }
        let journal = Self {
            daemon_id,
            path: Arc::new(path),
            state: Arc::new(Mutex::new(state)),
            config,
        };
        if !journal.path.exists() {
            let state = journal.state.lock().map_err(lock_error)?;
            journal.persist_locked(&state)?;
        } else {
            journal.set_private_permissions()?;
        }
        Ok(journal)
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn snapshot(&self) -> Result<JournalSnapshot, JournalError> {
        let state = self.state.lock().map_err(lock_error)?;
        Ok(JournalSnapshot {
            command_count: state.commands.len(),
            command_tombstone_count: state.command_tombstones.len(),
            event_count: state.events.len(),
            event_tombstone_count: state.event_tombstones.len(),
            processed_through: state
                .sessions
                .iter()
                .map(|(id, session)| (id.clone(), session.processed_through_seq))
                .collect(),
            event_ack_through: state
                .sessions
                .iter()
                .map(|(id, session)| (id.clone(), session.event_ack_through_seq))
                .collect(),
        })
    }

    pub fn process_command<E: RuntimeExecutor + ?Sized>(
        &self,
        envelope: CommandEnvelope,
        executor: &E,
    ) -> Result<CommandProcessResult, JournalError> {
        ServerFrame::Command(envelope.clone())
            .validate()
            .map_err(|error| JournalError::Protocol(error.to_string()))?;
        let digest = payload_digest(&envelope)?;
        let message_id = message_id(&envelope.command);
        let (record, duplicate, buffered) = self.prepare_command(&envelope, &digest, message_id)?;
        let acknowledgement = command_ack(&record);
        if record.state != CommandJournalState::Received || buffered {
            return Ok(CommandProcessResult {
                acknowledgement,
                state: record.state,
                outcome: record.outcome,
                duplicate,
                buffered,
                emitted_events: Vec::new(),
            });
        }

        if !self.mark_dispatch_started(&record.command_id)? {
            let current = self.command_record(&record.command_id)?;
            return Ok(CommandProcessResult {
                acknowledgement: command_ack(&current),
                state: current.state,
                outcome: current.outcome,
                duplicate: true,
                buffered: false,
                emitted_events: Vec::new(),
            });
        }
        let outcome = executor.dispatch(
            &record.runtime_operation_id,
            &record.command_id,
            &record.command,
        );
        let emitted_events = self.finish_command(&record.command_id, outcome.clone())?;
        self.compact_session(&record.session_id)?;
        Ok(CommandProcessResult {
            acknowledgement,
            state: CommandJournalState::Terminal,
            outcome: Some(outcome),
            duplicate,
            buffered: false,
            emitted_events,
        })
    }

    /// Process durable received commands whose per-session gap is now closed.
    pub fn process_ready<E: RuntimeExecutor + ?Sized>(
        &self,
        session_id: &str,
        executor: &E,
    ) -> Result<Vec<CommandProcessResult>, JournalError> {
        let mut results = Vec::new();
        loop {
            let next = {
                let state = self.state.lock().map_err(lock_error)?;
                let expected = expected_command_sequence(&state, session_id);
                state
                    .commands
                    .iter()
                    .find(|record| {
                        record.session_id == session_id
                            && record.server_command_seq == expected
                            && record.state == CommandJournalState::Received
                    })
                    .map(record_to_envelope)
                    .transpose()?
            };
            let Some(command) = next else {
                break;
            };
            results.push(self.process_command(command, executor)?);
        }
        Ok(results)
    }

    /// Recover records after daemon restart. `received` records may continue;
    /// `dispatch_started` records use runtime status recovery only.
    pub fn recover<E: RuntimeExecutor + ?Sized>(
        &self,
        executor: &E,
    ) -> Result<Vec<CommandProcessResult>, JournalError> {
        let records = {
            let state = self.state.lock().map_err(lock_error)?;
            state
                .commands
                .iter()
                .filter(|record| record.state != CommandJournalState::Terminal)
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut results = Vec::new();
        for record in records {
            if record.state == CommandJournalState::Received {
                results.push(self.process_command(record_to_envelope(&record)?, executor)?);
                continue;
            }
            let outcome = match executor.recover(
                &record.runtime_operation_id,
                &record.command_id,
                &record.command,
            ) {
                RecoveryOutcome::DispatchSucceeded => DispatchOutcome::DispatchSucceeded,
                RecoveryOutcome::DispatchFailed(reason) => DispatchOutcome::DispatchFailed(reason),
                RecoveryOutcome::Unknown => {
                    DispatchOutcome::Unknown(unknown_reason(&record.command_id))
                }
            };
            let emitted_events = self.finish_command(&record.command_id, outcome.clone())?;
            self.compact_session(&record.session_id)?;
            results.push(CommandProcessResult {
                acknowledgement: command_ack(&record),
                state: CommandJournalState::Terminal,
                outcome: Some(outcome),
                duplicate: false,
                buffered: false,
                emitted_events,
            });
        }
        // A recovered sequence-1 command can release buffered later commands.
        for session_id in session_ids(&self.state)? {
            results.extend(self.process_ready(&session_id, executor)?);
        }
        Ok(results)
    }

    pub fn append_event(
        &self,
        event_id: impl Into<String>,
        session_id: impl Into<String>,
        event: Event,
    ) -> Result<AppendedEvent, JournalError> {
        let event_id = event_id.into();
        let session_id = session_id.into();
        self.update(|state| {
            if let Some(existing) = state.events.iter().find(|record| {
                record.daemon_id == self.daemon_id && record.envelope.event_id == event_id
            }) {
                if existing.envelope.session_id != session_id || existing.envelope.event != event {
                    return Err(JournalError::IdentityConflict(
                        "event ID is already bound to different event data".into(),
                    ));
                }
                return Ok(AppendedEvent {
                    envelope: existing.envelope.clone(),
                    duplicate: true,
                });
            }
            if let Some(existing) = state
                .event_tombstones
                .iter()
                .find(|record| record.daemon_id == self.daemon_id && record.event_id == event_id)
            {
                if existing.session_id != session_id {
                    return Err(JournalError::IdentityConflict(
                        "event ID tombstone belongs to another session".into(),
                    ));
                }
                let candidate = EventEnvelope {
                    event_id: event_id.clone(),
                    session_id: session_id.clone(),
                    daemon_event_seq: existing.daemon_event_seq,
                    sent_at: String::new(),
                    schema_version: SCHEMA_VERSION,
                    event: event.clone(),
                };
                if existing.event_identity_digest != event_identity_digest(&candidate)? {
                    return Err(JournalError::IdentityConflict(
                        "event ID tombstone payload conflict".into(),
                    ));
                }
                return Ok(AppendedEvent {
                    envelope: EventEnvelope {
                        event_id: event_id.clone(),
                        session_id: session_id.clone(),
                        daemon_event_seq: existing.daemon_event_seq,
                        sent_at: now_string(),
                        schema_version: SCHEMA_VERSION,
                        event: event.clone(),
                    },
                    duplicate: true,
                });
            }
            let envelope = EventEnvelope {
                event_id,
                session_id: session_id.clone(),
                daemon_event_seq: next_event_sequence(state, &session_id),
                sent_at: now_string(),
                schema_version: SCHEMA_VERSION,
                event,
            };
            DaemonFrame::Event(envelope.clone())
                .validate()
                .map_err(|error| JournalError::Protocol(error.to_string()))?;
            let digest = payload_digest(&envelope)?;
            validate_event_identity(state, &self.daemon_id, &envelope, &digest)?;
            state.events.push(EventRecord {
                daemon_id: self.daemon_id.clone(),
                envelope: envelope.clone(),
                payload_digest: digest,
                acknowledged: false,
                acknowledgement: None,
            });
            Ok(AppendedEvent {
                envelope,
                duplicate: false,
            })
        })
    }

    pub fn append_event_envelope(
        &self,
        envelope: EventEnvelope,
        digest: String,
        duplicate: bool,
    ) -> Result<AppendedEvent, JournalError> {
        DaemonFrame::Event(envelope.clone())
            .validate()
            .map_err(|error| JournalError::Protocol(error.to_string()))?;
        let actual_digest = payload_digest(&envelope)?;
        if actual_digest != digest {
            return Err(JournalError::IdentityConflict(
                "event payload digest does not match envelope".into(),
            ));
        }
        self.update(|state| {
            validate_event_identity(state, &self.daemon_id, &envelope, &digest)?;
            if let Some(existing) = state
                .events
                .iter()
                .find(|record| record.envelope.event_id == envelope.event_id)
            {
                return Ok(AppendedEvent {
                    envelope: existing.envelope.clone(),
                    duplicate: true,
                });
            }
            if state
                .event_tombstones
                .iter()
                .any(|record| record.event_id == envelope.event_id)
            {
                return Ok(AppendedEvent {
                    envelope: envelope.clone(),
                    duplicate: true,
                });
            }
            let expected = next_event_sequence(state, &envelope.session_id);
            if envelope.daemon_event_seq != expected {
                return Err(JournalError::IdentityConflict(format!(
                    "event sequence {} is not next allocated sequence {expected}",
                    envelope.daemon_event_seq
                )));
            }
            state.events.push(EventRecord {
                daemon_id: self.daemon_id.clone(),
                envelope: envelope.clone(),
                payload_digest: digest,
                acknowledged: false,
                acknowledgement: None,
            });
            Ok(AppendedEvent {
                envelope,
                duplicate,
            })
        })
    }

    pub fn acknowledge_event(&self, ack: EventAck) -> Result<(), JournalError> {
        ServerFrame::EventAck(ack.clone())
            .validate()
            .map_err(|error| JournalError::Protocol(error.to_string()))?;
        self.update(|state| {
            let record = state
                .events
                .iter_mut()
                .find(|record| record.envelope.event_id == ack.event_id);
            let Some(record) = record else {
                if let Some(tombstone) = state
                    .event_tombstones
                    .iter_mut()
                    .find(|record| record.event_id == ack.event_id)
                {
                    if tombstone.session_id != ack.session_id
                        || tombstone.daemon_event_seq != ack.daemon_event_seq
                        || tombstone
                            .acknowledgement
                            .as_ref()
                            .is_some_and(|status| status != &ack.status)
                    {
                        return Err(JournalError::IdentityConflict(
                            "late event ACK conflicts with tombstone".into(),
                        ));
                    }
                    if tombstone.acknowledgement.is_none() {
                        tombstone.acknowledgement = Some(ack.status.clone());
                    }
                    return Ok(());
                }
                return Err(JournalError::IdentityConflict(
                    "event ACK references unknown event".into(),
                ));
            };
            if record.envelope.session_id != ack.session_id
                || record.envelope.daemon_event_seq != ack.daemon_event_seq
            {
                return Err(JournalError::IdentityConflict(
                    "event ACK identity conflicts with journal".into(),
                ));
            }
            if record.acknowledged {
                if let Some(status) = record.acknowledgement.as_ref() {
                    if status != &ack.status {
                        return Err(JournalError::IdentityConflict(
                            "event ACK status conflicts with retained terminal outcome".into(),
                        ));
                    }
                } else {
                    record.acknowledgement = Some(ack.status.clone());
                }
                return Ok(());
            }
            record.acknowledged = true;
            record.acknowledgement = Some(ack.status.clone());
            let session = state.sessions.entry(ack.session_id.clone()).or_default();
            if ack.daemon_event_seq > session.event_ack_through_seq {
                session.event_ack_sparse.push(ack.daemon_event_seq);
                session.event_ack_sparse.sort_unstable();
                session.event_ack_sparse.dedup();
            }
            advance_event_ack(state, &ack.session_id);
            Ok(())
        })
    }

    pub fn apply_reconciliation(&self, snapshot: ReconcileSnapshot) -> Result<(), JournalError> {
        ServerFrame::Reconcile(snapshot.clone())
            .validate()
            .map_err(|error| JournalError::Protocol(error.to_string()))?;
        self.update(|state| {
            for session in snapshot.sessions {
                let local = state
                    .sessions
                    .entry(session.session_id.clone())
                    .or_default();
                local.server_command_ack_through_seq = local
                    .server_command_ack_through_seq
                    .max(session.command_ack_through_seq);
                local.event_ack_through_seq = local
                    .event_ack_through_seq
                    .max(session.event_ack_through_seq);
                local.event_ack_sparse =
                    merge_sparse(&local.event_ack_sparse, &session.event_ack_sparse);
                local
                    .event_ack_sparse
                    .retain(|sequence| *sequence > local.event_ack_through_seq);
                for record in &mut state.events {
                    if record.envelope.session_id == session.session_id
                        && (record.envelope.daemon_event_seq <= local.event_ack_through_seq
                            || local
                                .event_ack_sparse
                                .contains(&record.envelope.daemon_event_seq))
                    {
                        record.acknowledged = true;
                    }
                }
            }
            Ok(())
        })
    }

    pub fn replay_events(
        &self,
        session_id: &str,
        state: &SessionReconcileState,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        ServerFrame::Reconcile(ReconcileSnapshot {
            schema_version: SCHEMA_VERSION,
            sessions: vec![state.clone()],
        })
        .validate()
        .map_err(|error| JournalError::Protocol(error.to_string()))?;
        let state_file = self.state.lock().map_err(lock_error)?;
        let mut events = state_file
            .events
            .iter()
            .filter(|record| {
                record.envelope.session_id == session_id
                    && record.envelope.daemon_event_seq > state.event_ack_through_seq
                    && !state
                        .event_ack_sparse
                        .contains(&record.envelope.daemon_event_seq)
                    && !record.acknowledged
            })
            .map(|record| record.envelope.clone())
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.daemon_event_seq);
        Ok(events)
    }

    pub fn pending_commands(&self, session_id: &str) -> Result<Vec<CommandEnvelope>, JournalError> {
        let state = self.state.lock().map_err(lock_error)?;
        let mut commands = state
            .commands
            .iter()
            .filter(|record| {
                record.session_id == session_id && record.state != CommandJournalState::Terminal
            })
            .map(record_to_envelope)
            .collect::<Result<Vec<_>, _>>()?;
        commands.sort_by_key(|command| command.server_command_seq);
        Ok(commands)
    }

    /// Replace terminal command/event payloads with identity tombstones only
    /// after durable delivery boundaries. Tombstones have no time-based expiry.
    pub fn compact_session(&self, session_id: &str) -> Result<(), JournalError> {
        self.update(|state| {
            let processed = state
                .sessions
                .get(session_id)
                .map(|session| session.processed_through_seq)
                .unwrap_or_default();
            let mut retained_commands = Vec::with_capacity(state.commands.len());
            for record in state.commands.drain(..) {
                if record.session_id == session_id
                    && record.state == CommandJournalState::Terminal
                    && record.server_command_seq <= processed
                {
                    state.command_tombstones.push(CommandTombstone {
                        daemon_id: record.daemon_id,
                        command_id: record.command_id,
                        session_id: record.session_id,
                        server_command_seq: record.server_command_seq,
                        payload_digest: record.payload_digest,
                        message_id: record.message_id,
                        outcome: record.outcome.unwrap_or_else(|| {
                            DispatchOutcome::Unknown("terminal outcome missing".into())
                        }),
                    });
                } else {
                    retained_commands.push(record);
                }
            }
            state.commands = retained_commands;

            let (event_ack_through, event_sparse) = state
                .sessions
                .get(session_id)
                .map(|session| {
                    (
                        session.event_ack_through_seq,
                        session.event_ack_sparse.clone(),
                    )
                })
                .unwrap_or_default();
            let mut retained_events = Vec::with_capacity(state.events.len());
            for record in state.events.drain(..) {
                if record.envelope.session_id == session_id
                    && record.acknowledged
                    && (record.envelope.daemon_event_seq <= event_ack_through
                        || event_sparse.contains(&record.envelope.daemon_event_seq))
                {
                    let identity_digest = event_identity_digest(&record.envelope)?;
                    state.event_tombstones.push(EventTombstone {
                        daemon_id: record.daemon_id,
                        event_id: record.envelope.event_id,
                        session_id: record.envelope.session_id,
                        daemon_event_seq: record.envelope.daemon_event_seq,
                        payload_digest: record.payload_digest,
                        event_identity_digest: identity_digest,
                        acknowledgement: record.acknowledgement,
                    });
                } else {
                    retained_events.push(record);
                }
            }
            state.events = retained_events;
            Ok(())
        })
    }

    fn prepare_command(
        &self,
        envelope: &CommandEnvelope,
        digest: &str,
        message_id: Option<String>,
    ) -> Result<(CommandRecord, bool, bool), JournalError> {
        self.update(|state| {
            validate_command_identity(
                state,
                &self.daemon_id,
                envelope,
                digest,
                message_id.as_deref(),
            )?;
            if let Some(existing) = state
                .commands
                .iter()
                .find(|record| record.command_id == envelope.command_id)
                .cloned()
            {
                let expected = expected_command_sequence(state, &envelope.session_id);
                return Ok((
                    existing.clone(),
                    true,
                    existing.server_command_seq > expected,
                ));
            }
            if let Some(existing) = state
                .command_tombstones
                .iter()
                .find(|record| record.command_id == envelope.command_id)
            {
                let record = tombstone_record(existing);
                return Ok((record, true, false));
            }
            let expected = expected_command_sequence(state, &envelope.session_id);
            if envelope.server_command_seq > expected {
                let pending = state
                    .commands
                    .iter()
                    .filter(|record| {
                        record.session_id == envelope.session_id
                            && record.server_command_seq > expected
                            && record.state != CommandJournalState::Terminal
                    })
                    .count();
                if pending >= self.config.max_gap_buffer_entries_per_session {
                    return Err(JournalError::GapOverflow {
                        session_id: envelope.session_id.clone(),
                        limit: self.config.max_gap_buffer_entries_per_session,
                    });
                }
            } else if envelope.server_command_seq < expected {
                return Err(JournalError::IdentityConflict(
                    "command sequence is below the retained processed boundary".into(),
                ));
            }
            let record = CommandRecord {
                daemon_id: self.daemon_id.clone(),
                command_id: envelope.command_id.clone(),
                session_id: envelope.session_id.clone(),
                server_command_seq: envelope.server_command_seq,
                sent_at: envelope.sent_at.clone(),
                payload_digest: digest.into(),
                command: envelope.command.clone(),
                message_id,
                state: CommandJournalState::Received,
                outcome: None,
                runtime_operation_id: envelope.command_id.clone(),
                reason: None,
                updated_at: now_string(),
            };
            state.commands.push(record.clone());
            Ok((record, false, envelope.server_command_seq > expected))
        })
    }

    fn command_record(&self, command_id: &str) -> Result<CommandRecord, JournalError> {
        let state = self.state.lock().map_err(lock_error)?;
        state
            .commands
            .iter()
            .find(|record| record.command_id == command_id)
            .cloned()
            .ok_or_else(|| {
                JournalError::IdentityConflict("command disappeared from journal".into())
            })
    }

    fn mark_dispatch_started(&self, command_id: &str) -> Result<bool, JournalError> {
        self.update(|state| {
            let record = state
                .commands
                .iter_mut()
                .find(|record| record.command_id == command_id)
                .ok_or_else(|| {
                    JournalError::IdentityConflict("command disappeared before dispatch".into())
                })?;
            if record.state != CommandJournalState::Received {
                return Ok(false);
            }
            record.state = CommandJournalState::DispatchStarted;
            record.updated_at = now_string();
            Ok(true)
        })
    }

    fn finish_command(
        &self,
        command_id: &str,
        outcome: DispatchOutcome,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.update(|state| {
            let (session_id, runtime_operation_id) = {
                let record = state
                    .commands
                    .iter_mut()
                    .find(|record| record.command_id == command_id)
                    .ok_or_else(|| JournalError::IdentityConflict("command disappeared before terminal commit".into()))?;
                record.state = CommandJournalState::Terminal;
                record.reason = outcome_reason(&outcome);
                record.outcome = Some(outcome.clone());
                record.updated_at = now_string();
                (record.session_id.clone(), record.runtime_operation_id.clone())
            };
            advance_processed(state, &session_id);
            let mut events = Vec::new();
            if let DispatchOutcome::Unknown(reason) = &outcome {
                let event = EventEnvelope {
                    event_id: format!("execution-unknown-{command_id}"),
                    session_id: session_id.clone(),
                    daemon_event_seq: next_event_sequence(state, &session_id),
                    sent_at: now_string(),
                    schema_version: SCHEMA_VERSION,
                    event: Event::SessionFailed(north_protocol::SessionFailed {
                        recoverable: false,
                        reason: format!(
                            "execution_outcome_unknown command_id={command_id} runtime_operation_id={runtime_operation_id} {reason} automatic_resubmit=false"
                        ),
                    }),
                };
                let digest = payload_digest(&event)?;
                state.events.push(EventRecord {
                    daemon_id: self.daemon_id.clone(),
                    envelope: event.clone(),
                    payload_digest: digest,
                    acknowledged: false,
                    acknowledgement: None,
                });
                events.push(event);
            }
            Ok(events)
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_command_for_test(
        &self,
        envelope: CommandEnvelope,
        digest: String,
    ) -> Result<(), JournalError> {
        self.prepare_command(&envelope, &digest, message_id(&envelope.command))
            .map(|_| ())
    }

    #[cfg(test)]
    pub(crate) fn mark_dispatch_started_for_test(
        &self,
        command_id: &str,
    ) -> Result<(), JournalError> {
        self.mark_dispatch_started(command_id).map(|_| ())
    }

    fn update<T>(
        &self,
        operation: impl FnOnce(&mut JournalFile) -> Result<T, JournalError>,
    ) -> Result<T, JournalError> {
        let mut state = self.state.lock().map_err(lock_error)?;
        let before = state.clone();
        let result = operation(&mut state);
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                *state = before;
                return Err(error);
            }
        };
        if let Err(error) = self.persist_locked(&state) {
            if let Ok(bytes) = fs::read(self.path.as_path()) {
                if let Ok(committed) = serde_json::from_slice::<JournalFile>(&bytes) {
                    *state = committed;
                } else {
                    *state = before;
                }
            } else {
                *state = before;
            }
            return Err(error);
        }
        Ok(value)
    }

    fn persist_locked(&self, state: &JournalFile) -> Result<(), JournalError> {
        let parent = journal_parent(self.path.as_path());
        fs::create_dir_all(parent).map_err(io_error)?;
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| JournalError::Format(error.to_string()))?;
        let temporary = self.path.with_extension("journal.tmp");
        let mut file = fs::File::create(&temporary).map_err(io_error)?;
        use std::io::Write;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        fs::rename(&temporary, self.path.as_path()).map_err(io_error)?;
        self.set_private_permissions()?;
        self.sync_parent_directory()
    }

    fn sync_parent_directory(&self) -> Result<(), JournalError> {
        #[cfg(unix)]
        {
            let parent = journal_parent(self.path.as_path());
            fs::File::open(parent)
                .map_err(io_error)?
                .sync_all()
                .map_err(io_error)?;
        }
        Ok(())
    }

    fn set_private_permissions(&self) -> Result<(), JournalError> {
        #[cfg(unix)]
        {
            let permissions = fs::Permissions::from_mode(0o600);
            fs::set_permissions(self.path.as_path(), permissions).map_err(io_error)?;
        }
        Ok(())
    }
}

fn journal_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn validate_command_identity(
    state: &JournalFile,
    daemon_id: &str,
    envelope: &CommandEnvelope,
    digest: &str,
    message_id: Option<&str>,
) -> Result<(), JournalError> {
    for record in state
        .commands
        .iter()
        .filter(|record| record.daemon_id == daemon_id)
    {
        let same_id = record.command_id == envelope.command_id;
        let same_sequence = record.session_id == envelope.session_id
            && record.server_command_seq == envelope.server_command_seq;
        let same_message = message_id.is_some()
            && record.message_id.as_deref() == message_id
            && record.session_id == envelope.session_id;
        if (same_id || same_sequence || same_message)
            && (record.session_id != envelope.session_id
                || record.server_command_seq != envelope.server_command_seq
                || record.payload_digest != digest
                || (same_id && record.command != envelope.command))
        {
            return Err(JournalError::IdentityConflict(
                "command ID, sequence, or message identity was reused with different data".into(),
            ));
        }
    }
    for record in state
        .command_tombstones
        .iter()
        .filter(|record| record.daemon_id == daemon_id)
    {
        let same_id = record.command_id == envelope.command_id;
        let same_sequence = record.session_id == envelope.session_id
            && record.server_command_seq == envelope.server_command_seq;
        let same_message = message_id.is_some()
            && record.message_id.as_deref() == message_id
            && record.session_id == envelope.session_id;
        if (same_id || same_sequence || same_message)
            && (record.session_id != envelope.session_id
                || record.server_command_seq != envelope.server_command_seq
                || record.payload_digest != digest
                || (same_message && record.command_id != envelope.command_id))
        {
            return Err(JournalError::IdentityConflict(
                "command tombstone identity conflict".into(),
            ));
        }
    }
    Ok(())
}

fn validate_event_identity(
    state: &JournalFile,
    daemon_id: &str,
    envelope: &EventEnvelope,
    digest: &str,
) -> Result<(), JournalError> {
    for record in state
        .events
        .iter()
        .filter(|record| record.daemon_id == daemon_id)
    {
        let same_id = record.envelope.event_id == envelope.event_id;
        let same_sequence = record.envelope.session_id == envelope.session_id
            && record.envelope.daemon_event_seq == envelope.daemon_event_seq;
        if (same_id || same_sequence)
            && (record.envelope.session_id != envelope.session_id
                || record.envelope.daemon_event_seq != envelope.daemon_event_seq
                || record.payload_digest != digest)
        {
            return Err(JournalError::IdentityConflict(
                "event ID or sequence was reused with different data".into(),
            ));
        }
    }
    for record in state
        .event_tombstones
        .iter()
        .filter(|record| record.daemon_id == daemon_id)
    {
        let same_id = record.event_id == envelope.event_id;
        let same_sequence = record.session_id == envelope.session_id
            && record.daemon_event_seq == envelope.daemon_event_seq;
        if (same_id || same_sequence)
            && (record.session_id != envelope.session_id
                || record.daemon_event_seq != envelope.daemon_event_seq
                || record.payload_digest != digest)
        {
            return Err(JournalError::IdentityConflict(
                "event tombstone identity conflict".into(),
            ));
        }
    }
    Ok(())
}

fn expected_command_sequence(state: &JournalFile, session_id: &str) -> u64 {
    state
        .sessions
        .get(session_id)
        .map(|session| session.processed_through_seq.saturating_add(1))
        .unwrap_or(1)
}

fn advance_processed(state: &mut JournalFile, session_id: &str) {
    let session = state.sessions.entry(session_id.to_owned()).or_default();
    loop {
        let next = session.processed_through_seq.saturating_add(1);
        let terminal = state.commands.iter().any(|record| {
            record.session_id == session_id
                && record.server_command_seq == next
                && record.state == CommandJournalState::Terminal
        });
        if !terminal {
            break;
        }
        session.processed_through_seq = next;
    }
}

fn advance_event_ack(state: &mut JournalFile, session_id: &str) {
    let session = state.sessions.entry(session_id.to_owned()).or_default();
    loop {
        let next = session.event_ack_through_seq.saturating_add(1);
        let acknowledged = state.events.iter().any(|record| {
            record.envelope.session_id == session_id
                && record.envelope.daemon_event_seq == next
                && record.acknowledged
        });
        if !acknowledged {
            break;
        }
        session.event_ack_through_seq = next;
        session.event_ack_sparse.retain(|sequence| *sequence > next);
    }
}

fn next_event_sequence(state: &JournalFile, session_id: &str) -> u64 {
    let cursor = state
        .sessions
        .get(session_id)
        .map(|session| {
            session.event_ack_through_seq.max(
                session
                    .event_ack_sparse
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let current = state
        .events
        .iter()
        .filter(|record| record.envelope.session_id == session_id)
        .map(|record| record.envelope.daemon_event_seq)
        .chain(
            state
                .event_tombstones
                .iter()
                .filter(|record| record.session_id == session_id)
                .map(|record| record.daemon_event_seq),
        )
        .max()
        .unwrap_or_default();
    cursor.max(current).saturating_add(1)
}

fn session_ids(state: &Arc<Mutex<JournalFile>>) -> Result<Vec<String>, JournalError> {
    let state = state.lock().map_err(lock_error)?;
    Ok(state.sessions.keys().cloned().collect())
}

fn merge_sparse(existing: &[u64], incoming: &[u64]) -> Vec<u64> {
    let mut values = existing.iter().chain(incoming).copied().collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn message_id(command: &Command) -> Option<String> {
    match command {
        Command::MessageSend(message) => Some(message.message_id.clone()),
        _ => None,
    }
}

fn command_ack(record: &CommandRecord) -> CommandAck {
    CommandAck {
        command_id: record.command_id.clone(),
        session_id: record.session_id.clone(),
        server_command_seq: record.server_command_seq,
        schema_version: SCHEMA_VERSION,
    }
}

fn record_to_envelope(record: &CommandRecord) -> Result<CommandEnvelope, JournalError> {
    Ok(CommandEnvelope {
        command_id: record.command_id.clone(),
        session_id: record.session_id.clone(),
        server_command_seq: record.server_command_seq,
        sent_at: record.sent_at.clone(),
        schema_version: SCHEMA_VERSION,
        command: record.command.clone(),
    })
}

fn tombstone_record(tombstone: &CommandTombstone) -> CommandRecord {
    CommandRecord {
        daemon_id: tombstone.daemon_id.clone(),
        command_id: tombstone.command_id.clone(),
        session_id: tombstone.session_id.clone(),
        server_command_seq: tombstone.server_command_seq,
        sent_at: now_string(),
        payload_digest: tombstone.payload_digest.clone(),
        message_id: tombstone.message_id.clone(),
        command: Command::SessionResume(north_protocol::SessionResume {}),
        state: CommandJournalState::Terminal,
        outcome: Some(tombstone.outcome.clone()),
        runtime_operation_id: tombstone.command_id.clone(),
        reason: None,
        updated_at: now_string(),
    }
}

fn outcome_reason(outcome: &DispatchOutcome) -> Option<String> {
    match outcome {
        DispatchOutcome::DispatchSucceeded => None,
        DispatchOutcome::DispatchFailed(reason) | DispatchOutcome::Unknown(reason) => {
            Some(reason.clone())
        }
    }
}

fn unknown_reason(command_id: &str) -> String {
    format!("command_id={command_id} runtime_operation_id={command_id}")
}

fn event_identity_digest(envelope: &EventEnvelope) -> Result<String, JournalError> {
    let mut identity = envelope.clone();
    identity.sent_at.clear();
    payload_digest(&identity)
}

fn payload_digest<T: Serialize>(value: &T) -> Result<String, JournalError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| JournalError::Format(error.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn io_error(error: std::io::Error) -> JournalError {
    JournalError::Io(error.to_string())
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> JournalError {
    JournalError::Io("journal lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use north_protocol::{MessageSend, SessionResume};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    struct FakeExecutor {
        calls: Arc<Mutex<Vec<String>>>,
        outcome: DispatchOutcome,
        recovery: RecoveryOutcome,
    }

    impl RuntimeExecutor for FakeExecutor {
        fn dispatch(
            &self,
            operation_id: &str,
            _command_id: &str,
            _command: &Command,
        ) -> DispatchOutcome {
            self.calls
                .lock()
                .expect("calls lock")
                .push(operation_id.into());
            self.outcome.clone()
        }

        fn recover(
            &self,
            _runtime_operation_id: &str,
            _command_id: &str,
            _command: &Command,
        ) -> RecoveryOutcome {
            self.recovery.clone()
        }
    }

    fn command(sequence: u64, id: &str, message_id: &str) -> CommandEnvelope {
        CommandEnvelope {
            command_id: id.into(),
            session_id: "session-1".into(),
            server_command_seq: sequence,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::MessageSend(MessageSend {
                message_id: message_id.into(),
                content: "hello".into(),
            }),
        }
    }

    #[test]
    fn duplicate_message_crosses_runtime_once() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        let first = journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("first");
        let second = journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("duplicate");
        assert!(!first.duplicate);
        assert!(second.duplicate);
        assert_eq!(calls.lock().expect("calls").as_slice(), ["command-1"]);
    }

    #[test]
    fn bounded_gap_buffers_then_drains_in_order() {
        let directory = tempdir().expect("temporary journal");
        let journal = Journal::open_with_config(
            directory.path().join("daemon.json"),
            "daemon-1",
            JournalConfig {
                max_gap_buffer_entries_per_session: 1,
            },
        )
        .expect("open");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        let buffered = journal
            .process_command(command(2, "command-2", "message-2"), &executor)
            .expect("buffer one gap entry");
        assert!(buffered.buffered);
        assert!(calls.lock().expect("calls").is_empty());
        let overflow = journal.process_command(command(3, "command-3", "message-3"), &executor);
        assert!(matches!(
            overflow,
            Err(JournalError::GapOverflow { limit: 1, .. })
        ));
        journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("close gap");
        let drained = journal
            .process_ready("session-1", &executor)
            .expect("drain gap");
        assert_eq!(drained.len(), 1);
        assert_eq!(
            calls.lock().expect("calls").as_slice(),
            ["command-1", "command-2"]
        );
    }

    #[test]
    fn received_command_recovers_after_journal_reopen() {
        let directory = tempdir().expect("temporary journal");
        let path = directory.path().join("daemon.json");
        let envelope = command(1, "received-command", "received-message");
        {
            let journal = Journal::open(&path, "daemon-1").expect("open");
            let digest = payload_digest(&envelope).expect("digest");
            journal
                .prepare_command_for_test(envelope, digest)
                .expect("received record");
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let journal = Journal::open(&path, "daemon-1").expect("reopen");
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        let recovered = journal
            .recover(&executor)
            .expect("recover received command");
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            calls.lock().expect("calls").as_slice(),
            ["received-command"]
        );
    }

    #[test]
    fn terminal_command_is_inert_after_journal_reopen() {
        let directory = tempdir().expect("temporary journal");
        let path = directory.path().join("daemon.json");
        let calls = Arc::new(Mutex::new(Vec::new()));
        {
            let journal = Journal::open(&path, "daemon-1").expect("open");
            let executor = FakeExecutor {
                calls: calls.clone(),
                outcome: DispatchOutcome::DispatchSucceeded,
                recovery: RecoveryOutcome::DispatchSucceeded,
            };
            journal
                .process_command(command(1, "command-1", "message-1"), &executor)
                .expect("first");
        }
        let journal = Journal::open(&path, "daemon-1").expect("reopen");
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        let result = journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("duplicate");
        assert!(result.duplicate);
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn compacted_message_tombstone_rejects_remap() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("first");
        journal.compact_session("session-1").expect("compact");
        let error = journal
            .process_command(command(2, "command-2", "message-1"), &executor)
            .expect_err("compacted message identity must remain bound");
        assert!(matches!(error, JournalError::IdentityConflict(_)));
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn message_identity_conflict_has_no_second_call() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        journal
            .process_command(command(1, "command-1", "message-1"), &executor)
            .expect("first");
        let error = journal
            .process_command(command(2, "command-2", "message-1"), &executor)
            .expect_err("same message under another command must fail");
        assert!(matches!(error, JournalError::IdentityConflict(_)));
        assert_eq!(calls.lock().expect("calls").len(), 1);
    }

    #[test]
    fn dispatch_started_recovery_does_not_dispatch_again() {
        let directory = tempdir().expect("temporary journal");
        let path = directory.path().join("daemon.json");
        let envelope = CommandEnvelope {
            command_id: "command-1".into(),
            session_id: "session-1".into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: SCHEMA_VERSION,
            command: Command::SessionResume(SessionResume {}),
        };
        {
            let journal = Journal::open(&path, "daemon-1").expect("open");
            journal
                .prepare_command(&envelope, &payload_digest(&envelope).expect("digest"), None)
                .expect("prepare");
            journal.mark_dispatch_started("command-1").expect("started");
        }
        let journal = Journal::open(&path, "daemon-1").expect("reopen");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::DispatchSucceeded,
        };
        let recovered = journal.recover(&executor).expect("recover");
        assert_eq!(
            recovered[0].outcome,
            Some(DispatchOutcome::DispatchSucceeded)
        );
        assert!(calls.lock().expect("calls").is_empty());
    }

    #[test]
    fn unknown_recovery_emits_non_resubmittable_failure_fact() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor = FakeExecutor {
            calls: calls.clone(),
            outcome: DispatchOutcome::DispatchSucceeded,
            recovery: RecoveryOutcome::Unknown,
        };
        let envelope = command(1, "command-unknown", "message-unknown");
        journal
            .prepare_command(
                &envelope,
                &payload_digest(&envelope).expect("digest"),
                Some("message-unknown".into()),
            )
            .expect("prepare");
        journal
            .mark_dispatch_started("command-unknown")
            .expect("started");
        let recovered = journal.recover(&executor).expect("recover");
        assert_eq!(recovered[0].emitted_events.len(), 1);
        let reason = match &recovered[0].emitted_events[0].event {
            Event::SessionFailed(failed) => &failed.reason,
            _ => panic!("expected session.failed"),
        };
        assert!(reason.contains("execution_outcome_unknown"));
        assert!(reason.contains("automatic_resubmit=false"));
        assert!(!reason.contains("secret"));
    }

    #[test]
    fn sparse_event_ack_suppresses_only_acknowledged_replay() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let first = journal
            .append_event(
                "event-1",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "first".into(),
                }),
            )
            .expect("first event");
        let second = journal
            .append_event(
                "event-2",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "second".into(),
                }),
            )
            .expect("second event");
        journal
            .acknowledge_event(EventAck {
                event_id: second.envelope.event_id.clone(),
                session_id: "session-1".into(),
                daemon_event_seq: 2,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Accepted,
                reason: None,
            })
            .expect("sparse ACK");
        let replay = journal
            .replay_events(
                "session-1",
                &SessionReconcileState {
                    session_id: "session-1".into(),
                    command_ack_through_seq: 0,
                    event_ack_through_seq: 0,
                    event_ack_sparse: vec![2],
                },
            )
            .expect("replay");
        assert_eq!(replay, vec![first.envelope]);
        journal
            .acknowledge_event(EventAck {
                event_id: "event-1".into(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Accepted,
                reason: None,
            })
            .expect("contiguous ACK");
        let snapshot = journal.snapshot().expect("snapshot");
        assert_eq!(snapshot.event_ack_through["session-1"], 2);
    }

    #[test]
    fn event_ack_and_compaction_leave_tombstone_protection() {
        let directory = tempdir().expect("temporary journal");
        let journal =
            Journal::open(directory.path().join("daemon.json"), "daemon-1").expect("open");
        let event = journal
            .append_event(
                "event-1",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "work".into(),
                }),
            )
            .expect("append");
        journal
            .acknowledge_event(EventAck {
                event_id: event.envelope.event_id.clone(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Rejected,
                reason: Some("stale".into()),
            })
            .expect("ack");
        journal.compact_session("session-1").expect("compact");
        let snapshot = journal.snapshot().expect("snapshot");
        assert_eq!(snapshot.event_count, 0);
        assert_eq!(snapshot.event_tombstone_count, 1);
        let duplicate = journal
            .append_event(
                "event-1",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "work".into(),
                }),
            )
            .expect("identical compacted event");
        assert!(duplicate.duplicate);
        assert!(journal
            .append_event(
                "event-1",
                "session-1",
                Event::AgentActivity(north_protocol::AgentActivity {
                    activity: "changed".into(),
                }),
            )
            .is_err());
        assert!(journal
            .acknowledge_event(EventAck {
                event_id: "event-1".into(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Accepted,
                reason: None,
            })
            .is_err());
        assert!(journal
            .acknowledge_event(EventAck {
                event_id: "event-1".into(),
                session_id: "session-1".into(),
                daemon_event_seq: 1,
                schema_version: SCHEMA_VERSION,
                status: EventAckStatus::Rejected,
                reason: Some("same".into()),
            })
            .is_ok());
    }
}
