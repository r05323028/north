//! The requirement aggregate: identity, structured content, lifecycle.

use crate::readiness::{ReadinessAssessment, Verdict};
use crate::status::{InvalidTransition, RequirementStatus};

/// Structured requirement, intentionally small for 0.1.0 (docs/product).
///
/// Invariants owned here:
/// - lifecycle transitions follow [`RequirementStatus::can_transition_to`];
/// - `revision` increments on every accepted content edit;
/// - `Ready` requires an assessment bound to the current revision;
/// - editing a `Ready` requirement demotes it to `Discussing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub status: RequirementStatus,
    pub revision: u64,
    pub created_by: String,
}

/// Content edit applied via [`Requirement::apply_edit`]. `None` fields are unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementEdit {
    pub title: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub assumptions: Option<Vec<String>>,
    pub open_questions: Option<Vec<String>>,
}

/// Why entering `Ready` was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkReadyError {
    /// The assessment targets an older revision; it is stale by construction.
    StaleAssessment {
        assessment_revision: u64,
        current_revision: u64,
    },
    /// The agent did not issue a Ready verdict.
    VerdictNotReady,
    /// The assessment records unresolved blockers.
    BlockersPresent,
    /// No meaningful acceptance criteria captured yet.
    MissingAcceptanceCriteria,
    /// Current lifecycle state forbids entering `Ready`.
    Transition(InvalidTransition),
}

/// Why a content edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Terminal states refuse direct edits; reopen or start a new requirement.
    StateForbidsEdit { status: RequirementStatus },
}

