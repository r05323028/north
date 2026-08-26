//! Explicit conversion from North readiness evidence into domain assessment.
//!
//! This boundary belongs to `north-server`: the protocol crate stays pure wire
//! DTOs while the server applies the domain representation and later business
//! gates.

use north_domain::readiness::{ReadinessAssessment, ReviewedRepository, Verdict};
use north_protocol::{FrameError, ReadinessVerdictWire, RequirementAssessed};

pub fn readiness_assessment_from_wire(
    wire: &RequirementAssessed,
    assessed_at_ms: u64,
) -> Result<ReadinessAssessment, FrameError> {
    wire.validate()?;
    Ok(ReadinessAssessment {
        requirement_revision: wire.requirement_revision,
        verdict: match wire.verdict {
            ReadinessVerdictWire::Ready => Verdict::Ready,
            ReadinessVerdictWire::NeedsClarification => Verdict::NeedsClarification,
        },
        blockers: wire.blockers.clone(),
        assumptions: wire.assumptions.clone(),
        repositories_reviewed: wire
            .repositories_reviewed
            .iter()
            .map(|repository| ReviewedRepository {
                repository_id: repository.repository_id.clone(),
                commit_sha: repository.commit_sha.clone(),
            })
            .collect(),
        assessed_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_typed_wire_evidence_without_protocol_domain_dependency() {
        let assessment = readiness_assessment_from_wire(
            &RequirementAssessed {
                requirement_id: "requirement-1".into(),
                requirement_revision: 2,
                verdict: ReadinessVerdictWire::Ready,
                blockers: Vec::new(),
                assumptions: vec!["One account".into()],
                repositories_reviewed: vec![north_protocol::ReviewedRepositoryWire {
                    repository_id: "north".into(),
                    commit_sha: "abc123".into(),
                }],
            },
            42,
        )
        .expect("valid wire evidence");

        assert_eq!(assessment.requirement_revision, 2);
        assert_eq!(assessment.verdict, Verdict::Ready);
        assert_eq!(assessment.repositories_reviewed[0].commit_sha, "abc123");
        assert_eq!(assessment.assessed_at_ms, 42);
    }
}
