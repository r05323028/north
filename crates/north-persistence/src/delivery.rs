use crate::{AuthStore, PersistenceError};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReceiptOutcome {
    Accepted,
    Rejected,
}

impl EventReceiptOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventReceipt {
    pub event_id: String,
    pub session_id: String,
    pub daemon_event_seq: u64,
    pub outcome: EventReceiptOutcome,
    pub rejection_reason: Option<String>,
    pub duplicate: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EventReceiptRequest<'a> {
    pub event_id: &'a str,
    pub session_id: &'a str,
    pub daemon_event_seq: u64,
    pub payload_digest: &'a str,
    pub payload: &'a str,
    pub outcome: EventReceiptOutcome,
    pub rejection_reason: Option<&'a str>,
}

#[derive(Debug, FromRow)]
struct EventReceiptRow {
    event_id: String,
    session_id: String,
    daemon_event_seq: i64,
    payload_digest: String,
    outcome: String,
    rejection_reason: Option<String>,
}

#[derive(Debug, FromRow)]
struct SessionDeliveryRow {
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
}

impl AuthStore {
    /// Commit a daemon command ACK and advance only the contiguous watermark.
    pub async fn acknowledge_command(
        &self,
        command_id: &str,
        session_id: &str,
        server_command_seq: u64,
    ) -> Result<u64, PersistenceError> {
        if command_id.trim().is_empty() || session_id.trim().is_empty() || server_command_seq == 0 {
            return Err(PersistenceError::ProtocolIntegrity(
                "invalid command ACK identity".into(),
            ));
        }
        let sequence =
            i64::try_from(server_command_seq).map_err(|_| PersistenceError::InvalidSessionState)?;
        let mut transaction = self.pool.begin().await?;
        let outbox = sqlx::query_as::<_, CommandReceiptRow>(
            "SELECT session_id, server_command_seq, payload, payload_digest,
                    command_identity_digest, acknowledged_at IS NOT NULL AS acknowledged
             FROM server_command_outbox
             WHERE command_id = $1
             FOR UPDATE",
        )
        .bind(command_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let watermark = if let Some(row) = outbox {
            if row.session_id != session_id || row.server_command_seq != sequence {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command ACK identity conflicts with outbox".into(),
                ));
            }
            if crate::payload_digest(&row.payload) != row.payload_digest
                || !crate::command_identity_digest_valid(&row.payload, &row.command_identity_digest)
            {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command outbox payload digest mismatch".into(),
                ));
            }
            if !row.acknowledged {
                sqlx::query(
                    "UPDATE server_command_outbox
                     SET acknowledged_at = CURRENT_TIMESTAMP
                     WHERE command_id = $1",
                )
                .bind(command_id)
                .execute(&mut *transaction)
                .await?;
            }
            advance_command_watermark(&mut transaction, session_id).await?
        } else {
            let Some(tombstone) = sqlx::query_as::<_, CommandTombstoneReceiptRow>(
                "SELECT session_id, server_command_seq
                 FROM server_command_tombstones
                 WHERE command_id = $1
                 FOR UPDATE",
            )
            .bind(command_id)
            .fetch_optional(&mut *transaction)
            .await?
            else {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command ACK references unknown command".into(),
                ));
            };
            if tombstone.session_id != session_id || tombstone.server_command_seq != sequence {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command ACK identity conflicts with tombstone".into(),
                ));
            }
            let watermark: i64 = sqlx::query_scalar(
                "SELECT command_ack_through_seq
                 FROM execution_sessions WHERE id = $1 FOR UPDATE",
            )
            .bind(session_id)
            .fetch_one(&mut *transaction)
            .await?;
            u64::try_from(watermark).map_err(|_| PersistenceError::InvalidSessionState)?
        };
        compact_server_commands_in_transaction(&mut transaction, session_id).await?;
        transaction.commit().await?;
        Ok(watermark)
    }

    /// Return unacknowledged immutable outbox payloads in per-session order.
    pub async fn unacknowledged_commands(
        &self,
        daemon_id: &str,
    ) -> Result<Vec<crate::PinnedCommand>, PersistenceError> {
        let rows = sqlx::query_as::<_, PinnedCommandRow>(
            "SELECT command_id, session_id, daemon_id, server_command_seq, payload,
                    payload_digest, command_identity_digest
             FROM server_command_outbox
             WHERE daemon_id = $1
               AND acknowledged_at IS NULL
               AND payload IS NOT NULL
             ORDER BY session_id ASC, server_command_seq ASC",
        )
        .bind(daemon_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn compact_server_commands(&self, session_id: &str) -> Result<u64, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let deleted = compact_server_commands_in_transaction(&mut transaction, session_id).await?;
        transaction.commit().await?;
        Ok(deleted)
    }

    /// Record a non-assessment daemon event after its server handling decision.
    /// The caller persists either a real effect outcome or a durable rejection;
    /// assessment events use the readiness transaction and the same watermark.
    pub async fn record_event_receipt(
        &self,
        event_id: &str,
        session_id: &str,
        daemon_event_seq: u64,
        payload_digest: &str,
        outcome: EventReceiptOutcome,
        rejection_reason: Option<&str>,
    ) -> Result<EventReceipt, PersistenceError> {
        self.record_event_receipt_with_payload(EventReceiptRequest {
            event_id,
            session_id,
            daemon_event_seq,
            payload_digest,
            payload: payload_digest,
            outcome,
            rejection_reason,
        })
        .await
    }

    pub async fn record_event_receipt_with_payload(
        &self,
        request: EventReceiptRequest<'_>,
    ) -> Result<EventReceipt, PersistenceError> {
        let EventReceiptRequest {
            event_id,
            session_id,
            daemon_event_seq,
            payload_digest,
            payload,
            outcome,
            rejection_reason,
        } = request;
        if event_id.trim().is_empty()
            || session_id.trim().is_empty()
            || payload_digest.trim().is_empty()
            || payload.trim().is_empty()
            || daemon_event_seq == 0
        {
            return Err(PersistenceError::ProtocolIntegrity(
                "invalid event identity".into(),
            ));
        }
        if !payload_digest_matches(payload, payload_digest) {
            return Err(PersistenceError::ProtocolIntegrity(
                "event payload digest mismatch".into(),
            ));
        }
        if outcome == EventReceiptOutcome::Accepted && rejection_reason.is_some() {
            return Err(PersistenceError::ProtocolIntegrity(
                "accepted event receipt cannot have a rejection reason".into(),
            ));
        }
        if outcome == EventReceiptOutcome::Rejected && rejection_reason.is_none_or(str::is_empty) {
            return Err(PersistenceError::ProtocolIntegrity(
                "rejected event receipt needs a reason".into(),
            ));
        }
        let sequence =
            i64::try_from(daemon_event_seq).map_err(|_| PersistenceError::InvalidSessionState)?;
        let mut transaction = self.pool.begin().await?;
        let session = sqlx::query_as::<_, SessionDeliveryRow>(
            "SELECT event_ack_through_seq, event_ack_sparse
             FROM execution_sessions
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PersistenceError::InvalidSessionState)?;
        let readiness_event_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM readiness_assessments WHERE event_id = $1)",
        )
        .bind(event_id)
        .fetch_one(&mut *transaction)
        .await?;
        if readiness_event_exists {
            return Err(PersistenceError::ProtocolIntegrity(
                "event ID belongs to readiness event ledger".into(),
            ));
        }

        if let Some(row) = sqlx::query_as::<_, EventReceiptRow>(
            "SELECT event_id, session_id, daemon_event_seq, payload_digest,
                    outcome, rejection_reason
             FROM server_event_dedupe
             WHERE event_id = $1
             FOR UPDATE",
        )
        .bind(event_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            if row.session_id != session_id
                || row.daemon_event_seq != sequence
                || row.payload_digest != payload_digest
            {
                return Err(PersistenceError::ProtocolIntegrity(
                    "event identity or payload conflicts with retained event".into(),
                ));
            }
            transaction.commit().await?;
            return event_receipt_from_row(row, true);
        }

        if let Some(row) = sqlx::query_as::<_, EventReceiptRow>(
            "SELECT event_id, session_id, daemon_event_seq, payload_digest,
                    outcome, rejection_reason
             FROM server_event_dedupe
             WHERE session_id = $1 AND daemon_event_seq = $2
             FOR UPDATE",
        )
        .bind(session_id)
        .bind(sequence)
        .fetch_optional(&mut *transaction)
        .await?
        {
            return Err(PersistenceError::ProtocolIntegrity(format!(
                "event sequence {sequence} already belongs to {}",
                row.event_id
            )));
        }

        let mut expected = session
            .event_ack_through_seq
            .checked_add(1)
            .ok_or(PersistenceError::InvalidSessionState)?;
        while session.event_ack_sparse.contains(&expected) {
            expected = expected
                .checked_add(1)
                .ok_or(PersistenceError::InvalidSessionState)?;
        }
        if sequence > expected {
            return Err(PersistenceError::EventSequenceGap {
                expected: u64::try_from(expected).unwrap_or(u64::MAX),
                received: daemon_event_seq,
            });
        }
        if sequence <= session.event_ack_through_seq {
            return Err(PersistenceError::ProtocolIntegrity(
                "event watermark has no retained matching identity".into(),
            ));
        }

        sqlx::query(
            "INSERT INTO server_event_dedupe
                (event_id, session_id, daemon_event_seq, payload_digest, payload,
                 outcome, rejection_reason)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(event_id)
        .bind(session_id)
        .bind(sequence)
        .bind(payload_digest)
        .bind(payload)
        .bind(outcome.as_str())
        .bind(rejection_reason)
        .execute(&mut *transaction)
        .await?;
        advance_event_watermark(&mut transaction, session_id, sequence).await?;
        transaction.commit().await?;
        Ok(EventReceipt {
            event_id: event_id.into(),
            session_id: session_id.into(),
            daemon_event_seq,
            outcome,
            rejection_reason: rejection_reason.map(str::to_owned),
            duplicate: false,
        })
    }
}