impl Requirement {
    /// Creates a `Draft` requirement at revision 1.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            summary: String::new(),
            acceptance_criteria: Vec::new(),
            assumptions: Vec::new(),
            open_questions: Vec::new(),
            status: RequirementStatus::Draft,
            revision: 1,
            created_by: created_by.into(),
        }
    }

    fn transition(&mut self, next: RequirementStatus) -> Result<(), InvalidTransition> {
        if self.status.can_transition_to(next) {
            self.status = next;
            Ok(())
        } else {
            Err(InvalidTransition {
                from: self.status,
                to: next,
            })
        }
    }

    /// Draft → Discussing: clarification begins.
    pub fn begin_discussion(&mut self) -> Result<(), InvalidTransition> {
        self.transition(RequirementStatus::Discussing)
    }

    /// Applies the agent's verdict and moves the requirement to `Ready`.
    ///
    /// Hard gates encoded here (semantic judgment stays with the agent):
    /// assessment targets the current revision, verdict is `Ready`, no
    /// unresolved blockers, and meaningful acceptance criteria exist.
    pub fn mark_ready(&mut self, assessment: &ReadinessAssessment) -> Result<(), MarkReadyError> {
        if assessment.requirement_revision != self.revision {
            return Err(MarkReadyError::StaleAssessment {
                assessment_revision: assessment.requirement_revision,
                current_revision: self.revision,
            });
        }
        if assessment.verdict != Verdict::Ready {
            return Err(MarkReadyError::VerdictNotReady);
        }
        if !assessment.blockers.is_empty() {
            return Err(MarkReadyError::BlockersPresent);
        }
        if self.acceptance_criteria.iter().all(|c| c.trim().is_empty()) {
            return Err(MarkReadyError::MissingAcceptanceCriteria);
        }
        self.transition(RequirementStatus::Ready)
            .map_err(MarkReadyError::Transition)
    }

    /// Human decision (Requirement Manager / Admin / Owner): Ready → Accepted.
    pub fn accept(&mut self) -> Result<(), InvalidTransition> {
        self.transition(RequirementStatus::Accepted)
    }

    /// Human decision: Ready → Rejected.
    pub fn reject(&mut self) -> Result<(), InvalidTransition> {
        self.transition(RequirementStatus::Rejected)
    }

    /// Human decision: Ready → Discussing, reviewer feedback preserved by callers.
    pub fn request_changes(&mut self) -> Result<(), InvalidTransition> {
        self.transition(RequirementStatus::Discussing)
    }

    /// Human decision: Rejected → Discussing.
    pub fn reopen(&mut self) -> Result<(), InvalidTransition> {
        self.transition(RequirementStatus::Discussing)
    }

    /// Applies structured content edits.
    ///
    /// Every applied edit bumps `revision`. Editing a `Ready` requirement makes
    /// its assessment stale, so it demotes to `Discussing`; the agent must
    /// re-assess before it can return to `Ready`. This does not depend on the
    /// agent remembering anything.
    pub fn apply_edit(&mut self, edit: &RequirementEdit) -> Result<u64, EditError> {
        if matches!(
            self.status,
            RequirementStatus::Accepted | RequirementStatus::Rejected
        ) {
            return Err(EditError::StateForbidsEdit {
                status: self.status,
            });
        }
        let RequirementEdit {
            title,
            description,
            summary,
            acceptance_criteria,
            assumptions,
            open_questions,
        } = edit;
        if let Some(v) = title {
            self.title = v.clone();
        }
        if let Some(v) = description {
            self.description = v.clone();
        }
        if let Some(v) = summary {
            self.summary = v.clone();
        }
        if let Some(v) = acceptance_criteria {
            self.acceptance_criteria = v.clone();
        }
        if let Some(v) = assumptions {
            self.assumptions = v.clone();
        }
        if let Some(v) = open_questions {
            self.open_questions = v.clone();
        }
        let was_ready = self.status == RequirementStatus::Ready;
        self.revision += 1;
        if was_ready {
            self.status = RequirementStatus::Discussing;
        }
        Ok(self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Requirement {
        Requirement::new("r1", "Login page", "Users log in with email codes.", "u1")
    }

    fn ready_assessment(rev: u64) -> ReadinessAssessment {
        ReadinessAssessment {
            requirement_revision: rev,
            verdict: Verdict::Ready,
            blockers: Vec::new(),
            assumptions: vec!["single-tenant instance".into()],
            repositories_reviewed: Vec::new(),
            assessed_at_ms: 0,
        }
    }

    #[test]
    fn happy_lifecycle_draft_to_accepted() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        r.acceptance_criteria = vec!["code arrives within 5 minutes".into()];
        r.mark_ready(&ready_assessment(r.revision)).unwrap();
        assert_eq!(r.status, RequirementStatus::Ready);
        r.accept().unwrap();
        assert_eq!(r.status, RequirementStatus::Accepted);
    }

    #[test]
    fn illegal_transitions_are_refused() {
        let mut r = sample();
        assert!(r.mark_ready(&ready_assessment(r.revision)).is_err()); // Draft → Ready
                                                                       // reopen() targets Discussing, which is also the legal Draft ->
                                                                       // Discussing edge, so it must succeed here; its Rejected-only entry
                                                                       // point is exercised by terminal_states_refuse_edits_but_reject_reopens.
        r.reopen().unwrap();
        assert_eq!(r.status, RequirementStatus::Discussing);
        let mut fresh = sample();
        assert!(fresh.accept().is_err());
        assert!(fresh.reject().is_err());
        fresh.begin_discussion().unwrap();
        assert!(r.accept().is_err()); // Discussing → Accepted forbidden
        assert!(r.begin_discussion().is_err()); // Discussing → Discussing forbidden
    }

    #[test]
    fn stale_assessment_cannot_make_ready() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        r.apply_edit(&RequirementEdit {
            acceptance_criteria: Some(vec!["criteria".into()]),
            ..Default::default()
        })
        .unwrap();
        let stale = ready_assessment(r.revision - 1);
        assert_eq!(
            r.mark_ready(&stale),
            Err(MarkReadyError::StaleAssessment {
                assessment_revision: stale.requirement_revision,
                current_revision: r.revision,
            })
        );
    }

    #[test]
    fn blocked_verdict_cannot_make_ready() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        r.acceptance_criteria = vec!["criteria".into()];
        let mut a = ready_assessment(r.revision);
        a.blockers = vec!["scope unclear for SSO".into()];
        assert_eq!(r.mark_ready(&a), Err(MarkReadyError::BlockersPresent));
        a.blockers.clear();
        a.verdict = Verdict::NeedsClarification;
        assert_eq!(r.mark_ready(&a), Err(MarkReadyError::VerdictNotReady));
    }

    #[test]
    fn editing_ready_demotes_to_discussing_and_bumps_revision() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        r.acceptance_criteria = vec!["criteria".into()];
        r.mark_ready(&ready_assessment(r.revision)).unwrap();
        let rev_before = r.revision;
        let new_rev = r
            .apply_edit(&RequirementEdit {
                summary: Some("updated".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(new_rev, rev_before + 1);
        assert_eq!(r.status, RequirementStatus::Discussing);
        // Old assessment is now stale by construction.
        assert!(r.mark_ready(&ready_assessment(rev_before)).is_err());
    }

    #[test]
    fn terminal_states_refuse_edits_but_reject_reopens() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        r.acceptance_criteria = vec!["criteria".into()];
        r.mark_ready(&ready_assessment(r.revision)).unwrap();
        r.reject().unwrap();
        assert!(r.apply_edit(&RequirementEdit::default()).is_err());
        r.reopen().unwrap();
        assert_eq!(r.status, RequirementStatus::Discussing);
    }

    #[test]
    fn missing_criteria_block_ready() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        assert_eq!(
            r.mark_ready(&ready_assessment(r.revision)),
            Err(MarkReadyError::MissingAcceptanceCriteria)
        );
    }
}
