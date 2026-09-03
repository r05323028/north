//! Non-blocking daemon runtime scheduling.
//!
//! Command durability stays in [`crate::journal::Journal`]. This module only
//! owns worker admission, session control, and delivery of completed worker
//! results back to the transport task.

use crate::journal::{DispatchOutcome, RuntimeExecutor};
use north_protocol::{Command, CommandEnvelope, Event, SessionCompleted, SessionFailed};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, Semaphore};

const MAX_RUNTIME_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCompletion {
    pub session_id: String,
    pub command_id: String,
    pub runtime_operation_id: String,
    pub outcome: DispatchOutcome,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFollowup {
    FinishCancellation(CommandEnvelope),
    RescheduleCancellation(CommandEnvelope),
}

#[derive(Debug, Default)]
pub struct RuntimeFinished {
    pub followup: Option<RuntimeFollowup>,
}

struct ActiveRuntime {
    command_id: String,
}

struct PendingCancellation {
    command: CommandEnvelope,
    requested: bool,
}

pub struct RuntimeScheduler<E> {
    executor: Arc<E>,
    completion_sender: mpsc::UnboundedSender<RuntimeCompletion>,
    active: Arc<Mutex<HashMap<String, ActiveRuntime>>>,
    pending_cancellations: Mutex<HashMap<String, PendingCancellation>>,
    queued: Mutex<VecDeque<CommandEnvelope>>,
    permits: Arc<Semaphore>,
}

impl<E: RuntimeExecutor + 'static> RuntimeScheduler<E> {
    pub fn new(
        executor: Arc<E>,
        completion_sender: mpsc::UnboundedSender<RuntimeCompletion>,
    ) -> Self {
        Self {
            executor,
            completion_sender,
            active: Arc::new(Mutex::new(HashMap::new())),
            pending_cancellations: Mutex::new(HashMap::new()),
            queued: Mutex::new(VecDeque::new()),
            permits: Arc::new(Semaphore::new(MAX_RUNTIME_CONCURRENCY)),
        }
    }

    /// Admit a durable command. Cancellation bypasses per-session command
    /// sequencing only after the journal has recorded it and an operation is
    /// active for that session.
    pub fn schedule(&self, command: CommandEnvelope) -> Result<(), String> {
        if matches!(&command.command, Command::SessionCancel(_))
            && self.is_active(&command.session_id)?
        {
            let requested = self.executor.cancel_for_session(&command.session_id);
            self.pending_cancellations
                .lock()
                .map_err(|_| "runtime cancellation state lock poisoned".to_owned())?
                .insert(
                    command.session_id.clone(),
                    PendingCancellation { command, requested },
                );
            return Ok(());
        }
        self.schedule_runtime(command)
    }

    /// Schedule a command that is already marked `dispatch_started`, without
    /// interpreting a cancellation command as a control request again.
    pub fn schedule_runtime(&self, command: CommandEnvelope) -> Result<(), String> {
        self.schedule_runtime_inner(command)
    }

    pub fn finish_active(&self, completion: &RuntimeCompletion) -> Result<RuntimeFinished, String> {
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "active runtime state lock poisoned".to_owned())?;
            let Some(runtime) = active.remove(&completion.session_id) else {
                return Err(format!(
                    "runtime completion for inactive session {}",
                    completion.session_id
                ));
            };
            if runtime.command_id != completion.command_id {
                return Err(format!(
                    "runtime completion command mismatch for session {}",
                    completion.session_id
                ));
            }
        }
        let pending = self
            .pending_cancellations
            .lock()
            .map_err(|_| "runtime cancellation state lock poisoned".to_owned())?
            .remove(&completion.session_id);
        let followup = pending.map(|pending| {
            if pending.requested && has_terminal_event(&completion.events) {
                RuntimeFollowup::FinishCancellation(pending.command)
            } else {
                RuntimeFollowup::RescheduleCancellation(pending.command)
            }
        });
        self.drain_queue()?;
        Ok(RuntimeFinished { followup })
    }

    /// Signal every currently executing session during daemon shutdown. The
    /// worker still owns journal completion; this method never blocks on Pi.
    pub fn request_shutdown(&self) -> Result<(), String> {
        let sessions = self
            .active
            .lock()
            .map_err(|_| "active runtime state lock poisoned".to_owned())?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for session_id in sessions {
            self.executor.cancel_for_session(&session_id);
        }
        Ok(())
    }

    fn is_active(&self, session_id: &str) -> Result<bool, String> {
        Ok(self
            .active
            .lock()
            .map_err(|_| "active runtime state lock poisoned".to_owned())?
            .contains_key(session_id))
    }

    fn schedule_runtime_inner(&self, command: CommandEnvelope) -> Result<(), String> {
        if self.is_active(&command.session_id)? {
            self.queued
                .lock()
                .map_err(|_| "runtime queue lock poisoned".to_owned())?
                .push_back(command);
            return Ok(());
        }
        let permit = match self.permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                self.queued
                    .lock()
                    .map_err(|_| "runtime queue lock poisoned".to_owned())?
                    .push_back(command);
                return Ok(());
            }
        };
        self.spawn_runtime(command, permit)
    }

    fn spawn_runtime(
        &self,
        command: CommandEnvelope,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<(), String> {
        let session_id = command.session_id.clone();
        let command_id = command.command_id.clone();
        let runtime_operation_id = command.command_id.clone();
        let executor = self.executor.clone();
        let completion_sender = self.completion_sender.clone();
        self.active
            .lock()
            .map_err(|_| "active runtime state lock poisoned".to_owned())?
            .insert(
                session_id.clone(),
                ActiveRuntime {
                    command_id: command_id.clone(),
                },
            );
        let worker_session_id = session_id.clone();
        let worker_command_id = command_id.clone();
        let worker_operation_id = runtime_operation_id.clone();
        let worker_command = command.command.clone();
        tokio::spawn(async move {
            let worker = tokio::task::spawn_blocking(move || {
                let outcome = executor.dispatch_for_session(
                    &worker_session_id,
                    &worker_operation_id,
                    &worker_command_id,
                    &worker_command,
                );
                let events = executor.take_events(&worker_session_id);
                RuntimeCompletion {
                    session_id: worker_session_id,
                    command_id: worker_command_id,
                    runtime_operation_id: worker_operation_id,
                    outcome,
                    events,
                }
            });
            let completion = match worker.await {
                Ok(completion) => completion,
                Err(error) => RuntimeCompletion {
                    session_id,
                    command_id,
                    runtime_operation_id,
                    outcome: DispatchOutcome::DispatchFailed(format!(
                        "runtime worker failed: {error}"
                    )),
                    events: vec![Event::SessionFailed(SessionFailed {
                        recoverable: false,
                        reason: "runtime worker failed".into(),
                    })],
                },
            };
            drop(permit);
            let _ = completion_sender.send(completion);
        });
        Ok(())
    }

    fn drain_queue(&self) -> Result<(), String> {
        let mut remaining = self
            .queued
            .lock()
            .map_err(|_| "runtime queue lock poisoned".to_owned())?
            .len();
        while remaining > 0 {
            remaining -= 1;
            let command = self
                .queued
                .lock()
                .map_err(|_| "runtime queue lock poisoned".to_owned())?
                .pop_front();
            let Some(command) = command else { break };
            if self.is_active(&command.session_id)? {
                self.queued
                    .lock()
                    .map_err(|_| "runtime queue lock poisoned".to_owned())?
                    .push_back(command);
                continue;
            }
            let permit = match self.permits.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    self.queued
                        .lock()
                        .map_err(|_| "runtime queue lock poisoned".to_owned())?
                        .push_front(command);
                    break;
                }
            };
            self.spawn_runtime(command, permit)?;
        }
        Ok(())
    }
}

