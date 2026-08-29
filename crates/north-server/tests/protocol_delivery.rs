use north_persistence::{
    AuthStore, EventReceiptOutcome, EventReceiptRequest, PersistenceError, PoolOptions,
};
use north_protocol::{Command, CommandEnvelope, MessageSend, SCHEMA_VERSION};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn event_payload(event_id: &str, sequence: u64) -> (String, String) {
    let value = serde_json::json!({
        "event_id": event_id,
        "daemon_event_seq": sequence,
        "fact": "agent.activity",
    });
    let payload = serde_json::to_string(&value).expect("event payload");
    let digest = north_persistence::canonical_payload_digest(&value);
    (payload, digest)
}

fn command_payload(command_id: &str, session_id: &str, sequence: u64) -> String {
    north_protocol::ServerFrame::Command(CommandEnvelope {
        command_id: command_id.into(),
        session_id: session_id.into(),
        server_command_seq: sequence,
        sent_at: "2026-01-01T00:00:00Z".into(),
        schema_version: SCHEMA_VERSION,
        command: Command::MessageSend(MessageSend {
            message_id: format!("message-{command_id}"),
            content: "hello".into(),
        }),
    })
    .to_json()
    .expect("valid command payload")
}

async fn connected_daemon(pool: &north_persistence::PgPool, user_id: &str) -> String {
    let daemon_id = unique("delivery-daemon");
    sqlx::query(
        "INSERT INTO daemon_registrations
            (daemon_id, credential_hash, label, created_by, protocol_version,
             capabilities, connected_at, last_seen_at)
         VALUES ($1, $2, $3, $4, '0.1', '[\"delivery-only\"]', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(&daemon_id)
    .bind(daemon_id.as_bytes().to_vec())
    .bind(&daemon_id)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("insert connected daemon");
    daemon_id
}

#[tokio::test]
#[ignore = "requires NORTH_TEST_DATABASE_URL; run explicitly with an isolated database"]
async fn durable_delivery_survives_lost_ack_gaps_and_retry() {
    let database_url = std::env::var("NORTH_TEST_DATABASE_URL")
        .expect("NORTH_TEST_DATABASE_URL is required for protocol delivery tests");
    let pool = PoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .expect("connect test database");
    north_persistence::run_migrations(&pool)
        .await
        .expect("run migrations");
    let user_id = unique("delivery-user");
    sqlx::query("INSERT INTO users (id, email, role) VALUES ($1, $2, 'Owner')")
        .bind(&user_id)
        .bind(format!("{user_id}@example.com"))
        .execute(&pool)
        .await
        .expect("insert user");
    let daemon_id = connected_daemon(&pool, &user_id).await;
    let store = AuthStore::new(pool.clone());
    let session_id = unique("delivery-session");

    let failed = store
        .start_session_with_command(
            &session_id,
            &unique("rolled-back-command"),
            &["delivery-only".into()],
            |_daemon_id, _sequence| Err(PersistenceError::InvalidCommandPayload),
        )
        .await;
    assert!(matches!(
        failed,
        Err(PersistenceError::InvalidCommandPayload)
    ));

    let command_one_id = unique("command-one");
    let command_one = store
        .start_session_with_command(
            &session_id,
            &command_one_id,
            &["delivery-only".into()],
            |_, sequence| Ok(command_payload(&command_one_id, &session_id, sequence)),
        )
        .await
        .expect("command one");
    assert_eq!(command_one.server_command_seq, 1);

    let duplicate = store
        .start_session_with_command(
            &session_id,
            &command_one_id,
            &["delivery-only".into()],
            |_, sequence| Ok(command_payload(&command_one_id, &session_id, sequence)),
        )
        .await
        .expect("duplicate command returns original");
    assert_eq!(duplicate, command_one);

    let command_two_id = unique("command-two");
    let command_two = store
        .start_session_with_command(
            &session_id,
            &command_two_id,
            &["delivery-only".into()],
            |_, sequence| Ok(command_payload(&command_two_id, &session_id, sequence)),
        )
        .await
        .expect("command two");
    assert_eq!(command_two.server_command_seq, 2);

    let pending = store
        .unacknowledged_commands(&daemon_id)
        .await
        .expect("pending commands");
    assert_eq!(
        pending
            .iter()
            .map(|command| command.command_id.clone())
            .collect::<Vec<_>>(),
        vec![command_one_id.clone(), command_two_id.clone()]
    );

    let watermark = store
        .acknowledge_command(&command_two_id, &session_id, 2)
        .await
        .expect("ack command two");
    assert_eq!(watermark, 0, "ACK above a gap cannot advance watermark");
    let watermark = store
        .acknowledge_command(&command_one_id, &session_id, 1)
        .await
        .expect("ack command one");
    assert_eq!(watermark, 2);
    assert!(store
        .unacknowledged_commands(&daemon_id)
        .await
        .expect("empty pending commands")
        .is_empty());
    let tombstones: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM server_command_tombstones
         WHERE session_id = $1",
    )
    .bind(&session_id)
    .fetch_one(&pool)
    .await
    .expect("count command tombstones");
    assert_eq!(tombstones, 2);
    let compacted = store
        .start_session_with_command(
            &session_id,
            &command_one_id,
            &["delivery-only".into()],
            |_, sequence| Ok(command_payload(&command_one_id, &session_id, sequence)),
        )
        .await
        .expect("compacted duplicate");
    assert!(compacted.compacted);
    assert!(compacted.payload.is_empty());
    assert_eq!(
        store
            .acknowledge_command(&command_one_id, &session_id, 1)
            .await
            .expect("late compacted ACK"),
        2
    );

    let event_two_id = unique("event-two");
    let (event_two_payload, event_two_digest) = event_payload(&event_two_id, 2);
    let event_gap = store
        .record_event_receipt_with_payload(EventReceiptRequest {
            event_id: &event_two_id,
            session_id: &session_id,
            daemon_event_seq: 2,
            payload_digest: &event_two_digest,
            payload: &event_two_payload,
            outcome: EventReceiptOutcome::Accepted,
            rejection_reason: None,
        })
        .await;
    assert!(matches!(
        event_gap,
        Err(PersistenceError::EventSequenceGap {
            expected: 1,
            received: 2
        })
    ));
    let event_one_id = unique("event-one");
    let (event_one_payload, event_one_digest) = event_payload(&event_one_id, 1);
    let first = store
        .record_event_receipt_with_payload(EventReceiptRequest {
            event_id: &event_one_id,
            session_id: &session_id,
            daemon_event_seq: 1,
            payload_digest: &event_one_digest,
            payload: &event_one_payload,
            outcome: EventReceiptOutcome::Accepted,
            rejection_reason: None,
        })
        .await
        .expect("event one");
    assert!(!first.duplicate);
    let duplicate = store
        .record_event_receipt_with_payload(EventReceiptRequest {
            event_id: &event_one_id,
            session_id: &session_id,
            daemon_event_seq: 1,
            payload_digest: &event_one_digest,
            payload: &event_one_payload,
            outcome: EventReceiptOutcome::Accepted,
            rejection_reason: None,
        })
        .await
        .expect("duplicate event");
    assert!(duplicate.duplicate);
    let conflict_id = unique("different-event");
    let (conflict_payload, conflict_digest) = event_payload(&conflict_id, 1);
    let conflict = store
        .record_event_receipt_with_payload(EventReceiptRequest {
            event_id: &conflict_id,
            session_id: &session_id,
            daemon_event_seq: 1,
            payload_digest: &conflict_digest,
            payload: &conflict_payload,
            outcome: EventReceiptOutcome::Accepted,
            rejection_reason: None,
        })
        .await;
    assert!(matches!(
        conflict,
        Err(PersistenceError::ProtocolIntegrity(_))
    ));
    let second = store
        .record_event_receipt_with_payload(EventReceiptRequest {
            event_id: &event_two_id,
            session_id: &session_id,
            daemon_event_seq: 2,
            payload_digest: &event_two_digest,
            payload: &event_two_payload,
            outcome: EventReceiptOutcome::Rejected,
            rejection_reason: Some("stale"),
        })
        .await
        .expect("event two");
    assert_eq!(second.outcome, EventReceiptOutcome::Rejected);

    let reconciliation = store
        .reconciliation_for_daemon(&daemon_id)
        .await
        .expect("reconciliation");
    assert_eq!(reconciliation.len(), 1);
    assert_eq!(reconciliation[0].command_ack_through_seq, 2);
    assert_eq!(reconciliation[0].event_ack_through_seq, 2);
}
