//! Non-blocking daemon runtime scheduling.
//!
//! Command durability stays in [`crate::journal::Journal`]. This module only
//! owns worker admission, session control, and delivery of completed worker
//! results back to the transport task.

use crate::journal::{DispatchOutcome, RuntimeControl, RuntimeExecutor, RuntimeLifecycle};
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
    control: RuntimeControl,
}

struct QueuedRuntime {
    command: CommandEnvelope,
    control: RuntimeControl,
}

struct PendingCancellation {
    command: CommandEnvelope,
}

pub struct RuntimeScheduler<E> {
    executor: Arc<E>,
    completion_sender: mpsc::UnboundedSender<RuntimeCompletion>,
    active: Mutex<HashMap<String, ActiveRuntime>>,
    controls: Mutex<HashMap<String, RuntimeControl>>,
    pending_cancellations: Mutex<HashMap<String, PendingCancellation>>,
    suppressed: Mutex<HashMap<String, RuntimeControl>>,
    queued: Mutex<VecDeque<QueuedRuntime>>,
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
            active: Mutex::new(HashMap::new()),
            controls: Mutex::new(HashMap::new()),
            pending_cancellations: Mutex::new(HashMap::new()),
            suppressed: Mutex::new(HashMap::new()),
            queued: Mutex::new(VecDeque::new()),
            permits: Arc::new(Semaphore::new(MAX_RUNTIME_CONCURRENCY)),
        }
    }

    /// Admit one durable command and create session control before any worker
    /// can start. Cancellation is control state, not a Pi-child side effect.
    pub fn schedule(&self, command: CommandEnvelope) -> Result<(), String> {
        let control = self.control_for(&command)?;
        if matches!(&command.command, Command::SessionCancel(_)) {
            return self.schedule_cancellation(command, control);
        }
        if control.is_cancellation_requested() {
            return self.suppress(command, control);
        }
        self.schedule_runtime_inner(command, control)
    }

    /// Schedule a command that is already marked `dispatch_started`, without
    /// interpreting a cancellation command as a new control request. This is
    /// used only for the cancellation follow-up after an interrupted runtime.
    pub fn schedule_runtime(&self, command: CommandEnvelope) -> Result<(), String> {
        let control = self.control_for(&command)?;
        self.schedule_runtime_inner(command, control)
    }

    pub fn finish_active(&self, completion: &RuntimeCompletion) -> Result<RuntimeFinished, String> {
        let control = if let Some(control) = self
            .suppressed
            .lock()
            .map_err(|_| "suppressed runtime state lock poisoned".to_owned())?
            .remove(&completion.command_id)
        {
            control
        } else {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "active runtime state lock poisoned".to_owned())?;
            let Some(runtime) = active.get(&completion.session_id) else {
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
            active
                .remove(&completion.session_id)
                .expect("active runtime was checked while holding its lock")
                .control
        };
        let completion_is_terminal = has_terminal_event(&completion.events);
        if completion_is_terminal {
            control.mark_terminal_fact_emitted();
        }
        let pending = self
            .pending_cancellations
            .lock()
            .map_err(|_| "runtime cancellation state lock poisoned".to_owned())?
            .remove(&completion.session_id);
        let terminal = completion_is_terminal || control.terminal_fact_emitted();
        let followup = pending.map(|pending| {
            if terminal {
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
        let controls = {
            let active = self
                .active
                .lock()
                .map_err(|_| "active runtime state lock poisoned".to_owned())?;
            active
                .values()
                .map(|runtime| runtime.control.clone())
                .collect::<Vec<_>>()
        };
        for control in controls {
            control.request_cancellation();
            let _ = self.executor.cancel_for_session(control.session_id());
        }
        Ok(())
    }

    fn control_for(&self, command: &CommandEnvelope) -> Result<RuntimeControl, String> {
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| "runtime control state lock poisoned".to_owned())?;
        Ok(controls
            .entry(command.session_id.clone())
            .or_insert_with(|| RuntimeControl::new(&command.session_id, &command.command_id))
            .clone())
    }

    fn schedule_cancellation(
        &self,
        command: CommandEnvelope,
        control: RuntimeControl,
    ) -> Result<(), String> {
        control.request_cancellation();
        // Retain compatibility with executors that expose an independent
        // active-child interrupt. The admission-time control is authoritative.
        let _ = self.executor.cancel_for_session(&command.session_id);
        let queued = self.remove_queued(&command.session_id)?;
        let active = self.is_active(&command.session_id)?;
        let already_pending = self
            .pending_cancellations
            .lock()
            .map_err(|_| "runtime cancellation state lock poisoned".to_owned())?
            .contains_key(&command.session_id);
        if active || !queued.is_empty() {
            if !already_pending {
                self.pending_cancellations
                    .lock()
                    .map_err(|_| "runtime cancellation state lock poisoned".to_owned())?
                    .insert(command.session_id.clone(), PendingCancellation { command });
            }
            for queued in queued {
                self.suppress(queued.command, queued.control)?;
            }
            return Ok(());
        }
        if already_pending {
            return Ok(());
        }
        self.schedule_runtime_inner(command, control)
    }

    fn is_active(&self, session_id: &str) -> Result<bool, String> {
        Ok(self
            .active
            .lock()
            .map_err(|_| "active runtime state lock poisoned".to_owned())?
            .contains_key(session_id))
    }

    fn remove_queued(&self, session_id: &str) -> Result<Vec<QueuedRuntime>, String> {
        let mut queue = self
            .queued
            .lock()
            .map_err(|_| "runtime queue lock poisoned".to_owned())?;
        let mut retained = VecDeque::with_capacity(queue.len());
        let mut removed = Vec::new();
        while let Some(item) = queue.pop_front() {
            if item.command.session_id == session_id {
                removed.push(item);
            } else {
                retained.push_back(item);
            }
        }
        *queue = retained;
        Ok(removed)
    }

    fn suppress(&self, command: CommandEnvelope, control: RuntimeControl) -> Result<(), String> {
        self.suppressed
            .lock()
            .map_err(|_| "suppressed runtime state lock poisoned".to_owned())?
            .insert(command.command_id.clone(), control.clone());
        let events = if control.mark_terminal_fact_emitted() {
            vec![Event::SessionCompleted(SessionCompleted {
                summary: "Clarification runtime cancelled before execution".into(),
            })]
        } else {
            Vec::new()
        };
        self.completion_sender
            .send(RuntimeCompletion {
                session_id: command.session_id,
                command_id: command.command_id.clone(),
                runtime_operation_id: command.command_id,
                outcome: DispatchOutcome::DispatchSucceeded,
                events,
            })
            .map_err(|_| "runtime completion channel closed".to_owned())
    }

    fn schedule_runtime_inner(
        &self,
        command: CommandEnvelope,
        control: RuntimeControl,
    ) -> Result<(), String> {
        if control.is_cancellation_requested()
            && !matches!(&command.command, Command::SessionCancel(_))
        {
            return self.suppress(command, control);
        }
        if self.is_active(&command.session_id)? {
            control.set_lifecycle(RuntimeLifecycle::Queued);
            self.queued
                .lock()
                .map_err(|_| "runtime queue lock poisoned".to_owned())?
                .push_back(QueuedRuntime { command, control });
            return Ok(());
        }
        let permit = match self.permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                control.set_lifecycle(RuntimeLifecycle::Queued);
                self.queued
                    .lock()
                    .map_err(|_| "runtime queue lock poisoned".to_owned())?
                    .push_back(QueuedRuntime { command, control });
                return Ok(());
            }
        };
        self.spawn_runtime(command, control, permit)
    }

    fn spawn_runtime(
        &self,
        command: CommandEnvelope,
        control: RuntimeControl,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<(), String> {
        let session_id = command.session_id.clone();
        let command_id = command.command_id.clone();
        let runtime_operation_id = command.command_id.clone();
        control.set_active_command_id(&command_id);
        control.set_lifecycle(RuntimeLifecycle::Preparing);
        let executor = self.executor.clone();
        let completion_sender = self.completion_sender.clone();
        let mut active = self
            .active
            .lock()
            .map_err(|_| "active runtime state lock poisoned".to_owned())?;
        if active.contains_key(&session_id) {
            return Err(format!("runtime session {} is already active", session_id));
        }
        active.insert(
            session_id.clone(),
            ActiveRuntime {
                command_id: command_id.clone(),
                control: control.clone(),
            },
        );
        drop(active);
        let worker_session_id = session_id.clone();
        let worker_command_id = command_id.clone();
        let worker_operation_id = runtime_operation_id.clone();
        let worker_command = command.command.clone();
        let worker_control = control;
        tokio::spawn(async move {
            let worker = tokio::task::spawn_blocking(move || {
                let outcome = executor.dispatch_for_session_with_control(
                    &worker_session_id,
                    &worker_operation_id,
                    &worker_command_id,
                    &worker_command,
                    worker_control,
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
            let item = self
                .queued
                .lock()
                .map_err(|_| "runtime queue lock poisoned".to_owned())?
                .pop_front();
            let Some(QueuedRuntime { command, control }) = item else {
                break;
            };
            if control.is_cancellation_requested()
                && !matches!(&command.command, Command::SessionCancel(_))
            {
                self.suppress(command, control)?;
                continue;
            }
            if self.is_active(&command.session_id)? {
                self.queued
                    .lock()
                    .map_err(|_| "runtime queue lock poisoned".to_owned())?
                    .push_back(QueuedRuntime { command, control });
                continue;
            }
            let permit = match self.permits.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    self.queued
                        .lock()
                        .map_err(|_| "runtime queue lock poisoned".to_owned())?
                        .push_front(QueuedRuntime { command, control });
                    break;
                }
            };
            self.spawn_runtime(command, control, permit)?;
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
                    std::thread::yield_now();
                }
            }
            self.dispatch(operation_id, command_id, command)
        }
    }

    fn command(session_id: &str, command_id: &str, server_command_seq: u64) -> CommandEnvelope {
        CommandEnvelope {
            command_id: command_id.into(),
            session_id: session_id.into(),
            server_command_seq,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: north_protocol::SCHEMA_VERSION,
            command: Command::SessionResume(north_protocol::SessionResume {}),
        }
    }

    fn cancel_command(
        session_id: &str,
        command_id: &str,
        server_command_seq: u64,
    ) -> CommandEnvelope {
        CommandEnvelope {
            command_id: command_id.into(),
            session_id: session_id.into(),
            server_command_seq,
            sent_at: "2026-01-01T00:00:00Z".into(),
            schema_version: north_protocol::SCHEMA_VERSION,
            command: Command::SessionCancel(north_protocol::SessionCancel {
                reason: "requester_cancelled".into(),
            }),
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
            .schedule(command("session-a", "command-a", 1))
            .unwrap();
        while !first_started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        scheduler
            .schedule(command("session-b", "command-b", 1))
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

    #[derive(Clone)]
    struct PermitFillingExecutor {
        entered: Arc<AtomicUsize>,
        release: Arc<AtomicBool>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl RuntimeExecutor for PermitFillingExecutor {
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
            self.calls
                .lock()
                .expect("calls")
                .push(session_id.to_owned());
            if matches!(
                session_id,
                "session-a" | "session-b" | "session-c" | "session-d"
            ) {
                self.entered.fetch_add(1, Ordering::SeqCst);
                while !self.release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            }
            self.dispatch(operation_id, command_id, command)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queued_start_cancelled_before_execution_never_launches_runtime() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let executor = Arc::new(PermitFillingExecutor {
            entered: Arc::new(AtomicUsize::new(0)),
            release: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let scheduler = RuntimeScheduler::new(executor.clone(), sender);
        for (session_id, command_id) in [
            ("session-a", "command-a"),
            ("session-b", "command-b"),
            ("session-c", "command-c"),
            ("session-d", "command-d"),
        ] {
            scheduler
                .schedule(command(session_id, command_id, 1))
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while executor.entered.load(Ordering::SeqCst) < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("four runtimes enter");
        scheduler
            .schedule(command("session-e", "command-e", 1))
            .unwrap();
        scheduler
            .schedule(cancel_command("session-e", "cancel-e", 2))
            .unwrap();

        let cancelled = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.session_id, "session-e");
        assert_eq!(cancelled.command_id, "command-e");
        assert!(matches!(
            cancelled.events.as_slice(),
            [Event::SessionCompleted(_)]
        ));
        scheduler.finish_active(&cancelled).unwrap();
        assert!(!executor
            .calls
            .lock()
            .expect("calls")
            .iter()
            .any(|session_id| session_id == "session-e"));

        executor.release.store(true, Ordering::Release);
        for _ in 0..4 {
            let completion = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            scheduler.finish_active(&completion).unwrap();
        }
    }

    #[derive(Clone)]
    struct RaceExecutor {
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }

    impl RuntimeExecutor for RaceExecutor {
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
            _session_id: &str,
            _operation_id: &str,
            _command_id: &str,
            _command: &Command,
        ) -> DispatchOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.store(true, Ordering::SeqCst);
            while !self.release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            DispatchOutcome::DispatchSucceeded
        }

        fn take_events(&self, _session_id: &str) -> Vec<Event> {
            vec![Event::SessionCompleted(SessionCompleted {
                summary: "natural completion".into(),
            })]
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_racing_natural_completion_emits_one_terminal_fact() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let executor = Arc::new(RaceExecutor {
            entered: Arc::new(AtomicBool::new(false)),
            release: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let scheduler = RuntimeScheduler::new(executor.clone(), sender);
        scheduler
            .schedule(command("session-race", "command-race", 1))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !executor.entered.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime enters");
        scheduler
            .schedule(cancel_command("session-race", "cancel-race", 2))
            .unwrap();
        executor.release.store(true, Ordering::Release);

        let completion = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            completion.events.as_slice(),
            [Event::SessionCompleted(_)]
        ));
        let finished = scheduler.finish_active(&completion).unwrap();
        assert!(matches!(
            finished.followup,
            Some(RuntimeFollowup::FinishCancellation(command)) if command.command_id == "cancel-race"
        ));
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), receiver.recv())
                .await
                .is_err()
        );
    }
}