fn has_terminal_event(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::SessionCompleted(SessionCompleted { .. })
                | Event::SessionFailed(SessionFailed { .. })
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[derive(Clone)]
    struct BlockingExecutor {
        first_started: Arc<AtomicBool>,
        release_first: Arc<AtomicBool>,
    }

    impl RuntimeExecutor for BlockingExecutor {
        fn recover(
            &self,
            _operation_id: &str,
            _command_id: &str,
            _command: &Command,
        ) -> crate::journal::RecoveryOutcome {
            crate::journal::RecoveryOutcome::Unknown
        }

        fn dispatch(
            &self,
            _operation_id: &str,
            _command_id: &str,
            _command: &Command,
        ) -> DispatchOutcome {
            DispatchOutcome::DispatchSucceeded
        }

        fn dispatch_for_session(
            &self,
            session_id: &str,
            operation_id: &str,
            command_id: &str,
            command: &Command,
        ) -> DispatchOutcome {
            if session_id == "session-a" {
                self.first_started.store(true, Ordering::SeqCst);
                while !self.release_first.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            self.dispatch(operation_id, command_id, command)
        }
    }

    fn command(session_id: &str, command_id: &str) -> CommandEnvelope {
        CommandEnvelope {
            command_id: command_id.into(),
            session_id: session_id.into(),
            server_command_seq: 1,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: north_protocol::SCHEMA_VERSION,
            command: Command::SessionResume(north_protocol::SessionResume {}),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn slow_session_does_not_block_other_runtime_work() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let first_started = Arc::new(AtomicBool::new(false));
        let release_first = Arc::new(AtomicBool::new(false));
        let scheduler = RuntimeScheduler::new(
            Arc::new(BlockingExecutor {
                first_started: first_started.clone(),
                release_first: release_first.clone(),
            }),
            sender,
        );
        scheduler
            .schedule(command("session-a", "command-a"))
            .unwrap();
        while !first_started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        scheduler
            .schedule(command("session-b", "command-b"))
            .unwrap();
        let completion = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completion.session_id, "session-b");
        release_first.store(true, Ordering::SeqCst);
        let completion = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completion.session_id, "session-a");
    }
}
