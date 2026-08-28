use crate::{hash_secret, random_hex, AuthStore, PersistenceError};
use sqlx::FromRow;
use subtle::ConstantTimeEq;

pub const DAEMON_SETUP_TTL_SECONDS: i64 = 10 * 60;
pub const DAEMON_SETUP_RETENTION_SECONDS: i64 = 24 * 60 * 60;
pub const DAEMON_SETUP_CLEANUP_BATCH_SIZE: i64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSetupRequest {
    pub request_token: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonSetupClaim {
    Pending,
    Claimed {
        daemon_id: String,
        credential: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonSetupState {
    Pending,
    Approved,
    Claimed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSetupPreview {
    pub label: String,
    pub state: DaemonSetupState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonRegistration {
    pub daemon_id: String,
    pub label: String,
    pub created_by: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub connected: bool,
    pub protocol_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedDaemon {
    pub daemon_id: String,
    pub connection_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSessionState {
    pub session_id: String,
    pub command_ack_through_seq: u64,
    pub event_ack_through_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedCommand {
    pub command_id: String,
    pub session_id: String,
    pub daemon_id: String,
    pub server_command_seq: u64,
    pub payload: String,
}

impl AuthStore {
    /// Remove only old expired setup rows, keeping recent rows for diagnostics.
    pub async fn cleanup_expired_daemon_setup_requests(&self) -> Result<u64, PersistenceError> {
        let deleted = sqlx::query(
            "WITH expired AS (
                 SELECT id
                 FROM daemon_setup_requests
                 WHERE expires_at < CURRENT_TIMESTAMP
                     - ($1::double precision * INTERVAL '1 second')
                 ORDER BY expires_at ASC
                 LIMIT $2
             )
             DELETE FROM daemon_setup_requests AS requests
             USING expired
             WHERE requests.id = expired.id",
        )
        .bind(DAEMON_SETUP_RETENTION_SECONDS)
        .bind(DAEMON_SETUP_CLEANUP_BATCH_SIZE)
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    pub async fn create_daemon_setup_request(
        &self,
        label: &str,
    ) -> Result<DaemonSetupRequest, PersistenceError> {
        self.cleanup_expired_daemon_setup_requests().await?;
        let request_token = random_hex(32);
        sqlx::query(
            "INSERT INTO daemon_setup_requests
                (id, request_token_hash, label, expires_at)
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP
                 + ($4::double precision * INTERVAL '1 second'))",
        )
        .bind(random_hex(16))
        .bind(hash_secret(request_token.as_bytes()))
        .bind(label)
        .bind(DAEMON_SETUP_TTL_SECONDS)
        .execute(&self.pool)
        .await?;
        Ok(DaemonSetupRequest {
            request_token,
            label: label.to_owned(),
        })
    }

    pub async fn preview_daemon_setup_request(
        &self,
        request_token: &str,
    ) -> Result<DaemonSetupPreview, PersistenceError> {
        let Some(request) = sqlx::query_as::<_, SetupPreviewRow>(
            "SELECT label, expires_at <= CURRENT_TIMESTAMP AS expired,
                    approved_at IS NOT NULL AS approved,
                    claimed_at IS NOT NULL AS claimed
             FROM daemon_setup_requests
             WHERE request_token_hash = $1",
        )
        .bind(hash_secret(request_token.as_bytes()))
        .fetch_optional(&self.pool)
        .await?
        else {
            return Err(PersistenceError::SetupNotFound);
        };
        if request.expired {
            return Err(PersistenceError::SetupExpired);
        }
        let state = if request.claimed {
            DaemonSetupState::Claimed
        } else if request.approved {
            DaemonSetupState::Approved
        } else {
            DaemonSetupState::Pending
        };
        Ok(DaemonSetupPreview {
            label: request.label,
            state,
        })
    }

    pub async fn approve_daemon_setup_request(
        &self,
        request_token: &str,
        user_id: &str,
    ) -> Result<(), PersistenceError> {
        let token_hash = hash_secret(request_token.as_bytes());
        let updated = sqlx::query(
            "UPDATE daemon_setup_requests
             SET created_by = $2, approved_at = CURRENT_TIMESTAMP
             WHERE request_token_hash = $1
               AND expires_at > CURRENT_TIMESTAMP
               AND approved_at IS NULL
               AND claimed_at IS NULL",
        )
        .bind(&token_hash)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 1 {
            return Ok(());
        }

        let state = sqlx::query_as::<_, SetupStateRow>(
            "SELECT expires_at <= CURRENT_TIMESTAMP AS expired,
                    approved_at IS NOT NULL AS approved,
                    claimed_at IS NOT NULL AS claimed
             FROM daemon_setup_requests
             WHERE request_token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        match state {
            None => Err(PersistenceError::SetupNotFound),
            Some(state) if state.claimed => Err(PersistenceError::SetupAlreadyClaimed),
            Some(state) if state.approved => Err(PersistenceError::SetupAlreadyApproved),
            Some(state) if state.expired => Err(PersistenceError::SetupExpired),
            Some(_) => Err(PersistenceError::SetupNotFound),
        }
    }

    pub async fn claim_daemon_setup_request(
        &self,
        request_token: &str,
    ) -> Result<DaemonSetupClaim, PersistenceError> {
        self.cleanup_expired_daemon_setup_requests().await?;
        let mut transaction = self.pool.begin().await?;
        let Some(request) = sqlx::query_as::<_, SetupRequestRow>(
            "SELECT label, created_by, approved_at IS NOT NULL AS approved,
                    claimed_at IS NOT NULL AS claimed,
                    expires_at <= CURRENT_TIMESTAMP AS expired
             FROM daemon_setup_requests
             WHERE request_token_hash = $1
             FOR UPDATE",
        )
        .bind(hash_secret(request_token.as_bytes()))
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Err(PersistenceError::SetupNotFound);
        };

        if request.claimed {
            return Err(PersistenceError::SetupAlreadyClaimed);
        }
        if request.expired {
            return Err(PersistenceError::SetupExpired);
        }
        if !request.approved {
            transaction.commit().await?;
            return Ok(DaemonSetupClaim::Pending);
        }
        let Some(created_by) = request.created_by else {
            return Err(PersistenceError::InvalidSetup);
        };

        let daemon_id = random_hex(16);
        let credential = random_hex(32);
        sqlx::query(
            "INSERT INTO daemon_registrations
                (daemon_id, credential_hash, label, created_by, protocol_version)
             VALUES ($1, $2, $3, $4, '0.1')",
        )
        .bind(&daemon_id)
        .bind(hash_secret(credential.as_bytes()))
        .bind(&request.label)
        .bind(created_by)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE daemon_setup_requests
             SET claimed_at = CURRENT_TIMESTAMP, daemon_id = $2
             WHERE request_token_hash = $1",
        )
        .bind(hash_secret(request_token.as_bytes()))
        .bind(&daemon_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(DaemonSetupClaim::Claimed {
            daemon_id,
            credential,
        })
    }

    pub async fn list_daemons(&self) -> Result<Vec<DaemonRegistration>, PersistenceError> {
        let rows = sqlx::query_as::<_, DaemonRegistrationRow>(
            "SELECT daemon_id, label, created_by, created_at::text AS created_at,
                    revoked_at::text AS revoked_at,
                    last_seen_at::text AS last_seen_at,
                    (connected_at IS NOT NULL
                     AND revoked_at IS NULL
                     AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds') AS connected,
                    protocol_version, capabilities
             FROM daemon_registrations
             ORDER BY label ASC, daemon_id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(DaemonRegistrationRow::into_domain)
            .collect()
    }

    pub async fn daemon_by_id(
        &self,
        daemon_id: &str,
    ) -> Result<Option<DaemonRegistration>, PersistenceError> {
        let row = sqlx::query_as::<_, DaemonRegistrationRow>(
            "SELECT daemon_id, label, created_by, created_at::text AS created_at,
                    revoked_at::text AS revoked_at,
                    last_seen_at::text AS last_seen_at,
                    (connected_at IS NOT NULL
                     AND revoked_at IS NULL
                     AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds') AS connected,
                    protocol_version, capabilities
             FROM daemon_registrations
             WHERE daemon_id = $1",
        )
        .bind(daemon_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(DaemonRegistrationRow::into_domain).transpose()
    }

    /// Invalidate connection leases left by a prior single-server process.
    pub async fn invalidate_daemon_connections(&self) -> Result<u64, PersistenceError> {
        let updated = sqlx::query(
            "UPDATE daemon_registrations
             SET connected_at = NULL, connection_id = NULL
             WHERE connected_at IS NOT NULL OR connection_id IS NOT NULL",
        )
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected())
    }

    pub async fn connect_daemon(
        &self,
        daemon_id: &str,
        credential: &str,
        protocol_version: &str,
        capabilities: &[String],
    ) -> Result<AuthenticatedDaemon, PersistenceError> {
        let capabilities = encode_capabilities(capabilities)?;
        let mut transaction = self.pool.begin().await?;
        let Some(row) = sqlx::query_as::<_, DaemonCredentialRow>(
            "SELECT credential_hash, revoked_at::text AS revoked_at
             FROM daemon_registrations
             WHERE daemon_id = $1
             FOR UPDATE",
        )
        .bind(daemon_id)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Err(PersistenceError::InvalidDaemonCredential);
        };
        if row.revoked_at.is_some() {
            return Err(PersistenceError::RevokedDaemon);
        }
        let candidate = hash_secret(credential.as_bytes());
        if row
            .credential_hash
            .as_slice()
            .ct_eq(candidate.as_slice())
            .unwrap_u8()
            != 1
        {
            return Err(PersistenceError::InvalidDaemonCredential);
        }

        let connection_id = random_hex(16);
        sqlx::query(
            "UPDATE daemon_registrations
             SET protocol_version = $2,
                 capabilities = $3,
                 connected_at = CURRENT_TIMESTAMP,
                 last_seen_at = CURRENT_TIMESTAMP,
                 connection_id = $4
             WHERE daemon_id = $1 AND revoked_at IS NULL",
        )
        .bind(daemon_id)
        .bind(protocol_version)
        .bind(capabilities)
        .bind(&connection_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(AuthenticatedDaemon {
            daemon_id: daemon_id.to_owned(),
            connection_id,
        })
    }

    pub async fn touch_daemon(
        &self,
        daemon_id: &str,
        connection_id: &str,
    ) -> Result<(), PersistenceError> {
        let updated = sqlx::query(
            "UPDATE daemon_registrations
             SET last_seen_at = CURRENT_TIMESTAMP
             WHERE daemon_id = $1
               AND connection_id = $2
               AND revoked_at IS NULL",
        )
        .bind(daemon_id)
        .bind(connection_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(PersistenceError::InvalidDaemonCredential)
        }
    }

    pub async fn disconnect_daemon(
        &self,
        daemon_id: &str,
        connection_id: &str,
    ) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE daemon_registrations
             SET connected_at = NULL, connection_id = NULL
             WHERE daemon_id = $1 AND connection_id = $2",
        )
        .bind(daemon_id)
        .bind(connection_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn revoke_daemon(&self, daemon_id: &str) -> Result<Option<String>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let Some(row) = sqlx::query_as::<_, ConnectionIdRow>(
            "SELECT connection_id
             FROM daemon_registrations
             WHERE daemon_id = $1
             FOR UPDATE",
        )
        .bind(daemon_id)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Err(PersistenceError::DaemonNotFound);
        };
        sqlx::query(
            "UPDATE daemon_registrations
             SET revoked_at = COALESCE(revoked_at, CURRENT_TIMESTAMP),
                 connected_at = NULL, connection_id = NULL
             WHERE daemon_id = $1",
        )
        .bind(daemon_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(row.connection_id)
    }

    pub async fn reconciliation_for_daemon(
        &self,
        daemon_id: &str,
    ) -> Result<Vec<DaemonSessionState>, PersistenceError> {
        let rows = sqlx::query_as::<_, SessionReconcileRow>(
            "SELECT execution_sessions.id AS session_id,
                    COALESCE(
                        MAX(server_command_outbox.server_command_seq)
                            FILTER (WHERE server_command_outbox.acknowledged_at IS NOT NULL),
                        0
                    ) AS command_ack_through_seq
             FROM execution_sessions
             LEFT JOIN server_command_outbox
               ON server_command_outbox.session_id = execution_sessions.id
             WHERE execution_sessions.daemon_id = $1
             GROUP BY execution_sessions.id
             ORDER BY execution_sessions.id ASC",
        )
        .bind(daemon_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DaemonSessionState {
                    session_id: row.session_id,
                    command_ack_through_seq: u64::try_from(row.command_ack_through_seq)
                        .map_err(|_| PersistenceError::InvalidSessionState)?,
                    event_ack_through_seq: 0,
                })
            })
            .collect()
    }

    pub async fn session_owner(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, PersistenceError> {
        let row = sqlx::query_as::<_, SessionOwnerRow>(
            "SELECT daemon_id, requirement_id FROM execution_sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|row| row.daemon_id))
    }

    pub async fn session_requirement(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, PersistenceError> {
        let row = sqlx::query_as::<_, SessionRequirementRow>(
            "SELECT requirement_id FROM execution_sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|row| row.requirement_id))
    }

    pub async fn start_session_with_command<F>(
        &self,
        session_id: &str,
        command_id: &str,
        required_capabilities: &[String],
        build_payload: F,
    ) -> Result<PinnedCommand, PersistenceError>
    where
        F: FnOnce(&str, u64) -> Result<String, PersistenceError> + Send,
    {
        self.start_session_with_command_for_requirement(
            session_id,
            command_id,
            required_capabilities,
            None,
            build_payload,
        )
        .await
    }

    pub async fn start_session_with_command_for_requirement<F>(
        &self,
        session_id: &str,
        command_id: &str,
        required_capabilities: &[String],
        requirement_id: Option<&str>,
        build_payload: F,
    ) -> Result<PinnedCommand, PersistenceError>
    where
        F: FnOnce(&str, u64) -> Result<String, PersistenceError> + Send,
    {
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query_as::<_, SessionOwnerRow>(
            "SELECT daemon_id, requirement_id
             FROM execution_sessions
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *transaction)
        .await?;

        let daemon_id = if let Some(existing) = existing {
            if existing
                .requirement_id
                .as_deref()
                .zip(requirement_id)
                .is_some_and(|(bound, requested)| bound != requested)
            {
                return Err(PersistenceError::SessionRequirementMismatch);
            }
            if let Some(requirement_id) =
                requirement_id.filter(|_| existing.requirement_id.is_none())
            {
                sqlx::query(
                    "UPDATE execution_sessions
                     SET requirement_id = $2
                     WHERE id = $1",
                )
                .bind(session_id)
                .bind(requirement_id)
                .execute(&mut *transaction)
                .await?;
            }
            if let Some(daemon_id) = existing.daemon_id {
                daemon_id
            } else {
                let daemon_id =
                    choose_eligible_daemon(&mut transaction, required_capabilities).await?;
                sqlx::query("UPDATE execution_sessions SET daemon_id = $2 WHERE id = $1")
                    .bind(session_id)
                    .bind(&daemon_id)
                    .execute(&mut *transaction)
                    .await?;
                daemon_id
            }
        } else {
            let daemon_id = choose_eligible_daemon(&mut transaction, required_capabilities).await?;
            sqlx::query(
                "INSERT INTO execution_sessions (id, daemon_id, requirement_id)
                 VALUES ($1, $2, $3)",
            )
            .bind(session_id)
            .bind(&daemon_id)
            .bind(requirement_id)
            .execute(&mut *transaction)
            .await?;
            daemon_id
        };

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(server_command_seq), 0) + 1
             FROM server_command_outbox
             WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&mut *transaction)
        .await?;
        let server_command_seq =
            u64::try_from(next_sequence).map_err(|_| PersistenceError::InvalidSessionState)?;
        let payload = build_payload(&daemon_id, server_command_seq)?;
        sqlx::query(
            "INSERT INTO server_command_outbox
                (command_id, session_id, daemon_id, server_command_seq, payload)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(command_id)
        .bind(session_id)
        .bind(&daemon_id)
        .bind(next_sequence)
        .bind(&payload)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(PinnedCommand {
            command_id: command_id.to_owned(),
            session_id: session_id.to_owned(),
            daemon_id,
            server_command_seq,
            payload,
        })
    }
}

async fn choose_eligible_daemon(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    required_capabilities: &[String],
) -> Result<String, PersistenceError> {
    let candidates = sqlx::query_as::<_, DaemonCandidateRow>(
        "SELECT daemon_id, capabilities
         FROM daemon_registrations
         WHERE revoked_at IS NULL
           AND connected_at IS NOT NULL
           AND last_seen_at > CURRENT_TIMESTAMP - INTERVAL '45 seconds'
           AND protocol_version = '0.1'
         ORDER BY daemon_id ASC
         FOR UPDATE",
    )
    .fetch_all(&mut **transaction)
    .await?;
    candidates
        .into_iter()
        .find_map(|candidate| {
            let capabilities = decode_capabilities(&candidate.capabilities).ok()?;
            required_capabilities
                .iter()
                .all(|required| capabilities.iter().any(|capability| capability == required))
                .then_some(candidate.daemon_id)
        })
        .ok_or(PersistenceError::NoEligibleDaemon)
}

fn encode_capabilities(capabilities: &[String]) -> Result<String, PersistenceError> {
    serde_json::to_string(capabilities).map_err(|_| PersistenceError::InvalidCapabilities)
}

fn decode_capabilities(value: &str) -> Result<Vec<String>, PersistenceError> {
    serde_json::from_str(value).map_err(|_| PersistenceError::InvalidCapabilities)
}

#[derive(Debug, FromRow)]
struct SetupPreviewRow {
    label: String,
    expired: bool,
    approved: bool,
    claimed: bool,
}

#[derive(Debug, FromRow)]
struct SetupStateRow {
    expired: bool,
    approved: bool,
    claimed: bool,
}

#[derive(Debug, FromRow)]
struct SetupRequestRow {
    label: String,
    created_by: Option<String>,
    approved: bool,
    claimed: bool,
    expired: bool,
}

#[derive(Debug, FromRow)]
struct DaemonRegistrationRow {
    daemon_id: String,
    label: String,
    created_by: String,
    created_at: String,
    revoked_at: Option<String>,
    last_seen_at: Option<String>,
    connected: bool,
    protocol_version: String,
    capabilities: String,
}

impl DaemonRegistrationRow {
    fn into_domain(self) -> Result<DaemonRegistration, PersistenceError> {
        Ok(DaemonRegistration {
            daemon_id: self.daemon_id,
            label: self.label,
            created_by: self.created_by,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
            last_seen_at: self.last_seen_at,
            connected: self.connected,
            protocol_version: self.protocol_version,
            capabilities: decode_capabilities(&self.capabilities)?,
        })
    }
}

#[derive(Debug, FromRow)]
struct DaemonCredentialRow {
    credential_hash: Vec<u8>,
    revoked_at: Option<String>,
}

#[derive(Debug, FromRow)]
struct ConnectionIdRow {
    connection_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct SessionReconcileRow {
    session_id: String,
    command_ack_through_seq: i64,
}

#[derive(Debug, FromRow)]
struct SessionOwnerRow {
    daemon_id: Option<String>,
    requirement_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct SessionRequirementRow {
    requirement_id: Option<String>,
}

#[derive(Debug, FromRow)]
struct DaemonCandidateRow {
    daemon_id: String,
    capabilities: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_round_trip_without_runtime_types() -> Result<(), PersistenceError> {
        let capabilities = vec!["agent".into(), "repository:read".into()];
        let encoded = encode_capabilities(&capabilities)?;
        assert_eq!(decode_capabilities(&encoded)?, capabilities);
        Ok(())
    }

    #[test]
    fn malformed_capabilities_are_rejected() {
        assert!(matches!(
            decode_capabilities("not-json"),
            Err(PersistenceError::InvalidCapabilities)
        ));
    }
}
