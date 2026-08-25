//! Requirement business lifecycle.
//!
//! Canonical diagram lives in docs/product/requirement-lifecycle.md:
//!
//! ```text
//! Draft ──▶ Discussing ──▶ Ready ──▶ Accepted
//!                             │  ▲         (human)
//!              request change │  │ reopen
//!                             ▼  │
//!                          (back to Discussing / from Rejected)
//!                             └──▶ Rejected ──reopen──▶ Discussing
//! ```

/// Business lifecycle state of a requirement. Infrastructure/runtime health
/// (Idle/Running/Retrying/Failed) is deliberately NOT part of this enum; see
/// docs/product/requirement-lifecycle.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementStatus {
    Draft,
    Discussing,
    Ready,
    Accepted,
    Rejected,
}

impl RequirementStatus {
    /// Stable lowercase identifier used by API/UI/persistence mappings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Discussing => "discussing",
            Self::Ready => "ready",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    /// Whether `self → next` is legal. Illegal transitions must stay
    /// unrepresentable through [`crate::requirement::Requirement`] APIs.
    pub fn can_transition_to(self, next: Self) -> bool {
        use RequirementStatus::*;
        matches!(
            (self, next),
            (Draft, Discussing)
                | (Discussing, Ready)
                | (Ready, Discussing) // request changes, or stale-assessment demotion
                | (Ready, Accepted) // human only
                | (Ready, Rejected) // human only
                | (Rejected, Discussing) // reopen
        )
    }
}

/// A refused lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: RequirementStatus,
    pub to: RequirementStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_edges_match_product_lifecycle() {
        use RequirementStatus::*;
        for (from, to) in [
            (Draft, Discussing),
            (Discussing, Ready),
            (Ready, Discussing),
            (Ready, Accepted),
            (Ready, Rejected),
            (Rejected, Discussing),
        ] {
            assert!(
                from.can_transition_to(to),
                "{from:?} → {to:?} must be legal"
            );
        }
    }

    #[test]
    fn all_other_edges_are_illegal() {
        use RequirementStatus::*;
        let all = [Draft, Discussing, Ready, Accepted, Rejected];
        for from in all {
            for to in all {
                let legal = matches!(
                    (from, to),
                    (Draft, Discussing)
                        | (Discussing, Ready)
                        | (Ready, Discussing)
                        | (Ready, Accepted)
                        | (Ready, Rejected)
                        | (Rejected, Discussing)
                );
                assert_eq!(from.can_transition_to(to), legal, "{from:?} → {to:?}");
            }
        }
        // Terminal states are truly terminal without human action.
        assert!(!Accepted.can_transition_to(Discussing));
        assert!(!Accepted.can_transition_to(Ready));
    }
}