async fn compact_server_commands_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<u64, PersistenceError> {
    let watermark: i64 = sqlx::query_scalar(
        "SELECT command_ack_through_seq
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO server_command_tombstones
            (command_id, session_id, daemon_id, server_command_seq, payload,
             payload_digest, command_identity_digest, acknowledged_at)
         SELECT command_id, session_id, daemon_id, server_command_seq,
                CASE WHEN command_identity_digest = payload_digest THEN payload ELSE NULL END,
                payload_digest, command_identity_digest, acknowledged_at
         FROM server_command_outbox
         WHERE session_id = $1
           AND server_command_seq <= $2
           AND acknowledged_at IS NOT NULL
         ON CONFLICT DO NOTHING",
    )
    .bind(session_id)
    .bind(watermark)
    .execute(&mut **transaction)
    .await?;
    let deleted = sqlx::query(
        "DELETE FROM server_command_outbox
         WHERE session_id = $1
           AND server_command_seq <= $2
           AND acknowledged_at IS NOT NULL",
    )
    .bind(session_id)
    .bind(watermark)
    .execute(&mut **transaction)
    .await?;
    Ok(deleted.rows_affected())
}

async fn advance_event_watermark(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    sequence: i64,
) -> Result<(), PersistenceError> {
    let mut cursor = sqlx::query_as::<_, EventCursorRow>(
        "SELECT event_ack_through_seq, event_ack_sparse
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    cursor.event_ack_through_seq = cursor.event_ack_through_seq.max(sequence);
    loop {
        let next = cursor.event_ack_through_seq.saturating_add(1);
        let Some(position) = cursor
            .event_ack_sparse
            .iter()
            .position(|value| *value == next)
        else {
            break;
        };
        cursor.event_ack_sparse.remove(position);
        cursor.event_ack_through_seq = next;
    }
    cursor
        .event_ack_sparse
        .retain(|value| *value > cursor.event_ack_through_seq);
    sqlx::query(
        "UPDATE execution_sessions
         SET event_ack_through_seq = $2, event_ack_sparse = $3
         WHERE id = $1",
    )
    .bind(session_id)
    .bind(cursor.event_ack_through_seq)
    .bind(cursor.event_ack_sparse)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[derive(Debug, FromRow)]
struct EventCursorRow {
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
}

async fn advance_command_watermark(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> Result<u64, PersistenceError> {
    let current: i64 = sqlx::query_scalar(
        "SELECT command_ack_through_seq
         FROM execution_sessions
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    let first_unacknowledged: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(server_command_seq)
         FROM server_command_outbox
         WHERE session_id = $1
           AND server_command_seq > $2
           AND acknowledged_at IS NULL",
    )
    .bind(session_id)
    .bind(current)
    .fetch_one(&mut **transaction)
    .await?;
    let maximum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(server_command_seq), 0)
         FROM server_command_outbox WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?;
    let watermark = first_unacknowledged
        .map(|sequence| sequence.saturating_sub(1))
        .unwrap_or(maximum)
        .max(current);
    sqlx::query("UPDATE execution_sessions SET command_ack_through_seq = $2 WHERE id = $1")
        .bind(session_id)
        .bind(watermark)
        .execute(&mut **transaction)
        .await?;
    u64::try_from(watermark).map_err(|_| PersistenceError::InvalidSessionState)
}

fn payload_digest_matches(payload: &str, expected: &str) -> bool {
    serde_json::from_str::<Value>(payload)
        .map(|value| canonical_payload_digest(&value) == expected)
        .unwrap_or_else(|_| payload == expected)
}

fn event_receipt_from_row(
    row: EventReceiptRow,
    duplicate: bool,
) -> Result<EventReceipt, PersistenceError> {
    let outcome = match row.outcome.as_str() {
        "accepted" => EventReceiptOutcome::Accepted,
        "rejected" => EventReceiptOutcome::Rejected,
        _value => return Err(PersistenceError::InvalidSessionState),
    };
    Ok(EventReceipt {
        event_id: row.event_id,
        session_id: row.session_id,
        daemon_event_seq: u64::try_from(row.daemon_event_seq)
            .map_err(|_| PersistenceError::InvalidSessionState)?,
        outcome,
        rejection_reason: row.rejection_reason,
        duplicate,
    })
}

#[derive(Debug, FromRow)]
struct CommandTombstoneReceiptRow {
    session_id: String,
    server_command_seq: i64,
}

#[derive(Debug, FromRow)]
struct CommandReceiptRow {
    session_id: String,
    server_command_seq: i64,
    payload: String,
    payload_digest: String,
    command_identity_digest: String,
    acknowledged: bool,
}

#[derive(Debug, FromRow)]
struct PinnedCommandRow {
    command_id: String,
    session_id: String,
    daemon_id: String,
    server_command_seq: i64,
    payload: String,
    payload_digest: String,
    command_identity_digest: String,
}

impl TryFrom<PinnedCommandRow> for crate::PinnedCommand {
    type Error = PersistenceError;

    fn try_from(row: PinnedCommandRow) -> Result<Self, Self::Error> {
        if crate::payload_digest(&row.payload) != row.payload_digest
            || !crate::command_identity_digest_valid(&row.payload, &row.command_identity_digest)
        {
            return Err(PersistenceError::ProtocolIntegrity(
                "command outbox payload digest mismatch".into(),
            ));
        }
        Ok(crate::PinnedCommand {
            command_id: row.command_id,
            session_id: row.session_id,
            daemon_id: row.daemon_id,
            server_command_seq: u64::try_from(row.server_command_seq)
                .map_err(|_| PersistenceError::InvalidSessionState)?,
            payload: row.payload,
            payload_digest: row.payload_digest,
            command_identity_digest: row.command_identity_digest,
            compacted: false,
        })
    }
}

/// Stable JSON digest input helper used by the server coordinator. Persistence
/// stores the opaque value and never interprets wire payloads.
pub fn canonical_payload_digest(payload: &Value) -> String {
    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

use sha2::Digest;
