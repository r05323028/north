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
    pub event_ack_sparse: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedCommand {
    pub command_id: String,
    pub session_id: String,
    pub daemon_id: String,
    pub server_command_seq: u64,
    pub payload: String,
    pub payload_digest: String,
    pub command_identity_digest: String,
    pub compacted: bool,
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
                    execution_sessions.command_ack_through_seq,
                    execution_sessions.event_ack_through_seq,
                    execution_sessions.event_ack_sparse
             FROM execution_sessions
             WHERE execution_sessions.daemon_id = $1
             ORDER BY execution_sessions.id ASC",
        )
        .bind(daemon_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let event_ack_sparse = row
                    .event_ack_sparse
                    .into_iter()
                    .map(|sequence| {
                        u64::try_from(sequence).map_err(|_| PersistenceError::InvalidSessionState)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(DaemonSessionState {
                    session_id: row.session_id,
                    command_ack_through_seq: u64::try_from(row.command_ack_through_seq)
                        .map_err(|_| PersistenceError::InvalidSessionState)?,
                    event_ack_through_seq: u64::try_from(row.event_ack_through_seq)
                        .map_err(|_| PersistenceError::InvalidSessionState)?,
                    event_ack_sparse,
                })
            })
            .collect()
    }

    pub async fn command_by_id(
        &self,
        command_id: &str,
    ) -> Result<Option<PinnedCommand>, PersistenceError> {
        let outbox = sqlx::query_as::<_, ExistingCommandRow>(
            "SELECT command_id, session_id, daemon_id, server_command_seq, payload,
                    payload_digest, command_identity_digest, FALSE AS compacted
             FROM server_command_outbox
             WHERE command_id = $1",
        )
        .bind(command_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(command) = outbox {
            return command.into_pinned_command().map(Some);
        }
        sqlx::query_as::<_, ExistingCommandRow>(
            "SELECT command_id, session_id, daemon_id, server_command_seq,
                    payload, payload_digest, command_identity_digest,
                    TRUE AS compacted
             FROM server_command_tombstones
             WHERE command_id = $1",
        )
        .bind(command_id)
        .fetch_optional(&self.pool)
        .await?
        .map(ExistingCommandRow::into_pinned_command)
        .transpose()
    }

    pub async fn message_command_matches(
        &self,
        session_id: &str,
        command_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<bool, PersistenceError> {
        let mapping: Option<(String, String)> = sqlx::query_as(
            "SELECT command_id, content_digest
             FROM server_message_command_map
             WHERE session_id = $1 AND message_id = $2",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(mapping.is_some_and(|(mapped_command, digest)| {
            mapped_command == command_id && digest == crate::payload_digest(content)
        }))
    }

    pub async fn session_owner(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, PersistenceError> {
        let row = sqlx::query_as::<_, SessionOwnerRow>(
            "SELECT daemon_id, requirement_id, repository_ids,
                    repository_context_initialized
             FROM execution_sessions WHERE id = $1",
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
        self.start_session_with_command_for_requirement_and_repositories(
            session_id,
            command_id,
            required_capabilities,
            None,
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
        self.start_session_with_command_for_requirement_and_repositories(
            session_id,
            command_id,
            required_capabilities,
            requirement_id,
            None,
            build_payload,
        )
        .await
    }

    pub async fn start_session_with_command_for_requirement_and_repositories<F>(
        &self,
        session_id: &str,
        command_id: &str,
        required_capabilities: &[String],
        requirement_id: Option<&str>,
        repository_ids: Option<&[String]>,
        build_payload: F,
    ) -> Result<PinnedCommand, PersistenceError>
    where
        F: FnOnce(&str, u64) -> Result<String, PersistenceError> + Send,
    {
        let repository_context_initialized = repository_ids.is_some();
        let repository_ids = repository_ids.unwrap_or(&[]);
        let mut transaction = self.pool.begin().await?;
        let mut build_payload = Some(build_payload);
        let existing_command = if let Some(row) = sqlx::query_as::<_, ExistingCommandRow>(
            "SELECT command_id, session_id, daemon_id, server_command_seq, payload, payload_digest,
                    command_identity_digest, FALSE AS compacted
             FROM server_command_outbox
             WHERE command_id = $1
             FOR UPDATE",
        )
        .bind(command_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            Some(row)
        } else {
            sqlx::query_as::<_, ExistingCommandRow>(
                "SELECT command_id, session_id, daemon_id, server_command_seq,
                        payload, payload_digest, command_identity_digest,
                        TRUE AS compacted
                 FROM server_command_tombstones
                 WHERE command_id = $1
                 FOR UPDATE",
            )
            .bind(command_id)
            .fetch_optional(&mut *transaction)
            .await?
        };
        if let Some(existing_command) = existing_command {
            if existing_command.session_id != session_id {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command ID is already bound to another session".into(),
                ));
            }
            let builder = build_payload
                .take()
                .ok_or(PersistenceError::InvalidCommandPayload)?;
            let candidate = builder(
                &existing_command.daemon_id,
                u64::try_from(existing_command.server_command_seq)
                    .map_err(|_| PersistenceError::InvalidSessionState)?,
            )?;
            let identity_matches = match existing_command.payload.as_deref() {
                Some(existing_payload) => crate::command_identity_matches(
                    existing_payload,
                    &candidate,
                    &existing_command.command_identity_digest,
                ),
                None => {
                    crate::command_identity_digest(&candidate)
                        == existing_command.command_identity_digest
                }
            };
            if !identity_matches {
                return Err(PersistenceError::ProtocolIntegrity(
                    "command ID is already bound to a different payload".into(),
                ));
            }
            transaction.commit().await?;
            return existing_command.into_pinned_command();
        }
        let existing = sqlx::query_as::<_, SessionOwnerRow>(
            "SELECT daemon_id, requirement_id, repository_ids,
                    repository_context_initialized
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
            if repository_context_initialized && !existing.repository_context_initialized {
                validate_active_repository_ids(&mut transaction, repository_ids).await?;
                sqlx::query(
                    "UPDATE execution_sessions
                     SET repository_ids = $2, repository_context_initialized = TRUE
                     WHERE id = $1",
                )
                .bind(session_id)
                .bind(repository_ids)
                .execute(&mut *transaction)
                .await?;
            } else if repository_context_initialized
                && existing.repository_context_initialized
                && existing.repository_ids != repository_ids
            {
                return Err(PersistenceError::ProtocolIntegrity(
                    "session repository context cannot be retargeted".into(),
                ));
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
            validate_active_repository_ids(&mut transaction, repository_ids).await?;
            let daemon_id = choose_eligible_daemon(&mut transaction, required_capabilities).await?;
            sqlx::query(
                "INSERT INTO execution_sessions
                    (id, daemon_id, requirement_id, repository_ids,
                     repository_context_initialized)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(session_id)
            .bind(&daemon_id)
            .bind(requirement_id)
            .bind(repository_ids)
            .bind(repository_context_initialized)
            .execute(&mut *transaction)
            .await?;
            daemon_id
        };

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT GREATEST(
                    COALESCE((SELECT MAX(server_command_seq)
                             FROM server_command_outbox WHERE session_id = $1), 0),
                    COALESCE((SELECT MAX(server_command_seq)
                             FROM server_command_tombstones WHERE session_id = $1), 0),
                    (SELECT command_ack_through_seq FROM execution_sessions WHERE id = $1)
                 ) + 1",
        )
        .bind(session_id)
        .fetch_one(&mut *transaction)
        .await?;
        let server_command_seq =
            u64::try_from(next_sequence).map_err(|_| PersistenceError::InvalidSessionState)?;
        let builder = build_payload
            .take()
            .ok_or(PersistenceError::InvalidCommandPayload)?;
        let payload = builder(&daemon_id, server_command_seq)?;
        let payload_digest = crate::payload_digest(&payload);
        let command_identity_digest = crate::command_identity_digest(&payload);
        if let Some((message_id, content_digest)) = message_identity(&payload) {
            let inserted = sqlx::query(
                "INSERT INTO server_message_command_map
                    (session_id, message_id, command_id, content_digest)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT DO NOTHING",
            )
            .bind(session_id)
            .bind(&message_id)
            .bind(command_id)
            .bind(&content_digest)
            .execute(&mut *transaction)
            .await?;
            if inserted.rows_affected() == 0 {
                let existing: (String, String) = sqlx::query_as(
                    "SELECT command_id, content_digest
                     FROM server_message_command_map
                     WHERE session_id = $1 AND message_id = $2
                     FOR UPDATE",
                )
                .bind(session_id)
                .bind(&message_id)
                .fetch_one(&mut *transaction)
                .await?;
                if existing.0 != command_id || existing.1 != content_digest {
                    return Err(PersistenceError::ProtocolIntegrity(
                        "message ID is already bound to another command".into(),
                    ));
                }
            }
        }
        sqlx::query(
            "INSERT INTO server_command_outbox
                (command_id, session_id, daemon_id, server_command_seq, payload,
                 payload_digest, command_identity_digest)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(command_id)
        .bind(session_id)
        .bind(&daemon_id)
        .bind(next_sequence)
        .bind(&payload)
        .bind(&payload_digest)
        .bind(&command_identity_digest)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(PinnedCommand {
            command_id: command_id.to_owned(),
            session_id: session_id.to_owned(),
            daemon_id,
            server_command_seq,
            payload,
            payload_digest,
            command_identity_digest,
            compacted: false,
        })
    }
}

fn message_identity(payload: &str) -> Option<(String, String)> {
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    let command = value.get("payload")?.get("command")?;
    if command.get("type")?.as_str()? != "message.send" {
        return None;
    }
    let command_payload = command.get("payload")?;
    let message_id = command_payload.get("message_id")?.as_str()?.to_owned();
    let content = command_payload.get("content")?.as_str()?;
    (!message_id.trim().is_empty() && !content.trim().is_empty())
        .then(|| (message_id, crate::payload_digest(content)))
}

async fn validate_active_repository_ids(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    repository_ids: &[String],
) -> Result<(), PersistenceError> {
    for repository_id in repository_ids {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM repositories
                 WHERE id = $1 AND disabled_at IS NULL
             )",
        )
        .bind(repository_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !exists {
            return Err(PersistenceError::RepositoryNotFound);
        }
    }
    Ok(())
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
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
}

#[derive(Debug, FromRow)]
struct ExistingCommandRow {
    command_id: String,
    session_id: String,
    daemon_id: String,
    server_command_seq: i64,
    payload: Option<String>,
    payload_digest: String,
    command_identity_digest: String,
    compacted: bool,
}

impl ExistingCommandRow {
    fn into_pinned_command(self) -> Result<PinnedCommand, PersistenceError> {
        let payload = self.payload.unwrap_or_default();
        if !self.compacted
            && (crate::payload_digest(&payload) != self.payload_digest
                || !crate::command_identity_digest_valid(&payload, &self.command_identity_digest))
        {
            return Err(PersistenceError::ProtocolIntegrity(
                "command outbox payload digest mismatch".into(),
            ));
        }
        Ok(PinnedCommand {
            command_id: self.command_id,
            session_id: self.session_id,
            daemon_id: self.daemon_id,
            server_command_seq: u64::try_from(self.server_command_seq)
                .map_err(|_| PersistenceError::InvalidSessionState)?,
            payload,
            payload_digest: self.payload_digest,
            command_identity_digest: self.command_identity_digest,
            compacted: self.compacted,
        })
    }
}

#[derive(Debug, FromRow)]
struct SessionOwnerRow {
    daemon_id: Option<String>,
    requirement_id: Option<String>,
    repository_ids: Vec<String>,
    repository_context_initialized: bool,
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
