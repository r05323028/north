//! The requirement aggregate: identity, structured content, lifecycle.
//!
//! Encapsulation invariant: callers may inspect Requirement state through
//! accessors but CANNOT bypass business rules to mutate it — invariant-bearing
//! fields are private and every change flows through an explicit operation.
//!
//! Edge-vs-operation invariant: a legal lifecycle *edge* does not make any
//! operation targeting the destination state legal from any source. Each
//! operation below names its single allowed source state:
//!
//! ```text
//! begin_discussion : Draft       -> Discussing
//! mark_ready       : Discussing  -> Ready      (+ readiness gates)
//! request_changes  : Ready       -> Discussing
//! accept           : Ready       -> Accepted
//! reject           : Ready       -> Rejected
//! reopen           : Rejected    -> Discussing
//! ```
//!
//! Revision invariant: `revision` increments ONLY when canonical structured
//! content actually changes; no-op edits neither invalidate assessments nor
//! demote Ready.

use crate::readiness::{ReadinessAssessment, Verdict};
use crate::status::{InvalidTransition, RequirementStatus};

/// Structured requirement, intentionally small for 0.1.0 (docs/product).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    id: String,
    title: String,
    description: String,
    summary: String,
    acceptance_criteria: Vec<String>,
    assumptions: Vec<String>,
    open_questions: Vec<String>,
    status: RequirementStatus,
    revision: u64,
    state_version: u64,
    created_by: String,
}

/// Complete state needed to reconstitute a Requirement from durable storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedRequirement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
    pub status: RequirementStatus,
    pub revision: u64,
    pub state_version: u64,
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

/// Why persisted Requirement state could not be reconstituted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    /// Revisions start at one and never use zero.
    InvalidRevision,
    /// State versions start at one and never use zero.
    InvalidStateVersion,
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
    /// Current lifecycle state forbids entering `Ready` (only Discussing may).
    Transition(InvalidTransition),
}

