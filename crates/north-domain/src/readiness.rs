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

/// Persisted accepted evidence paired with its stable identity and Ready
/// generation. Persistence constructs this only after selecting one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedReadinessAssessment {
    pub id: String,
    pub state_version: u64,
    pub assessment: ReadinessAssessment,
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
    /// The assessment was produced for a prior mutable Ready generation.
    StaleStateVersion {
        assessment_state_version: u64,
        current_state_version: u64,
    },
    /// Review decisions must identify durable assessment evidence.
    InvalidAssessmentIdentity,
    /// Only Ready requirements can produce review packets.
    NotReady,
    /// Packet evidence must be an accepted Ready assessment.
    InvalidAssessment,
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
    pub assessment_id: String,
    pub requirement_revision: u64,
    pub requirement_state_version: u64,
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
        evidence: &AcceptedReadinessAssessment,
    ) -> Result<Self, PacketError> {
        if evidence.id.trim().is_empty() {
            return Err(PacketError::InvalidAssessmentIdentity);
        }
        if evidence.assessment.requirement_revision != requirement.revision() {
            return Err(PacketError::StaleAssessment {
                assessment_revision: evidence.assessment.requirement_revision,
                current_revision: requirement.revision(),
            });
        }
        if evidence.state_version != requirement.state_version() {
            return Err(PacketError::StaleStateVersion {
                assessment_state_version: evidence.state_version,
                current_state_version: requirement.state_version(),
            });
        }
        if requirement.status() != crate::status::RequirementStatus::Ready {
            return Err(PacketError::NotReady);
        }
        if evidence.assessment.verdict != Verdict::Ready
            || !evidence.assessment.blockers.is_empty()
            || requirement
                .acceptance_criteria()
                .iter()
                .all(|value| value.trim().is_empty())
        {
            return Err(PacketError::InvalidAssessment);
        }
        Ok(Self {
            assessment_id: evidence.id.clone(),
            requirement_revision: requirement.revision(),
            requirement_state_version: requirement.state_version(),
            goal: requirement.title().to_string(),
            scope: requirement.description().to_string(),
            summary: requirement.summary().to_string(),
            acceptance_criteria: requirement.acceptance_criteria().to_vec(),
            assumptions: requirement.assumptions().to_vec(),
            open_questions: requirement.open_questions().to_vec(),
            blockers: evidence.assessment.blockers.clone(),
            assessment_assumptions: evidence.assessment.assumptions.clone(),
            repositories_reviewed: evidence.assessment.repositories_reviewed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_projects_both_sources_when_revisions_match() {
        let mut r = Requirement::new("r1", "Login page", "Email-code login.", "u1");
        r.begin_discussion().unwrap();
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
            blockers: Vec::new(),
            assumptions: vec!["single tenant".into()],
            repositories_reviewed: vec![ReviewedRepository {
                repository_id: "billing".into(),
                commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }],
            assessed_at_ms: 42,
        };
        if let Err(error) = r.mark_ready(&assessment) {
            panic!("mark ready for packet test: {error:?}");
        }
        let packet = match ReviewPacket::project(
            &r,
            &AcceptedReadinessAssessment {
                id: "assessment-1".into(),
                state_version: r.state_version(),
                assessment: assessment.clone(),
            },
        ) {
            Ok(packet) => packet,
            Err(error) => panic!("matching review packet: {error:?}"),
        };
        assert_eq!(packet.assessment_id, "assessment-1");
        assert_eq!(packet.requirement_revision, r.revision());
        assert_eq!(packet.requirement_state_version, r.state_version());
        assert_eq!(packet.goal, "Login page");
        assert_eq!(packet.scope, "Email-code login.");
        assert_eq!(packet.acceptance_criteria, vec!["code arrives".to_string()]);
        assert_eq!(packet.assumptions, vec!["canonical assumption".to_string()]);
        assert!(packet.blockers.is_empty());
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
            ReviewPacket::project(
                &r,
                &AcceptedReadinessAssessment {
                    id: "assessment-1".into(),
                    state_version: r.state_version(),
                    assessment: assessment.clone(),
                },
            ),
            Err(PacketError::StaleAssessment {
                assessment_revision: 1,
                current_revision: 2,
            })
        );
    }

    #[test]
    fn non_ready_requirement_cannot_project_packet() {
        let requirement = Requirement::new("r1", "Title", "Description", "u1");
        let assessment = ReadinessAssessment {
            requirement_revision: 1,
            verdict: Verdict::Ready,
            blockers: Vec::new(),
            assumptions: Vec::new(),
            repositories_reviewed: Vec::new(),
            assessed_at_ms: 0,
        };
        assert_eq!(
            ReviewPacket::project(
                &requirement,
                &AcceptedReadinessAssessment {
                    id: "assessment-1".into(),
                    state_version: 1,
                    assessment,
                },
            ),
            Err(PacketError::NotReady)
        );
    }

    #[test]
    fn stale_ready_generation_cannot_project_packet() {
        let mut requirement = Requirement::new("r1", "Title", "Description", "u1");
        assert!(requirement.begin_discussion().is_ok());
        assert!(requirement
            .apply_edit(&crate::requirement::RequirementEdit {
                acceptance_criteria: Some(vec!["criterion".into()]),
                ..Default::default()
            })
            .is_ok());
        let assessment = ReadinessAssessment {
            requirement_revision: requirement.revision(),
            verdict: Verdict::Ready,
            blockers: Vec::new(),
            assumptions: Vec::new(),
            repositories_reviewed: Vec::new(),
            assessed_at_ms: 0,
        };
        assert!(requirement.mark_ready(&assessment).is_ok());
        assert_eq!(
            ReviewPacket::project(
                &requirement,
                &AcceptedReadinessAssessment {
                    id: "assessment-1".into(),
                    state_version: requirement.state_version() - 1,
                    assessment: assessment.clone(),
                },
            ),
            Err(PacketError::StaleStateVersion {
                assessment_state_version: 3,
                current_state_version: 4,
            })
        );
    }

    #[test]
    fn invalid_assessment_cannot_project_packet() {
        let mut requirement = Requirement::new("r1", "Title", "Description", "u1");
        assert!(requirement.begin_discussion().is_ok());
        assert!(requirement
            .apply_edit(&crate::requirement::RequirementEdit {
                acceptance_criteria: Some(vec!["criterion".into()]),
                ..Default::default()
            })
            .is_ok());
        let valid_assessment = ReadinessAssessment {
            requirement_revision: requirement.revision(),
            verdict: Verdict::Ready,
            blockers: Vec::new(),
            assumptions: Vec::new(),
            repositories_reviewed: Vec::new(),
            assessed_at_ms: 0,
        };
        assert!(requirement.mark_ready(&valid_assessment).is_ok());
        let invalid_assessment = ReadinessAssessment {
            blockers: vec!["blocker".into()],
            ..valid_assessment
        };
        assert_eq!(
            ReviewPacket::project(
                &requirement,
                &AcceptedReadinessAssessment {
                    id: "assessment-1".into(),
                    state_version: requirement.state_version(),
                    assessment: invalid_assessment,
                },
            ),
            Err(PacketError::InvalidAssessment)
        );
    }
}
