use sea_orm::{ActiveModelTrait, ConnectionTrait, DbBackend, Schema};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Postgres);
        for table in [
            schema.create_table_from_entity(crate::entities::users::Entity),
            schema.create_table_from_entity(crate::entities::verification_codes::Entity),
            schema.create_table_from_entity(crate::entities::sessions::Entity),
            schema.create_table_from_entity(crate::entities::instance_settings::Entity),
            schema.create_table_from_entity(crate::entities::requirements::Entity),
            schema.create_table_from_entity(crate::entities::transition_audit::Entity),
            schema.create_table_from_entity(crate::entities::conversations::Entity),
            schema.create_table_from_entity(crate::entities::messages::Entity),
            schema.create_table_from_entity(crate::entities::readiness_assessments::Entity),
            schema.create_table_from_entity(crate::entities::daemon_setup_requests::Entity),
            schema.create_table_from_entity(crate::entities::daemon_registrations::Entity),
            schema.create_table_from_entity(crate::entities::execution_sessions::Entity),
            schema.create_table_from_entity(crate::entities::server_command_outbox::Entity),
            schema.create_table_from_entity(crate::entities::repositories::Entity),
            schema.create_table_from_entity(crate::entities::server_command_tombstones::Entity),
            schema.create_table_from_entity(crate::entities::server_message_command_map::Entity),
            schema.create_table_from_entity(crate::entities::server_event_dedupe::Entity),
            schema.create_table_from_entity(crate::entities::clarification_activities::Entity),
            schema.create_table_from_entity(crate::entities::execution_attempts::Entity),
        ] {
            manager.create_table(table).await?;
        }

        for statement in POST_TABLE_DDL {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        crate::entities::instance_settings::ActiveModel {
            id: sea_orm::ActiveValue::Set(1),
            owner_user_id: Default::default(),
            created_at: Default::default(),
        }
        .insert(manager.get_connection())
        .await?;
        Ok(())
    }
}

