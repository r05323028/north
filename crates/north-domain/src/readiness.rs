//! Readiness assessment: the agent's structured verdict about one requirement revision.
//!
//! Invariant (docs/product/readiness.md): a requirement may be `Ready` only while
//! its latest assessment targets the *current* requirement revision. The
//! enforcement lives in [`crate::requirement::Requirement::mark_ready`] and the
//! edit-demotion rule; this module only models the assessment itself.

/// Agent verdict. `Ready` claims the requirement is clear enough for human review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    NeedsClarification,
}

/// Source identity of a repository inspected during assessment.
/// `commit_sha` preserves what the assessment was actually based on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedRepository {
    pub repository_id: String,
    pub commit_sha: String,
}

/// Structured result of the agent's readiness evaluation of exactly one revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessAssessment {
    /// Requirement revision this assessment was produced against.
    pub requirement_revision: u64,
    pub verdict: Verdict,
    /// Unresolved issues that would materially change scope, observable
    /// behavior, or acceptance criteria. Non-empty blocks Ready.
    pub blockers: Vec<String>,
    /// Explicit assumptions the reviewer should be aware of.
    pub assumptions: Vec<String>,
    /// Repositories consulted, with the commit each was inspected at.
    pub repositories_reviewed: Vec<ReviewedRepository>,
    /// Assessment time as UNIX milliseconds.
    pub assessed_at_ms: u64,
}