/// Why a content edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Terminal states refuse direct edits; reopen or start a new requirement.
    StateForbidsEdit { status: RequirementStatus },
    /// Version tokens cannot be incremented beyond their representable range.
    VersionExhausted,
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
            state_version: 1,
            created_by: created_by.into(),
        }
    }

    /// Reconstitutes state read from persistence without exposing setters.
    pub fn from_persisted(state: PersistedRequirement) -> Result<Self, RestoreError> {
        if state.revision == 0 {
            return Err(RestoreError::InvalidRevision);
        }
        if state.state_version == 0 {
            return Err(RestoreError::InvalidStateVersion);
        }
        Ok(Self {
            id: state.id,
            title: state.title,
            description: state.description,
            summary: state.summary,
            acceptance_criteria: state.acceptance_criteria,
            assumptions: state.assumptions,
            open_questions: state.open_questions,
            status: state.status,
            revision: state.revision,
            state_version: state.state_version,
            created_by: state.created_by,
        })
    }

    // ---- Read-only accessors (no generic setters exist) ----

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn acceptance_criteria(&self) -> &[String] {
        &self.acceptance_criteria
    }

    pub fn assumptions(&self) -> &[String] {
        &self.assumptions
    }

    pub fn open_questions(&self) -> &[String] {
        &self.open_questions
    }

    pub fn status(&self) -> RequirementStatus {
        self.status
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn state_version(&self) -> u64 {
        self.state_version
    }

    pub fn created_by(&self) -> &str {
        &self.created_by
    }

    // ---- Business operations (each pins its allowed source state) ----

    fn transition_from(
        &mut self,
        expected_source: RequirementStatus,
        next: RequirementStatus,
    ) -> Result<(), InvalidTransition> {
        if self.status != expected_source || !self.status.can_transition_to(next) {
            return Err(InvalidTransition {
                from: self.status,
                to: next,
            });
        }
        let next_state_version = self.state_version.checked_add(1).ok_or(InvalidTransition {
            from: self.status,
            to: next,
        })?;
        self.status = next;
        self.state_version = next_state_version;
        Ok(())
    }

    /// Draft → Discussing only: clarification begins.
    pub fn begin_discussion(&mut self) -> Result<(), InvalidTransition> {
        self.transition_from(RequirementStatus::Draft, RequirementStatus::Discussing)
    }

    /// Discussing → Ready only, plus hard readiness gates:
    /// assessment bound to the current revision, Ready verdict, no blockers,
    /// and existing acceptance criteria.
    pub fn mark_ready(&mut self, assessment: &ReadinessAssessment) -> Result<(), MarkReadyError> {
        if self.status != RequirementStatus::Discussing {
            return Err(MarkReadyError::Transition(InvalidTransition {
                from: self.status,
                to: RequirementStatus::Ready,
            }));
        }
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
        self.transition_from(RequirementStatus::Discussing, RequirementStatus::Ready)
            .map_err(MarkReadyError::Transition)
    }

    /// Ready → Accepted only (human decision).
    pub fn accept(&mut self) -> Result<(), InvalidTransition> {
        self.transition_from(RequirementStatus::Ready, RequirementStatus::Accepted)
    }

    /// Ready → Rejected only (human decision).
    pub fn reject(&mut self) -> Result<(), InvalidTransition> {
        self.transition_from(RequirementStatus::Ready, RequirementStatus::Rejected)
    }

    /// Ready → Discussing only (human Request Changes; feedback preserved by callers).
    pub fn request_changes(&mut self) -> Result<(), InvalidTransition> {
        self.transition_from(RequirementStatus::Ready, RequirementStatus::Discussing)
    }

    /// Rejected → Discussing only (human Reopen).
    pub fn reopen(&mut self) -> Result<(), InvalidTransition> {
        self.transition_from(RequirementStatus::Rejected, RequirementStatus::Discussing)
    }

    /// Applies structured content edits.
    ///
    /// Revision increments ONLY when canonical content actually changes:
    /// empty and same-value edits are no-ops (no bump, no assessment
    /// invalidation, no Ready demotion). A real edit while Ready bumps the
    /// revision once and demotes to Discussing (stale-assessment demotion).
    /// Terminal states refuse edits outright.
    pub fn apply_edit(&mut self, edit: &RequirementEdit) -> Result<u64, EditError> {
        if matches!(
            self.status,
            RequirementStatus::Accepted | RequirementStatus::Rejected
        ) {
            return Err(EditError::StateForbidsEdit {
                status: self.status,
            });
        }
        let candidate_title = edit.title.clone().unwrap_or_else(|| self.title.clone());
        let candidate_description = edit
            .description
            .clone()
            .unwrap_or_else(|| self.description.clone());
        let candidate_summary = edit.summary.clone().unwrap_or_else(|| self.summary.clone());
        let candidate_criteria = edit
            .acceptance_criteria
            .clone()
            .unwrap_or_else(|| self.acceptance_criteria.clone());
        let candidate_assumptions = edit
            .assumptions
            .clone()
            .unwrap_or_else(|| self.assumptions.clone());
        let candidate_open = edit
            .open_questions
            .clone()
            .unwrap_or_else(|| self.open_questions.clone());

        let unchanged = candidate_title == self.title
            && candidate_description == self.description
            && candidate_summary == self.summary
            && candidate_criteria == self.acceptance_criteria
            && candidate_assumptions == self.assumptions
            && candidate_open == self.open_questions;
        if unchanged {
            return Ok(self.revision);
        }

        let was_ready = self.status == RequirementStatus::Ready;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(EditError::VersionExhausted)?;
        let next_state_version = self
            .state_version
            .checked_add(1)
            .ok_or(EditError::VersionExhausted)?;
        self.title = candidate_title;
        self.description = candidate_description;
        self.summary = candidate_summary;
        self.acceptance_criteria = candidate_criteria;
        self.assumptions = candidate_assumptions;
        self.open_questions = candidate_open;
        self.revision = next_revision;
        self.state_version = next_state_version;
        if was_ready {
            self.status = RequirementStatus::Discussing;
        }
        Ok(self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readiness::{AcceptedReadinessAssessment, ReviewPacket};

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

    fn discuss_with_criteria(r: &mut Requirement) {
        r.begin_discussion().unwrap();
        r.apply_edit(&RequirementEdit {
            acceptance_criteria: Some(vec!["criteria".into()]),
            ..Default::default()
        })
        .unwrap();
    }

    #[test]
    fn happy_lifecycle_draft_to_accepted() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        r.mark_ready(&ready_assessment(r.revision())).unwrap();
        assert_eq!(r.status(), RequirementStatus::Ready);
        r.accept().unwrap();
        assert_eq!(r.status(), RequirementStatus::Accepted);
        assert_eq!(r.revision(), 2);
        assert_eq!(r.state_version(), 5);
    }

    #[test]
    fn operations_are_source_state_specific() {
        let mut draft = sample();
        assert!(draft.reopen().is_err()); // Draft.reopen forbidden
        assert!(draft.request_changes().is_err()); // Draft.request_changes forbidden
        assert!(draft.accept().is_err());
        assert!(draft.reject().is_err());

        let mut discussing = sample();
        discussing.begin_discussion().unwrap();
        assert!(discussing.reopen().is_err()); // Discussing.reopen forbidden
        assert!(discussing.accept().is_err());
        assert!(discussing.reject().is_err());
        assert!(discussing.begin_discussion().is_err());

        let rejected = {
            let mut r = sample();
            discuss_with_criteria(&mut r);
            r.mark_ready(&ready_assessment(r.revision())).unwrap();
            r.reject().unwrap();
            r
        };
        let mut rejected = rejected;
        assert!(rejected.begin_discussion().is_err()); // Rejected.begin_discussion forbidden
        assert!(rejected.accept().is_err());
        rejected.reopen().unwrap(); // Rejected.reopen is THE reopen path
        assert_eq!(rejected.status(), RequirementStatus::Discussing);
    }

    #[test]
    fn request_changes_only_from_ready() {
        let mut r = sample();
        assert!(r.request_changes().is_err()); // Draft
        discuss_with_criteria(&mut r);
        r.mark_ready(&ready_assessment(r.revision())).unwrap();
        r.request_changes().unwrap();
        assert_eq!(r.status(), RequirementStatus::Discussing);
    }

    #[test]
    fn stale_assessment_cannot_make_ready() {
        let mut r = sample();
        discuss_with_criteria(&mut r); // revision 2
        let stale = ready_assessment(r.revision() - 1);
        assert_eq!(
            r.mark_ready(&stale),
            Err(MarkReadyError::StaleAssessment {
                assessment_revision: stale.requirement_revision,
                current_revision: r.revision(),
            })
        );
    }

    #[test]
    fn ready_entry_requires_discussing_state() {
        let mut draft_with_criteria = sample();
        draft_with_criteria
            .apply_edit(&RequirementEdit {
                acceptance_criteria: Some(vec!["criteria".into()]),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            draft_with_criteria.mark_ready(&ready_assessment(draft_with_criteria.revision())),
            Err(MarkReadyError::Transition(_))
        ));
    }

    #[test]
    fn blocked_verdict_cannot_make_ready() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        let mut a = ready_assessment(r.revision());
        a.blockers = vec!["scope unclear for SSO".into()];
        assert_eq!(r.mark_ready(&a), Err(MarkReadyError::BlockersPresent));
        a.blockers.clear();
        a.verdict = Verdict::NeedsClarification;
        assert_eq!(r.mark_ready(&a), Err(MarkReadyError::VerdictNotReady));
    }

    #[test]
    fn empty_and_same_value_edits_are_noops() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        let rev_before = r.revision();
        let state_version_before = r.state_version();
        // Empty edit.
        assert_eq!(
            r.apply_edit(&RequirementEdit::default()).unwrap(),
            rev_before
        );
        // Same-value edit.
        assert_eq!(
            r.apply_edit(&RequirementEdit {
                title: Some("Login page".into()),
                acceptance_criteria: Some(vec!["criteria".into()]),
                ..Default::default()
            })
            .unwrap(),
            rev_before
        );
        assert_eq!(r.revision(), rev_before);
        assert_eq!(r.state_version(), state_version_before);
        assert_eq!(r.status(), RequirementStatus::Discussing);
    }

    #[test]
    fn noop_edit_while_ready_does_not_demote_or_invalidate() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        r.mark_ready(&ready_assessment(r.revision())).unwrap();
        let rev = r.revision();
        let state_version = r.state_version();
        r.apply_edit(&RequirementEdit::default()).unwrap();
        assert_eq!(r.revision(), rev);
        assert_eq!(r.state_version(), state_version);
        assert_eq!(r.status(), RequirementStatus::Ready);
        // The assessment is still valid.
        assert!(ReviewPacket::project(
            &r,
            &AcceptedReadinessAssessment {
                id: "assessment-1".into(),
                state_version: r.state_version(),
                assessment: ready_assessment(rev),
            },
        )
        .is_ok());
    }

    #[test]
    fn actual_edit_bumps_exactly_once() {
        let mut r = sample();
        let rev_before = r.revision();
        let state_version_before = r.state_version();
        let rev = r
            .apply_edit(&RequirementEdit {
                summary: Some("updated".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rev, rev_before + 1);
        assert_eq!(r.state_version(), state_version_before + 1);
    }

    #[test]
    fn editing_ready_demotes_to_discussing_and_bumps_revision() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        r.mark_ready(&ready_assessment(r.revision())).unwrap();
        let rev_before = r.revision();
        let state_version_before = r.state_version();
        let new_rev = r
            .apply_edit(&RequirementEdit {
                summary: Some("updated".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(new_rev, rev_before + 1);
        assert_eq!(r.state_version(), state_version_before + 1);
        assert_eq!(r.status(), RequirementStatus::Discussing);
        // Old assessment is stale by construction.
        assert!(r.mark_ready(&ready_assessment(rev_before)).is_err());
    }

    #[test]
    fn terminal_states_refuse_edits_but_reject_reopens() {
        let mut r = sample();
        discuss_with_criteria(&mut r);
        r.mark_ready(&ready_assessment(r.revision())).unwrap();
        r.reject().unwrap();
        assert!(r.apply_edit(&RequirementEdit::default()).is_err());
        r.reopen().unwrap();
        assert_eq!(r.status(), RequirementStatus::Discussing);
    }

    #[test]
    fn missing_criteria_block_ready() {
        let mut r = sample();
        r.begin_discussion().unwrap();
        assert_eq!(
            r.mark_ready(&ready_assessment(r.revision())),
            Err(MarkReadyError::MissingAcceptanceCriteria)
        );
    }

    #[test]
    fn restore_rejects_zero_state_version() {
        let state = PersistedRequirement {
            id: "r1".into(),
            title: "title".into(),
            description: "description".into(),
            summary: String::new(),
            acceptance_criteria: Vec::new(),
            assumptions: Vec::new(),
            open_questions: Vec::new(),
            status: RequirementStatus::Draft,
            revision: 1,
            state_version: 0,
            created_by: "u1".into(),
        };
        assert_eq!(
            Requirement::from_persisted(state),
            Err(RestoreError::InvalidStateVersion)
        );
    }

    #[test]
    fn accessors_expose_state_without_mutation_paths() {
        let r = sample();
        // Read-only surface compiles and returns borrowed data; there are no
        // setters, so callers cannot bypass operations (enforced by privacy).
        assert_eq!(r.id(), "r1");
        assert_eq!(r.title(), "Login page");
        assert_eq!(r.created_by(), "u1");
        assert_eq!(r.status(), RequirementStatus::Draft);
        assert_eq!(r.revision(), 1);
        assert_eq!(r.state_version(), 1);
        assert!(r.acceptance_criteria().is_empty());
    }
}
