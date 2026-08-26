//! north-daemon owns local execution-host coordination. Its server link is
//! one `tokio-tungstenite` connection supervised outside session/runtime code.

pub mod transport;
