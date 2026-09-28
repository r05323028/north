macro_rules! entity {
    ($module:ident, $table:literal, primary $($(#[$pk_attr:meta])* $pk:ident: $pk_type:ty => $auto:literal),+; {
        $($(#[$field_attr:meta])* $field:ident: $field_type:ty),* $(,)?
    }) => {
        pub mod $module {
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
            #[sea_orm(table_name = $table)]
            pub struct Model {
                $(
                    $(#[$pk_attr])*
                    #[sea_orm(primary_key, auto_increment = $auto)]
                    pub $pk: $pk_type,
                )+
                $(
                    $(#[$field_attr])*
                    pub $field: $field_type,
                )*
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}

            impl ActiveModelBehavior for ActiveModel {}
        }
    };
}

entity!(users, "users", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    email: String,
    #[sea_orm(column_type = "Text")]
    role: String,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(requirement_board_positions, "requirement_board_positions", primary #[sea_orm(column_type = "Text")] requirement_id: String => false; {
    rank: i64,
});

entity!(verification_codes, "verification_codes", primary id: i64 => true; {
    #[sea_orm(column_type = "Text")]
    email: String,
    code_hash: Vec<u8>,
    expires_at: TimeDateTimeWithTimeZone,
    used_at: Option<TimeDateTimeWithTimeZone>,
    created_at: TimeDateTimeWithTimeZone,
    failed_attempts: i32,
});

entity!(sessions, "sessions", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    user_id: String,
    token_hash: Vec<u8>,
    expires_at: TimeDateTimeWithTimeZone,
    invalidated_at: Option<TimeDateTimeWithTimeZone>,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(instance_settings, "instance_settings", primary id: i16 => false; {
    #[sea_orm(column_type = "Text")]
    owner_user_id: Option<String>,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(requirements, "requirements", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    title: String,
    #[sea_orm(column_type = "Text")]
    description: String,
    #[sea_orm(column_type = "Text")]
    summary: String,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    acceptance_criteria: Vec<String>,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    assumptions: Vec<String>,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    open_questions: Vec<String>,
    #[sea_orm(column_type = "Text")]
    status: String,
    revision: i64,
    #[sea_orm(column_type = "Text")]
    created_by: String,
    created_at: TimeDateTimeWithTimeZone,
    updated_at: TimeDateTimeWithTimeZone,
    state_version: i64,
});

entity!(transition_audit, "transition_audit", primary id: i64 => true; {
    #[sea_orm(column_type = "Text")]
    requirement_id: String,
    #[sea_orm(column_type = "Text")]
    actor_id: String,
    #[sea_orm(column_type = "Text")]
    transition: String,
    #[sea_orm(column_type = "Text")]
    from_status: String,
    #[sea_orm(column_type = "Text")]
    to_status: String,
    #[sea_orm(column_type = "Text")]
    feedback: Option<String>,
    created_at: TimeDateTimeWithTimeZone,
    #[sea_orm(column_type = "Text")]
    assessment_id: Option<String>,
    state_version: Option<i64>,
});

entity!(conversations, "conversations", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    requirement_id: String,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(messages, "messages", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    conversation_id: String,
    #[sea_orm(column_type = "Text")]
    author_user_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    kind: String,
    #[sea_orm(column_type = "Text")]
    body: String,
    created_at: TimeDateTimeWithTimeZone,
    #[sea_orm(column_type = "Text")]
    source_event_id: Option<String>,
});

entity!(readiness_assessments, "readiness_assessments", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    event_id: String,
    #[sea_orm(column_type = "Text")]
    session_id: String,
    daemon_event_seq: i64,
    #[sea_orm(column_type = "Text")]
    event_requirement_id: String,
    #[sea_orm(column_type = "Text")]
    requirement_id: Option<String>,
    requirement_revision: i64,
    #[sea_orm(column_type = "Text")]
    verdict: String,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    blockers: Vec<String>,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    assumptions: Vec<String>,
    #[sea_orm(column_type = "JsonBinary")]
    repositories_reviewed: Json,
    #[sea_orm(column_type = "Text")]
    outcome: String,
    #[sea_orm(column_type = "Text")]
    rejection_reason: Option<String>,
    assessed_at_ms: i64,
    created_at: TimeDateTimeWithTimeZone,
    accepted_state_version: Option<i64>,
    generation_unknown: bool,
});

entity!(daemon_setup_requests, "daemon_setup_requests", primary #[sea_orm(column_type = "Text")] id: String => false; {
    request_token_hash: Vec<u8>,
    #[sea_orm(column_type = "Text")]
    label: String,
    #[sea_orm(column_type = "Text")]
    created_by: Option<String>,
    created_at: TimeDateTimeWithTimeZone,
    expires_at: TimeDateTimeWithTimeZone,
    approved_at: Option<TimeDateTimeWithTimeZone>,
    claimed_at: Option<TimeDateTimeWithTimeZone>,
    #[sea_orm(column_type = "Text")]
    daemon_id: Option<String>,
    #[sea_orm(column_type = "Cidr")]
    client_network_key: Option<IpNetwork>,
});

entity!(daemon_registrations, "daemon_registrations", primary #[sea_orm(column_type = "Text")] daemon_id: String => false; {
    credential_hash: Vec<u8>,
    #[sea_orm(column_type = "Text")]
    label: String,
    #[sea_orm(column_type = "Text")]
    created_by: String,
    created_at: TimeDateTimeWithTimeZone,
    revoked_at: Option<TimeDateTimeWithTimeZone>,
    last_seen_at: Option<TimeDateTimeWithTimeZone>,
    connected_at: Option<TimeDateTimeWithTimeZone>,
    #[sea_orm(column_type = "Text")]
    connection_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    protocol_version: String,
    #[sea_orm(column_type = "Text")]
    capabilities: String,
});

entity!(execution_sessions, "execution_sessions", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    daemon_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    state: String,
    created_at: TimeDateTimeWithTimeZone,
    #[sea_orm(column_type = "Text")]
    requirement_id: Option<String>,
    command_ack_through_seq: i64,
    event_ack_through_seq: i64,
    event_ack_sparse: Vec<i64>,
    #[sea_orm(column_type = "Array(RcOrArc::new(ColumnType::Text))")]
    repository_ids: Vec<String>,
    repository_context_initialized: bool,
    #[sea_orm(column_type = "Text")]
    start_message_id: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    start_context: Option<Json>,
    #[sea_orm(column_type = "Text")]
    start_command_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    cancel_command_id: Option<String>,
    cancel_requested: bool,
    #[sea_orm(column_type = "Text")]
    runtime_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    terminal_summary: Option<String>,
    #[sea_orm(column_type = "Text")]
    failure_reason: Option<String>,
    started_at: Option<TimeDateTimeWithTimeZone>,
    updated_at: TimeDateTimeWithTimeZone,
    last_activity_at: TimeDateTimeWithTimeZone,
    attempt_count: i64,
    max_attempts: i64,
    next_retry_at: Option<TimeDateTimeWithTimeZone>,
    #[sea_orm(column_type = "Text")]
    failure_class: Option<String>,
    #[sea_orm(column_type = "Text")]
    current_attempt_id: Option<String>,
});

entity!(server_command_outbox, "server_command_outbox", primary #[sea_orm(column_type = "Text")] command_id: String => false; {
    #[sea_orm(column_type = "Text")]
    session_id: String,
    #[sea_orm(column_type = "Text")]
    daemon_id: String,
    server_command_seq: i64,
    #[sea_orm(column_type = "Text")]
    payload: String,
    acknowledged_at: Option<TimeDateTimeWithTimeZone>,
    created_at: TimeDateTimeWithTimeZone,
    #[sea_orm(column_type = "Text")]
    payload_digest: String,
    #[sea_orm(column_type = "Text")]
    command_identity_digest: String,
});

entity!(repositories, "repositories", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    name: String,
    #[sea_orm(column_type = "Text")]
    name_normalized: String,
    #[sea_orm(column_type = "Text")]
    url: String,
    #[sea_orm(column_type = "Text")]
    description: String,
    created_at: TimeDateTimeWithTimeZone,
    updated_at: TimeDateTimeWithTimeZone,
    disabled_at: Option<TimeDateTimeWithTimeZone>,
});

entity!(server_command_tombstones, "server_command_tombstones", primary #[sea_orm(column_type = "Text")] command_id: String => false; {
    #[sea_orm(column_type = "Text")]
    session_id: String,
    #[sea_orm(column_type = "Text")]
    daemon_id: String,
    server_command_seq: i64,
    #[sea_orm(column_type = "Text")]
    payload: Option<String>,
    #[sea_orm(column_type = "Text")]
    payload_digest: String,
    #[sea_orm(column_type = "Text")]
    command_identity_digest: String,
    acknowledged_at: TimeDateTimeWithTimeZone,
});

