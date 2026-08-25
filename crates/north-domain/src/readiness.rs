//! Readiness assessment: the agent's structured verdict about one requirement revision.
//!
//! Invariant (docs/product/readiness.md): a requirement may be `Ready` only while
//! its latest assessment targets the *current* requirement revision. The
//! enforcement lives in [`crate::requirement::Requirement::mark_ready`] and the
//! edit-demotion rule; this module models the assessment and the human-review
//! packet projection.

use crate::requirement::Requirement;

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
    /// Explicit assumptions recorded by the assessor.
    pub assumptions: Vec<String>,
    /// Repositories consulted, with the commit each was inspected at.
    pub repositories_reviewed: Vec<ReviewedRepository>,
    /// Assessment time as UNIX milliseconds.
    pub assessed_at_ms: u64,
}

/// Why a review packet cannot be projected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    /// The assessment does not target the requirement's current revision;
    /// a stale packet must never be reviewable.
    StaleAssessment {
        assessment_revision: u64,
        current_revision: u64,
    },
}

/// Human-review handoff: a projection of the **current Requirement** plus the
/// latest **valid** [`ReadinessAssessment`] for exactly that revision.
///
/// Ownership split (docs/product/readiness.md):
/// - goal/scope/summary/criteria/open questions belong to the canonical
///   Requirement (source of truth for content);
/// - blockers/assessment assumptions/repositories reviewed belong to the
///   assessment (evidence about one revision).
///
/// The packet is never stored as truth; it is derived on demand so staleness is
/// structural, not procedural.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewPacket {
    pub requirement_revision: u64,
    pub goal: String,
    pub scope: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    /// Evidence owned by the assessment.
    pub blockers: Vec<String>,
    pub assessment_assumptions: Vec<String>,
    pub repositories_reviewed: Vec<ReviewedRepository>,
}

impl ReviewPacket {
    /// Projects the packet for a requirement and its latest assessment.
    /// Refuses any revision mismatch: a stale packet must never be reviewable
    /// or accepted.
    pub fn project(
        requirement: &Requirement,
        latest_valid: &ReadinessAssessment,
    ) -> Result<Self, PacketError> {
        if latest_valid.requirement_revision != requirement.revision() {
            return Err(PacketError::StaleAssessment {
                assessment_revision: latest_valid.requirement_revision,
                current_revision: requirement.revision(),
            });
        }
        Ok(Self {
            requirement_revision: requirement.revision(),
            goal: requirement.title().to_string(),
            scope: requirement.description().to_string(),
            summary: requirement.summary().to_string(),
            acceptance_criteria: requirement.acceptance_criteria().to_vec(),
            assumptions: requirement.assumptions().to_vec(),
            open_questions: requirement.open_questions().to_vec(),
            blockers: latest_valid.blockers.clone(),
            assessment_assumptions: latest_valid.assumptions.clone(),
            repositories_reviewed: latest_valid.repositories_reviewed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_projects_both_sources_when_revisions_match() {
        let mut r = Requirement::new("r1", "Login page", "Email-code login.", "u1");
        r.apply_edit(&crate::requirement::RequirementEdit {
            summary: Some("Users log in".into()),
            acceptance_criteria: Some(vec!["code arrives".into()]),
            assumptions: Some(vec!["canonical assumption".into()]),
            ..Default::default()
        })
        .unwrap();
        let assessment = ReadinessAssessment {
            requirement_revision: r.revision(),
            verdict: Verdict::Ready,
            blockers: vec!["none".into()],
            assumptions: vec!["single tenant".into()],
            repositories_reviewed: vec![ReviewedRepository {
                repository_id: "billing".into(),
                commit_sha: "a82c19f".into(),
            }],
            assessed_at_ms: 42,
        };
        let packet = ReviewPacket::project(&r, &assessment).unwrap();
        assert_eq!(packet.goal, "Login page");
        assert_eq!(packet.scope, "Email-code login.");
        assert_eq!(packet.acceptance_criteria, vec!["code arrives".to_string()]);
        assert_eq!(packet.assumptions, vec!["canonical assumption".to_string()]);
        assert_eq!(packet.blockers, vec!["none".to_string()]);
        assert_eq!(packet.repositories_reviewed.len(), 1);
    }

    #[test]
    fn stale_pair_cannot_project_a_packet() {
        let mut r = Requirement::new("r1", "t", "d", "u1");
        r.begin_discussion().unwrap();
        let assessment = ReadinessAssessment {
            requirement_revision: r.revision(),
            verdict: Verdict::Ready,
            blockers: Vec::new(),
            assumptions: Vec::new(),
            repositories_reviewed: Vec::new(),
            assessed_at_ms: 0,
        };
        r.apply_edit(&crate::requirement::RequirementEdit {
            summary: Some("changed".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            ReviewPacket::project(&r, &assessment),
            Err(PacketError::StaleAssessment {
                assessment_revision: 1,
                current_revision: 2,
            })
        );
    }
}
