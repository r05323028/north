//! north-daemon owns local execution-host coordination. Its server link is
//! one `tokio-tungstenite` connection supervised outside session/runtime code.

pub mod coordination;
pub mod journal;
pub mod transport;

pub use coordination::{protocol_error, CoordinationError, DaemonCoordinator, HandshakeActions};
pub use journal::{
    AppendedEvent, CommandJournalState, CommandProcessResult, DispatchOutcome, Journal,
    JournalConfig, JournalError, JournalSnapshot, RecoveryOutcome, RuntimeExecutor,
    MAX_GAP_BUFFER_ENTRIES_PER_SESSION,
};