entity!(server_message_command_map, "server_message_command_map", primary #[sea_orm(column_type = "Text")] session_id: String => false, #[sea_orm(column_type = "Text")] #[sea_orm(column_type = "Text")] message_id: String => false; {
    #[sea_orm(column_type = "Text")]
    command_id: String,
    #[sea_orm(column_type = "Text")]
    content_digest: String,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(server_event_dedupe, "server_event_dedupe", primary #[sea_orm(column_type = "Text")] event_id: String => false; {
    #[sea_orm(column_type = "Text")]
    session_id: String,
    daemon_event_seq: i64,
    #[sea_orm(column_type = "Text")]
    payload_digest: String,
    #[sea_orm(column_type = "Text")]
    payload: String,
    legacy_identity: bool,
    #[sea_orm(column_type = "Text")]
    outcome: String,
    #[sea_orm(column_type = "Text")]
    rejection_reason: Option<String>,
    created_at: TimeDateTimeWithTimeZone,
});

entity!(clarification_activities, "clarification_activities", primary id: i64 => true; {
    #[sea_orm(column_type = "Text")]
    event_id: String,
    #[sea_orm(column_type = "Text")]
    session_id: String,
    #[sea_orm(column_type = "Text")]
    activity: String,
    created_at: TimeDateTimeWithTimeZone,
    expires_at: TimeDateTimeWithTimeZone,
});

