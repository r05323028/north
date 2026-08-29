//! North requirement domain: pure business behavior.
//!
//! This crate MUST stay free of infrastructure concerns (HTTP, database,
//! agent SDKs, daemons). See docs/architecture/dependency-boundaries.md and
//! the structural tests in tests/architecture.
//!
//! Seed surface encodes 0.1.0 invariants before any host layer exists:
//! - requirement lifecycle transitions ([`status`]),
//! - revision-bound readiness ([`readiness`], [`requirement`]),
//! - role and permission rules ([`role`]).

pub mod readiness;
pub mod repository;
pub mod requirement;
pub mod role;
pub mod status;