const POST_TABLE_DDL: &[&str] = &[
    // Defaults from the supported final schema.
    "ALTER TABLE users ALTER COLUMN role SET DEFAULT 'Requester', ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE verification_codes ALTER COLUMN failed_attempts SET DEFAULT 0, ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE sessions ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE instance_settings ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE requirements ALTER COLUMN summary SET DEFAULT '', ALTER COLUMN acceptance_criteria SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN assumptions SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN open_questions SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN status SET DEFAULT 'Draft', ALTER COLUMN revision SET DEFAULT 1, ALTER COLUMN state_version SET DEFAULT 1, ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP, ALTER COLUMN updated_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE transition_audit ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE conversations ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE messages ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE readiness_assessments ALTER COLUMN blockers SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN assumptions SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN generation_unknown SET DEFAULT FALSE, ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE daemon_setup_requests ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE daemon_registrations ALTER COLUMN capabilities SET DEFAULT '[]', ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE execution_sessions ALTER COLUMN state SET DEFAULT 'Idle', ALTER COLUMN command_ack_through_seq SET DEFAULT 0, ALTER COLUMN event_ack_through_seq SET DEFAULT 0, ALTER COLUMN event_ack_sparse SET DEFAULT ARRAY[]::BIGINT[], ALTER COLUMN repository_ids SET DEFAULT ARRAY[]::TEXT[], ALTER COLUMN repository_context_initialized SET DEFAULT FALSE, ALTER COLUMN cancel_requested SET DEFAULT FALSE, ALTER COLUMN attempt_count SET DEFAULT 0, ALTER COLUMN max_attempts SET DEFAULT 3, ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP, ALTER COLUMN updated_at SET DEFAULT CURRENT_TIMESTAMP, ALTER COLUMN last_activity_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE server_command_outbox ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE repositories ALTER COLUMN description SET DEFAULT '', ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP, ALTER COLUMN updated_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE server_message_command_map ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE server_event_dedupe ALTER COLUMN legacy_identity SET DEFAULT FALSE, ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE clarification_activities ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",
    "ALTER TABLE execution_attempts ALTER COLUMN created_at SET DEFAULT CURRENT_TIMESTAMP",

    // Unique constraints and CHECK constraints.
    "ALTER TABLE verification_codes ADD CONSTRAINT verification_codes_failed_attempts_check CHECK (failed_attempts >= 0)",
    "ALTER TABLE readiness_assessments ADD CONSTRAINT readiness_assessments_assessed_at_ms_check CHECK (assessed_at_ms >= 0)",
    "ALTER TABLE users ADD CONSTRAINT users_email_key UNIQUE (email), ADD CONSTRAINT users_role_check CHECK (role IN ('Owner', 'Admin', 'RequirementManager', 'Requester'))",
    "ALTER TABLE sessions ADD CONSTRAINT sessions_token_hash_key UNIQUE (token_hash), ADD CONSTRAINT sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE",
    "ALTER TABLE instance_settings ADD CONSTRAINT instance_settings_id_check CHECK (id = 1), ADD CONSTRAINT instance_settings_owner_user_id_fkey FOREIGN KEY (owner_user_id) REFERENCES users(id)",
    "ALTER TABLE requirements ADD CONSTRAINT requirements_title_check CHECK (CHAR_LENGTH(BTRIM(title)) BETWEEN 1 AND 500), ADD CONSTRAINT requirements_description_check CHECK (CHAR_LENGTH(BTRIM(description)) BETWEEN 1 AND 10000), ADD CONSTRAINT requirements_status_check CHECK (status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')), ADD CONSTRAINT requirements_revision_check CHECK (revision > 0), ADD CONSTRAINT requirements_state_version_check CHECK (state_version > 0), ADD CONSTRAINT requirements_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id)",
    "ALTER TABLE transition_audit ADD CONSTRAINT transition_audit_from_status_check CHECK (from_status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')), ADD CONSTRAINT transition_audit_to_status_check CHECK (to_status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')), ADD CONSTRAINT transition_audit_feedback_check CHECK (feedback IS NULL OR CHAR_LENGTH(BTRIM(feedback)) BETWEEN 1 AND 10000), ADD CONSTRAINT transition_audit_state_version_check CHECK (state_version IS NULL OR state_version > 0), ADD CONSTRAINT transition_audit_requirement_id_fkey FOREIGN KEY (requirement_id) REFERENCES requirements(id) ON DELETE CASCADE, ADD CONSTRAINT transition_audit_assessment_id_fkey FOREIGN KEY (assessment_id) REFERENCES readiness_assessments(id)",
    "ALTER TABLE conversations ADD CONSTRAINT conversations_requirement_id_key UNIQUE (requirement_id), ADD CONSTRAINT conversations_requirement_id_fkey FOREIGN KEY (requirement_id) REFERENCES requirements(id) ON DELETE CASCADE",
    "ALTER TABLE messages ADD CONSTRAINT messages_kind_check CHECK (kind IN ('requester', 'agent', 'system')), ADD CONSTRAINT messages_body_check CHECK (CHAR_LENGTH(BTRIM(body)) BETWEEN 1 AND 100000), ADD CONSTRAINT messages_check CHECK (kind <> 'requester' OR author_user_id IS NOT NULL), ADD CONSTRAINT messages_source_event_id_key UNIQUE (source_event_id), ADD CONSTRAINT messages_conversation_id_fkey FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE, ADD CONSTRAINT messages_author_user_id_fkey FOREIGN KEY (author_user_id) REFERENCES users(id)",
    "ALTER TABLE readiness_assessments ADD CONSTRAINT readiness_assessments_event_id_key UNIQUE (event_id), ADD CONSTRAINT readiness_assessments_session_id_daemon_event_seq_key UNIQUE (session_id, daemon_event_seq), ADD CONSTRAINT readiness_assessments_daemon_event_seq_check CHECK (daemon_event_seq > 0), ADD CONSTRAINT readiness_assessments_requirement_revision_check CHECK (requirement_revision > 0), ADD CONSTRAINT readiness_assessments_verdict_check CHECK (verdict IN ('ready', 'needs_clarification')), ADD CONSTRAINT readiness_assessments_repositories_reviewed_check CHECK (JSONB_TYPEOF(repositories_reviewed) = 'array'), ADD CONSTRAINT readiness_assessments_outcome_check CHECK (outcome IN ('accepted', 'rejected')), ADD CONSTRAINT readiness_assessments_check CHECK ((outcome = 'accepted' AND rejection_reason IS NULL) OR (outcome = 'rejected' AND rejection_reason IS NOT NULL)), ADD CONSTRAINT readiness_assessments_accepted_state_version_check CHECK (accepted_state_version IS NULL OR accepted_state_version > 0), ADD CONSTRAINT readiness_assessments_generation_consistency CHECK (((outcome = 'accepted') AND ((accepted_state_version IS NOT NULL AND generation_unknown = FALSE) OR (accepted_state_version IS NULL AND generation_unknown = TRUE))) OR ((outcome = 'rejected') AND accepted_state_version IS NULL AND generation_unknown = FALSE)), ADD CONSTRAINT readiness_assessments_requirement_id_fkey FOREIGN KEY (requirement_id) REFERENCES requirements(id)",
    "ALTER TABLE daemon_setup_requests ADD CONSTRAINT daemon_setup_requests_request_token_hash_key UNIQUE (request_token_hash), ADD CONSTRAINT daemon_setup_requests_label_check CHECK (CHAR_LENGTH(BTRIM(label)) BETWEEN 1 AND 100), ADD CONSTRAINT daemon_setup_requests_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id), ADD CONSTRAINT daemon_setup_requests_daemon_fk FOREIGN KEY (daemon_id) REFERENCES daemon_registrations(daemon_id)",
    "ALTER TABLE daemon_registrations ADD CONSTRAINT daemon_registrations_credential_hash_key UNIQUE (credential_hash), ADD CONSTRAINT daemon_registrations_connection_id_key UNIQUE (connection_id), ADD CONSTRAINT daemon_registrations_label_check CHECK (CHAR_LENGTH(BTRIM(label)) BETWEEN 1 AND 100), ADD CONSTRAINT daemon_registrations_protocol_version_check CHECK (CHAR_LENGTH(BTRIM(protocol_version)) > 0), ADD CONSTRAINT daemon_registrations_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id)",
    "ALTER TABLE execution_sessions ADD CONSTRAINT execution_sessions_state_check CHECK (state IN ('Idle', 'Running', 'Retrying', 'Failed', 'Completed')), ADD CONSTRAINT execution_sessions_command_ack_through_seq_check CHECK (command_ack_through_seq >= 0), ADD CONSTRAINT execution_sessions_event_ack_through_seq_check CHECK (event_ack_through_seq >= 0), ADD CONSTRAINT execution_sessions_clarification_context_check CHECK ((start_message_id IS NULL AND start_context IS NULL) OR (start_message_id IS NOT NULL AND start_context IS NOT NULL AND JSONB_TYPEOF(start_context) = 'object')), ADD CONSTRAINT execution_sessions_attempt_count_check CHECK (attempt_count >= 0), ADD CONSTRAINT execution_sessions_max_attempts_check CHECK (max_attempts > 0), ADD CONSTRAINT execution_sessions_failure_class_check CHECK (failure_class IS NULL OR failure_class IN ('runtime_failure', 'execution_outcome_unknown', 'retry_exhausted', 'owner_unavailable', 'cancelled')), ADD CONSTRAINT execution_sessions_failure_reason_check CHECK (failure_reason IS NULL OR CHAR_LENGTH(BTRIM(failure_reason)) BETWEEN 1 AND 256), ADD CONSTRAINT execution_sessions_daemon_id_fkey FOREIGN KEY (daemon_id) REFERENCES daemon_registrations(daemon_id), ADD CONSTRAINT execution_sessions_requirement_id_fkey FOREIGN KEY (requirement_id) REFERENCES requirements(id), ADD CONSTRAINT execution_sessions_start_message_id_fkey FOREIGN KEY (start_message_id) REFERENCES messages(id), ADD CONSTRAINT execution_sessions_current_attempt_fk FOREIGN KEY (current_attempt_id) REFERENCES execution_attempts(id) ON DELETE SET NULL",
    "ALTER TABLE server_command_outbox ADD CONSTRAINT server_command_outbox_session_id_server_command_seq_key UNIQUE (session_id, server_command_seq), ADD CONSTRAINT server_command_outbox_server_command_seq_check CHECK (server_command_seq > 0), ADD CONSTRAINT server_command_outbox_payload_digest_check CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0), ADD CONSTRAINT server_command_outbox_command_identity_digest_check CHECK (CHAR_LENGTH(BTRIM(command_identity_digest)) > 0), ADD CONSTRAINT server_command_outbox_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id) ON DELETE CASCADE, ADD CONSTRAINT server_command_outbox_daemon_id_fkey FOREIGN KEY (daemon_id) REFERENCES daemon_registrations(daemon_id)",
    "ALTER TABLE repositories ADD CONSTRAINT repositories_id_check CHECK (id = LOWER(id) AND id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'), ADD CONSTRAINT repositories_name_check CHECK (OCTET_LENGTH(BTRIM(name)) BETWEEN 1 AND 100), ADD CONSTRAINT repositories_name_normalized_key UNIQUE (name_normalized), ADD CONSTRAINT repositories_name_normalized_check CHECK (CHAR_LENGTH(BTRIM(name_normalized)) BETWEEN 1 AND 400), ADD CONSTRAINT repositories_url_check CHECK (OCTET_LENGTH(BTRIM(url)) BETWEEN 1 AND 2048), ADD CONSTRAINT repositories_description_check CHECK (OCTET_LENGTH(BTRIM(description)) BETWEEN 0 AND 10000)",
    "ALTER TABLE server_command_tombstones ADD CONSTRAINT server_command_tombstones_session_id_server_command_seq_key UNIQUE (session_id, server_command_seq), ADD CONSTRAINT server_command_tombstones_server_command_seq_check CHECK (server_command_seq > 0), ADD CONSTRAINT server_command_tombstones_payload_digest_check CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0), ADD CONSTRAINT server_command_tombstones_command_identity_digest_check CHECK (CHAR_LENGTH(BTRIM(command_identity_digest)) > 0), ADD CONSTRAINT server_command_tombstones_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id), ADD CONSTRAINT server_command_tombstones_daemon_id_fkey FOREIGN KEY (daemon_id) REFERENCES daemon_registrations(daemon_id)",
    "ALTER TABLE server_message_command_map ADD CONSTRAINT server_message_command_map_command_id_key UNIQUE (command_id), ADD CONSTRAINT server_message_command_map_message_id_check CHECK (CHAR_LENGTH(BTRIM(message_id)) > 0), ADD CONSTRAINT server_message_command_map_content_digest_check CHECK (CHAR_LENGTH(BTRIM(content_digest)) > 0), ADD CONSTRAINT server_message_command_map_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id)",
    "ALTER TABLE server_event_dedupe ADD CONSTRAINT server_event_dedupe_session_id_daemon_event_seq_key UNIQUE (session_id, daemon_event_seq), ADD CONSTRAINT server_event_dedupe_daemon_event_seq_check CHECK (daemon_event_seq > 0), ADD CONSTRAINT server_event_dedupe_payload_digest_check CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0), ADD CONSTRAINT server_event_dedupe_payload_check CHECK (CHAR_LENGTH(BTRIM(payload)) > 0), ADD CONSTRAINT server_event_dedupe_outcome_check CHECK (outcome IN ('accepted', 'rejected')), ADD CONSTRAINT server_event_dedupe_check CHECK ((outcome = 'accepted' AND rejection_reason IS NULL) OR (outcome = 'rejected' AND rejection_reason IS NOT NULL)), ADD CONSTRAINT server_event_dedupe_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id)",
    "ALTER TABLE clarification_activities ADD CONSTRAINT clarification_activities_event_id_key UNIQUE (event_id), ADD CONSTRAINT clarification_activities_activity_check CHECK (CHAR_LENGTH(BTRIM(activity)) BETWEEN 1 AND 1000), ADD CONSTRAINT clarification_activities_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id) ON DELETE CASCADE",
    "ALTER TABLE execution_attempts ADD CONSTRAINT execution_attempts_command_id_key UNIQUE (command_id), ADD CONSTRAINT execution_attempts_failure_event_id_key UNIQUE (failure_event_id), ADD CONSTRAINT execution_attempts_session_id_attempt_number_key UNIQUE (session_id, attempt_number), ADD CONSTRAINT execution_attempts_attempt_number_check CHECK (attempt_number > 0), ADD CONSTRAINT execution_attempts_command_kind_check CHECK (command_kind IN ('session.start', 'session.resume')), ADD CONSTRAINT execution_attempts_outcome_check CHECK (outcome IN ('completed', 'failed')), ADD CONSTRAINT execution_attempts_failure_class_check CHECK (failure_class IS NULL OR failure_class IN ('runtime_failure', 'execution_outcome_unknown', 'retry_exhausted', 'owner_unavailable', 'cancelled')), ADD CONSTRAINT execution_attempts_failure_reason_check CHECK (failure_reason IS NULL OR CHAR_LENGTH(BTRIM(failure_reason)) BETWEEN 1 AND 256), ADD CONSTRAINT execution_attempts_check CHECK ((outcome = 'failed' AND failure_event_id IS NOT NULL AND failure_class IS NOT NULL AND failure_reason IS NOT NULL) OR (outcome = 'completed' AND failure_event_id IS NULL AND failure_class IS NULL AND failure_reason IS NULL) OR (outcome IS NULL AND failure_event_id IS NULL AND failure_class IS NULL AND failure_reason IS NULL)), ADD CONSTRAINT execution_attempts_session_id_fkey FOREIGN KEY (session_id) REFERENCES execution_sessions(id) ON DELETE CASCADE",

    // Explicit indexes, including the partial indexes used by runtime queries.
    "CREATE UNIQUE INDEX verification_codes_one_active_per_email ON verification_codes (email) WHERE used_at IS NULL",
    "CREATE INDEX requirements_status_updated_at_idx ON requirements (status, updated_at DESC, id ASC)",
    "CREATE INDEX requirements_created_by_updated_at_idx ON requirements (created_by, updated_at DESC, id ASC)",
    "CREATE INDEX transition_audit_requirement_created_at_idx ON transition_audit (requirement_id, created_at ASC, id ASC)",
    "CREATE INDEX messages_conversation_created_at_idx ON messages (conversation_id, created_at ASC, id ASC)",
    "CREATE INDEX readiness_assessments_requirement_revision_idx ON readiness_assessments (requirement_id, requirement_revision, outcome, created_at DESC, id ASC)",
    "CREATE UNIQUE INDEX readiness_assessments_accepted_generation_idx ON readiness_assessments (requirement_id, accepted_state_version) WHERE outcome = 'accepted' AND accepted_state_version IS NOT NULL AND generation_unknown = FALSE",
    "CREATE INDEX daemon_setup_requests_expires_at_idx ON daemon_setup_requests (expires_at)",
    "CREATE INDEX execution_sessions_requirement_id_idx ON execution_sessions (requirement_id)",
    "CREATE UNIQUE INDEX execution_sessions_clarification_slot_idx ON execution_sessions (requirement_id) WHERE start_message_id IS NOT NULL AND state NOT IN ('Completed', 'Failed')",
    "CREATE INDEX execution_sessions_clarification_latest_idx ON execution_sessions (requirement_id, created_at DESC, id DESC) WHERE start_message_id IS NOT NULL",
    "CREATE INDEX repositories_catalog_order_idx ON repositories (name_normalized ASC, id ASC)",
    "CREATE INDEX server_command_tombstones_session_order_idx ON server_command_tombstones (session_id, server_command_seq ASC)",
    "CREATE INDEX server_event_dedupe_session_order_idx ON server_event_dedupe (session_id, daemon_event_seq ASC)",
    "CREATE INDEX clarification_activities_session_order_idx ON clarification_activities (session_id, created_at ASC, id ASC)",
    "CREATE INDEX execution_attempts_session_order_idx ON execution_attempts (session_id, attempt_number ASC)",
    "CREATE INDEX execution_sessions_retry_due_idx ON execution_sessions (state, next_retry_at, id) WHERE state = 'Retrying' AND next_retry_at IS NOT NULL",
    "CREATE INDEX clarification_activities_expires_at_idx ON clarification_activities (expires_at ASC, id ASC)",
    "CREATE INDEX daemon_setup_requests_client_network_key_expires_at_idx ON daemon_setup_requests (client_network_key, expires_at) WHERE claimed_at IS NULL",

    // PostgreSQL-only immutability triggers.
    r#"CREATE FUNCTION prevent_readiness_assessment_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $function$
    BEGIN
        IF pg_trigger_depth() > 1 THEN
            IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'readiness assessments are immutable';
    END;
    $function$"#,
    "CREATE TRIGGER readiness_assessments_immutable BEFORE UPDATE OR DELETE ON readiness_assessments FOR EACH ROW EXECUTE FUNCTION prevent_readiness_assessment_mutation()",
    r#"CREATE FUNCTION prevent_repository_url_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $function$
    BEGIN
        IF OLD.id IS DISTINCT FROM NEW.id OR OLD.url IS DISTINCT FROM NEW.url OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
            RAISE EXCEPTION 'repository identity, URL, and created_at are immutable';
        END IF;
        RETURN NEW;
    END;
    $function$"#,
    "CREATE TRIGGER repositories_url_immutable BEFORE UPDATE ON repositories FOR EACH ROW EXECUTE FUNCTION prevent_repository_url_mutation()",
    r#"CREATE FUNCTION prevent_server_command_outbox_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $function$
    BEGIN
        IF OLD.command_id IS DISTINCT FROM NEW.command_id OR OLD.session_id IS DISTINCT FROM NEW.session_id OR OLD.daemon_id IS DISTINCT FROM NEW.daemon_id OR OLD.server_command_seq IS DISTINCT FROM NEW.server_command_seq OR OLD.payload IS DISTINCT FROM NEW.payload OR OLD.payload_digest IS DISTINCT FROM NEW.payload_digest OR OLD.command_identity_digest IS DISTINCT FROM NEW.command_identity_digest THEN
            RAISE EXCEPTION 'server command outbox identity and payload are immutable';
        END IF;
        RETURN NEW;
    END;
    $function$"#,
    "CREATE TRIGGER server_command_outbox_immutable BEFORE UPDATE ON server_command_outbox FOR EACH ROW EXECUTE FUNCTION prevent_server_command_outbox_mutation()",
];