entity!(execution_attempts, "execution_attempts", primary #[sea_orm(column_type = "Text")] id: String => false; {
    #[sea_orm(column_type = "Text")]
    session_id: String,
    attempt_number: i64,
    #[sea_orm(column_type = "Text")]
    command_id: String,
    #[sea_orm(column_type = "Text")]
    command_kind: String,
    #[sea_orm(column_type = "Text")]
    outcome: Option<String>,
    #[sea_orm(column_type = "Text")]
    failure_event_id: Option<String>,
    #[sea_orm(column_type = "Text")]
    failure_class: Option<String>,
    #[sea_orm(column_type = "Text")]
    failure_reason: Option<String>,
    created_at: TimeDateTimeWithTimeZone,
    closed_at: Option<TimeDateTimeWithTimeZone>,
});

#[cfg(test)]
mod tests {
    use sea_orm::{sea_query::PostgresQueryBuilder, DbBackend, Schema};

    macro_rules! assert_entity {
        ($entity:expr, $table:literal; $($column:literal),+ $(,)?) => {{
            let ddl = Schema::new(DbBackend::Postgres)
                .create_table_from_entity($entity)
                .to_string(PostgresQueryBuilder);
            assert!(ddl.contains($table), "missing table {}: {ddl}", $table);
            $(assert!(ddl.contains($column), "missing column {} in {}: {ddl}", $column, $table);)+
        }};
    }

    #[test]
    fn entity_schema_matches_baseline_table_and_column_names() {
        assert_entity!(super::users::Entity, "users"; "id", "email", "role", "created_at");
        assert_entity!(super::requirement_board_positions::Entity, "requirement_board_positions"; "requirement_id", "rank");
        assert_entity!(super::verification_codes::Entity, "verification_codes"; "id", "code_hash", "failed_attempts");
        assert_entity!(super::sessions::Entity, "sessions"; "id", "user_id", "token_hash", "invalidated_at");
        assert_entity!(super::instance_settings::Entity, "instance_settings"; "id", "owner_user_id");
        assert_entity!(super::requirements::Entity, "requirements"; "id", "acceptance_criteria", "open_questions", "state_version");
        assert_entity!(super::transition_audit::Entity, "transition_audit"; "requirement_id", "assessment_id", "state_version");
        assert_entity!(super::conversations::Entity, "conversations"; "id", "requirement_id");
        assert_entity!(super::messages::Entity, "messages"; "conversation_id", "source_event_id");
        assert_entity!(super::readiness_assessments::Entity, "readiness_assessments"; "repositories_reviewed", "accepted_state_version", "generation_unknown");
        assert_entity!(super::daemon_setup_requests::Entity, "daemon_setup_requests"; "request_token_hash", "client_network_key");
        assert_entity!(super::daemon_registrations::Entity, "daemon_registrations"; "connection_id", "protocol_version");
        assert_entity!(super::execution_sessions::Entity, "execution_sessions"; "event_ack_sparse", "repository_ids", "current_attempt_id");
        assert_entity!(super::server_command_outbox::Entity, "server_command_outbox"; "payload_digest", "command_identity_digest");
        assert_entity!(super::repositories::Entity, "repositories"; "name_normalized", "disabled_at");
        assert_entity!(super::server_command_tombstones::Entity, "server_command_tombstones"; "payload_digest", "acknowledged_at");
        assert_entity!(super::server_message_command_map::Entity, "server_message_command_map"; "session_id", "message_id", "content_digest");
        assert_entity!(super::server_event_dedupe::Entity, "server_event_dedupe"; "daemon_event_seq", "payload_digest", "rejection_reason");
        assert_entity!(super::clarification_activities::Entity, "clarification_activities"; "event_id", "expires_at");
        assert_entity!(super::execution_attempts::Entity, "execution_attempts"; "attempt_number", "failure_event_id", "closed_at");
    }
}
