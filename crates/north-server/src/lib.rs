//! north-server owns business coordination and exposes Axum HTTP/WebSocket
//! transport adapters. WebSocket messages are North JSON frames; application
//! coordination stays outside the handler.

pub mod transport;
