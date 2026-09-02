//! Server-owned assembly of the runtime context sent in `session.start`.
//!
//! These inputs are server snapshots, not domain or persistence types. The
//! conversion creates transport DTOs only after the server has selected the
//! bounded conversation excerpt and filtered configured repositories.

use north_domain::requirement::Requirement;
use north_persistence::{MAX_CONTEXT_BYTES, MAX_CONTEXT_MESSAGES};
use north_protocol::{
    ConversationContext, ConversationMessageContext, ConversationRoleWire, FrameError,
    RepositoryContext, RequirementContext, SessionStart,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementSnapshot {
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub description: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
    pub assumptions: Vec<String>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationRole {
    Requester,
    Agent,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessageSnapshot {
    pub message_id: String,
    pub role: ConversationRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub repository_id: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub enabled: bool,
}

impl From<&Requirement> for RequirementSnapshot {
    fn from(requirement: &Requirement) -> Self {
        Self {
            id: requirement.id().to_string(),
            revision: requirement.revision(),
            title: requirement.title().to_string(),
            description: requirement.description().to_string(),
            summary: requirement.summary().to_string(),
            acceptance_criteria: requirement.acceptance_criteria().to_vec(),
            assumptions: requirement.assumptions().to_vec(),
            open_questions: requirement.open_questions().to_vec(),
        }
    }
}

impl From<RequirementSnapshot> for RequirementContext {
    fn from(snapshot: RequirementSnapshot) -> Self {
        Self {
            id: snapshot.id,
            revision: snapshot.revision,
            title: snapshot.title,
            description: snapshot.description,
            summary: snapshot.summary,
            acceptance_criteria: snapshot.acceptance_criteria,
            assumptions: snapshot.assumptions,
            open_questions: snapshot.open_questions,
        }
    }
}

impl From<ConversationRole> for ConversationRoleWire {
    fn from(role: ConversationRole) -> Self {
        match role {
            ConversationRole::Requester => Self::Requester,
            ConversationRole::Agent => Self::Agent,
            ConversationRole::System => Self::System,
        }
    }
}

impl From<ConversationMessageSnapshot> for ConversationMessageContext {
    fn from(snapshot: ConversationMessageSnapshot) -> Self {
        Self {
            message_id: snapshot.message_id,
            role: snapshot.role.into(),
            content: snapshot.content,
        }
    }
}

impl From<RepositorySnapshot> for RepositoryContext {
    fn from(snapshot: RepositorySnapshot) -> Self {
        Self {
            repository_id: snapshot.repository_id,
            name: snapshot.name,
            url: snapshot.url,
            description: snapshot.description,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSelectionError {
    StartMessageNotFound,
    InvalidBounds,
}

/// Select a deterministic provider-independent conversation excerpt.
/// Newest fitting messages win; the start message is retained even when it
/// requires removing older non-start messages.
pub fn select_conversation_excerpt(
    messages: &[ConversationMessageSnapshot],
    start_message_id: &str,
) -> Result<Vec<ConversationMessageSnapshot>, ContextSelectionError> {
    if start_message_id.trim().is_empty() {
        return Err(ContextSelectionError::StartMessageNotFound);
    }
    let Some(start_index) = messages
        .iter()
        .position(|message| message.message_id == start_message_id)
    else {
        return Err(ContextSelectionError::StartMessageNotFound);
    };
    let mut selected = vec![true; messages.len()];
    let mut selected_count = messages.len();
    let mut bytes = messages.iter().fold(0usize, |total, message| {
        total.saturating_add(message.content.len())
    });
    let mut oldest = 0;
    while selected_count > MAX_CONTEXT_MESSAGES || bytes > MAX_CONTEXT_BYTES {
        while oldest < messages.len() && (oldest == start_index || !selected[oldest]) {
            oldest += 1;
        }
        if oldest == messages.len() {
            break;
        }
        selected[oldest] = false;
        selected_count -= 1;
        bytes = bytes.saturating_sub(messages[oldest].content.len());
        oldest += 1;
    }
    Ok(messages
        .iter()
        .enumerate()
        .filter(|(index, _)| selected[*index])
        .map(|(_, message)| message.clone())
        .collect())
}

/// Assemble complete runtime context before the server dispatches `session.start`.
/// The caller supplies a bounded/relevant conversation excerpt; disabled
/// repositories are excluded and no credential-bearing fields exist in inputs.
pub fn assemble_session_start(
    requirement: RequirementSnapshot,
    conversation_excerpt: Vec<ConversationMessageSnapshot>,
    repositories: Vec<RepositorySnapshot>,
) -> Result<SessionStart, FrameError> {
    let start = SessionStart {
        requirement: requirement.into(),
        conversation: ConversationContext {
            excerpt: conversation_excerpt.into_iter().map(Into::into).collect(),
        },
        repositories: repositories
            .into_iter()
            .filter(|repository| repository.enabled)
            .map(Into::into)
            .collect(),
    };
    start.validate()?;
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_requirement_converts_to_server_snapshot() {
        let requirement = Requirement::new("requirement-1", "Login", "Clarify login", "user-1");
        let snapshot = RequirementSnapshot::from(&requirement);
        assert_eq!(snapshot.id, "requirement-1");
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.title, "Login");
    }

    #[test]
    fn assembles_context_and_filters_disabled_repositories() {
        let start = assemble_session_start(
            RequirementSnapshot {
                id: "requirement-1".into(),
                revision: 4,
                title: "Login".into(),
                description: "Clarify login".into(),
                summary: "Email code".into(),
                acceptance_criteria: vec!["Expires".into()],
                assumptions: vec!["One account".into()],
                open_questions: vec!["Provider?".into()],
            },
            vec![ConversationMessageSnapshot {
                message_id: "message-1".into(),
                role: ConversationRole::Requester,
                content: "Need clarification".into(),
            }],
            vec![
                RepositorySnapshot {
                    repository_id: "enabled".into(),
                    name: "Enabled".into(),
                    url: "https://example.test/enabled".into(),
                    description: "Configured".into(),
                    enabled: true,
                },
                RepositorySnapshot {
                    repository_id: "disabled".into(),
                    name: "Disabled".into(),
                    url: "https://example.test/disabled".into(),
                    description: "Not selected".into(),
                    enabled: false,
                },
            ],
        )
        .expect("valid assembled context");

        assert_eq!(start.requirement.id, "requirement-1");
        assert_eq!(start.conversation.excerpt.len(), 1);
        assert_eq!(start.repositories.len(), 1);
        assert_eq!(start.repositories[0].repository_id, "enabled");
    }

    #[test]
    fn retains_start_and_drops_oldest_non_start_message() {
        let messages = (0..=51)
            .map(|index| ConversationMessageSnapshot {
                message_id: format!("message-{index}"),
                role: ConversationRole::Requester,
                content: format!("content-{index}"),
            })
            .collect::<Vec<_>>();
        let excerpt = select_conversation_excerpt(&messages, "message-0").expect("excerpt");

        assert_eq!(excerpt.len(), MAX_CONTEXT_MESSAGES);
        assert_eq!(
            excerpt.first().map(|message| message.message_id.as_str()),
            Some("message-0")
        );
        assert_eq!(
            excerpt.last().map(|message| message.message_id.as_str()),
            Some("message-51")
        );
        assert!(!excerpt
            .iter()
            .any(|message| message.message_id == "message-2"));
        assert!(excerpt
            .iter()
            .any(|message| message.message_id == "message-3"));
    }

    #[test]
    fn drops_oversized_newest_message_without_replacing_it_with_old_context() {
        let messages = vec![
            ConversationMessageSnapshot {
                message_id: "start".into(),
                role: ConversationRole::Requester,
                content: "start".into(),
            },
            ConversationMessageSnapshot {
                message_id: "old".into(),
                role: ConversationRole::Requester,
                content: "old".into(),
            },
            ConversationMessageSnapshot {
                message_id: "newest".into(),
                role: ConversationRole::Requester,
                content: "x".repeat(MAX_CONTEXT_BYTES + 1),
            },
        ];

        let excerpt = select_conversation_excerpt(&messages, "start").expect("excerpt");
        assert_eq!(
            excerpt
                .iter()
                .map(|message| message.message_id.as_str())
                .collect::<Vec<_>>(),
            vec!["start"]
        );
    }
}
