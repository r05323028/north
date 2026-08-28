use crate::AuthStore;
use sqlx::{FromRow, Postgres, Transaction};
use std::{error::Error, fmt};

/// The single durable conversation attached to one requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationRecord {
    pub id: String,
    pub requirement_id: String,
    pub created_at: String,
}

/// Message kinds allowed in a requirement conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Requester,
    Agent,
    System,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requester => "requester",
            Self::Agent => "agent",
            Self::System => "system",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, ConversationError> {
        match value {
            "requester" => Ok(Self::Requester),
            "agent" => Ok(Self::Agent),
            "system" => Ok(Self::System),
            value => Err(ConversationError::InvalidKind(value.to_owned())),
        }
    }
}

/// Durable conversation message. Runtime telemetry never uses this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub author_user_id: Option<String>,
    pub kind: MessageKind,
    pub body: String,
    pub created_at: String,
}

/// Offset page with deterministic `(created_at, id)` ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationPage {
    pub conversation: ConversationRecord,
    pub messages: Vec<MessageRecord>,
    pub next_offset: Option<u64>,
}

#[derive(Debug)]
pub enum ConversationError {
    Database(sqlx::Error),
    RequirementNotFound,
    InvalidKind(String),
    InvalidMessage,
    InvalidPage,
}

impl fmt::Display for ConversationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::RequirementNotFound => f.write_str("requirement conversation not found"),
            Self::InvalidKind(kind) => write!(f, "invalid conversation message kind: {kind}"),
            Self::InvalidMessage => f.write_str("invalid conversation message"),
            Self::InvalidPage => f.write_str("invalid conversation page"),
        }
    }
}

impl Error for ConversationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ConversationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl AuthStore {
    /// Append a requester-authored message to the requirement's one thread.
    pub async fn post_requester_message(
        &self,
        requirement_id: &str,
        author_user_id: &str,
        body: &str,
    ) -> Result<MessageRecord, ConversationError> {
        if author_user_id.trim().is_empty() || body.trim().is_empty() || body.len() > 100_000 {
            return Err(ConversationError::InvalidMessage);
        }
        let mut transaction = self.pool.begin().await?;
        let conversation = conversation_row(&mut transaction, requirement_id)
            .await?
            .ok_or(ConversationError::RequirementNotFound)?;
        let row = sqlx::query_as::<_, MessageRow>(
            "INSERT INTO messages
                (id, conversation_id, author_user_id, kind, body)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, conversation_id, author_user_id, kind, body,
                       created_at::text AS created_at",
        )
        .bind(crate::random_hex(16))
        .bind(&conversation.id)
        .bind(author_user_id)
        .bind(MessageKind::Requester.as_str())
        .bind(body.trim())
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        row.into_record()
    }

    /// Read one deterministic page without requiring transcript replay.
    pub async fn conversation_page(
        &self,
        requirement_id: &str,
        offset: u64,
        limit: u64,
    ) -> Result<ConversationPage, ConversationError> {
        if limit == 0 || limit > 100 || offset > i64::MAX as u64 {
            return Err(ConversationError::InvalidPage);
        }
        let limit_i64 = i64::try_from(limit).map_err(|_| ConversationError::InvalidPage)?;
        let fetch_limit = limit_i64
            .checked_add(1)
            .ok_or(ConversationError::InvalidPage)?;
        let offset_i64 = i64::try_from(offset).map_err(|_| ConversationError::InvalidPage)?;
        let mut transaction = self.pool.begin().await?;
        let conversation = conversation_row(&mut transaction, requirement_id)
            .await?
            .ok_or(ConversationError::RequirementNotFound)?;
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, conversation_id, author_user_id, kind, body,
                    created_at::text AS created_at
             FROM messages
             WHERE conversation_id = $1
             ORDER BY created_at ASC, id ASC
             LIMIT $2 OFFSET $3",
        )
        .bind(&conversation.id)
        .bind(fetch_limit)
        .bind(offset_i64)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;

        let has_more = rows.len() > limit as usize;
        let messages = rows
            .into_iter()
            .take(limit as usize)
            .map(MessageRow::into_record)
            .collect::<Result<Vec<_>, _>>()?;
        let next_offset = has_more
            .then(|| {
                offset
                    .checked_add(limit)
                    .ok_or(ConversationError::InvalidPage)
            })
            .transpose()?;
        Ok(ConversationPage {
            conversation: conversation.into_record(),
            messages,
            next_offset,
        })
    }
}

async fn conversation_row(
    transaction: &mut Transaction<'_, Postgres>,
    requirement_id: &str,
) -> Result<Option<ConversationRow>, ConversationError> {
    Ok(sqlx::query_as::<_, ConversationRow>(
        "SELECT id, requirement_id, created_at::text AS created_at
         FROM conversations
         WHERE requirement_id = $1",
    )
    .bind(requirement_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

#[derive(Debug, FromRow)]
struct ConversationRow {
    id: String,
    requirement_id: String,
    created_at: String,
}

impl ConversationRow {
    fn into_record(self) -> ConversationRecord {
        ConversationRecord {
            id: self.id,
            requirement_id: self.requirement_id,
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, FromRow)]
struct MessageRow {
    id: String,
    conversation_id: String,
    author_user_id: Option<String>,
    kind: String,
    body: String,
    created_at: String,
}

impl MessageRow {
    fn into_record(self) -> Result<MessageRecord, ConversationError> {
        Ok(MessageRecord {
            id: self.id,
            conversation_id: self.conversation_id,
            author_user_id: self.author_user_id,
            kind: MessageKind::from_persisted(&self.kind)?,
            body: self.body,
            created_at: self.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_kinds_are_closed_and_stable() {
        assert_eq!(MessageKind::Requester.as_str(), "requester");
        assert_eq!(MessageKind::Agent.as_str(), "agent");
        assert!(MessageKind::from_persisted("tool-output").is_err());
    }
}
