# Tasks

## 1. Types and control frames

- [x] 1.1 Add distinct `CommandEnvelope`/`EventEnvelope` fields with `command_id`/`event_id`, `session_id`, directional sequence, `sent_at`, and `schema_version`; keep connection/control schemas separate and validate exact `protocol_version` hello/welcome. JSON codec validates the pure wire values.
- [x] 1.2 Add baseline `command_ack`, event ACK accepted/rejected status, reconciliation watermark, and `protocol.error` payload/frame coverage for the existing catalog.
- [x] 1.3 Make `protocol.error` bidirectional: add the `DaemonFrame` variant, preserve the `ServerFrame` variant, define no-severity terminal semantics, and add round-trip/validation tests for both directions.

## 2. Durable delivery slices

- [x] 2.1 Define durable command-outbox schema with immutable envelope/payload digest, globally unique `command_id`, unique `(session_id, server_command_seq)`, and retained ACK/tombstone fields.
- [x] 2.2 Implement transactional server sequence allocation plus outbox insertion; prove rollback does not consume a sequence value or create a committed hole.
- [x] 2.3 Implement durable server `command_ack` processing, identity/digest validation, contiguous `command_ack_through_seq`, and ACK conflict handling.
- [x] 2.4 Implement ascending resend of unacknowledged outbox rows with original ID, sequence, session, and serialized payload; test lost ACK/reconnect.
- [x] 2.5 Implement daemon durable command inbox keyed by global `command_id` and `(session_id, server_command_seq)` with payload digest and duplicate lookup.
- [x] 2.6 Define and use the narrow internal command dispatch/execution seam: durable journal state and idempotency are decided before crossing it, the seam accepts stable command/runtime operation identity, durable-delivery tests may use a deterministic fake executor, and no agent prompting or SDK behavior is introduced here.
- [x] 2.7 Implement daemon `received` → `dispatch_started` → `terminal` state transitions; persist `dispatch_started` before runtime invocation and record dispatch-succeeded/dispatch-failed/unknown local outcome metadata.
- [x] 2.8 Implement restart recovery: continue `received`, reattach `dispatch_started` by stable runtime operation identity, and never blindly resubmit an unknown side-effecting operation.
- [x] 2.9 Emit and journal explicit unknown execution outcome as `session.failed` with the daemon's local `recoverable` fact (`false` for `execution_outcome_unknown`), stable command/runtime identity, no automatic resubmission, and server-owned next-step policy.
- [x] 2.10 Enforce `message.send` command/message identity mapping, immutable content, and at-most-one automatic runtime submission across reconnect and daemon restart.
- [x] 2.11 Implement atomic daemon event-sequence allocation plus journal append; prove failed append leaves no committed sequence hole.
- [x] 2.12 Implement ascending event replay from the daemon journal with original event ID/sequence/payload and durable replay eligibility.
- [x] 2.13 Implement server event transaction: identity/digest dedupe, immutable evidence/business effect or durable rejection, and ACK only after commit.
- [x] 2.14 Implement terminal `event_ack(status=accepted)` and `event_ack(status=rejected)` handling; rejected ACK must not retry and both outcomes must update reconciliation state.
- [x] 2.15 Add command/event same-ID, same-sequence, payload-mismatch, and cross-sequence conflict checks with no side effect and no ACK for conflicting frames.
- [x] 2.16 Implement per-session bounded gap buffering/reconciliation with `max_gap_buffer_entries_per_session`, no business effect before gaps close, and retryable overflow behavior.
- [x] 2.17 Implement reconciliation merge for command watermark, event contiguous watermark, sparse event ACKs, ascending command resend, ascending event replay, and late duplicate inertness.
- [x] 2.18 Persist command/event watermarks and ID/digest tombstones across daemon/server restart; prove no late replay can reapply a business effect.
- [x] 2.19 Implement safe compaction: payload removal only at durable boundaries, retained `processed_through_seq`, retained event dedupe protection, and no time-only tombstone expiry for relevant sessions.
- [x] 2.20 Add fault-injection integration tests for lost ACKs, reconnect, daemon restart after each journal state, crash-after-dispatch unknown outcome, gap overflow, duplicate delivery, and compaction/restart. Executable coverage is provided by `durable_delivery_survives_lost_ack_gaps_and_retry`, `received_command_recovers_after_journal_reopen`, `dispatch_started_recovery_does_not_dispatch_again`, `terminal_command_is_inert_after_journal_reopen`, `unknown_recovery_emits_non_resubmittable_failure_fact`, `bounded_gap_buffers_then_drains_in_order`, and the compaction/replay restart tests.

## 3. Boundaries and validation

- [x] 3.1 Architecture tests confirm protocol purity and no new server↔daemon crate edge after adding dependencies.
- [x] 3.2 Existing wire/transport foundation validation completed; prior `./scripts/validate.sh fast` and `openspec validate --all --strict` coverage applies to that foundation, with durable-delivery validation recorded in task 3.3.
- [x] 3.3 Final durable-delivery validation: focused journal/restart/fault-injection tests, PostgreSQL-backed server integration, protocol transport integration, Rust `cargo fmt`/`cargo clippy`/`cargo test`, web validation, and `openspec validate --all --strict` after the corrected migration head is verified.

## 4. Transport standardization slice

- [x] 4.1 Fix North 0.1 wire representation as JSON text and keep codec errors distinct from WebSocket transport errors.
- [x] 4.2 Add the thin Axum daemon WebSocket upgrade/adapter with bounded channels, text decoding, control-frame handling, and configured message/frame limits.
- [x] 4.3 Add the daemon `tokio-tungstenite` connection supervisor with hello, split reader/writer tasks, bounded outbound buffering, ping/pong, and reconnect backoff.
- [x] 4.4 Add adapter boundary tests and update architecture/docs/OpenSpec terminology. Durable auth persistence, outbox/journal replay, and session coordination remain in their owning tasks.

## 5. Context and contract hardening

- [x] 5.1 Add server-owned `session.start` context assembly with requirement, bounded conversation excerpt, and enabled repository metadata DTOs; test disabled repository filtering.
- [x] 5.2 Replace opaque assessment text with typed readiness evidence and structural validation tests; keep domain conversion outside `north-protocol`.
- [x] 5.3 Make `session.resume` execution-only and standardize all ACK terminology as `command_ack` plus `event_ack(status=...)`.
- [x] 5.4 Add explicit daemon handshake phases, stage timeouts, terminal-vs-retryable failure classification, and pure-crate dependency allowlists.

## 6. Review hardening

- [x] 6.1 Model reconciliation as one validated connection-level snapshot supporting zero or multiple pinned sessions.
- [x] 6.2 Deliver welcome and reconciliation to coordination, gate `Active` on coordination readiness, and reset transport backoff after healthy activation.
- [x] 6.3 Bound post-upgrade hello and coordinator admission deadlines in the Axum adapter.
- [x] 6.4 Remove protocol-error severity state and define every `protocol.error` as terminal to the current connection.
- [x] 6.5 Add real Axum↔tokio-tungstenite integration tests for empty/multi-session snapshots, gating, protocol failure, and admission backpressure.
- [x] 6.6 Align architecture/docs/OpenSpec contracts; durable outbox/journal/session coordination are implemented by the completed 2.x tasks, while generic runtime-event business projections remain deferred.
- [x] 6.7 Add bidirectional protocol-error integration tests proving daemon-reported and server-reported violations close only the current connection and retain unacknowledged durable work.

## 7. Release correction

- [x] 7.1 Renumber repository and protocol migrations strictly after published main head 0012 as `0013_repositories.sql` then `0014_protocol_delivery.sql`; update exact SQL references and documentation.
- [x] 7.2 Replace the unsafe pre-0012 repository migration fixture with a forward-upgrade regression that applies exact historical 0001–0005 and 0007–0012 SQL, inserts legacy state, then applies exact 0013 and 0014 SQL; assert retained state, backfills, constraints, and citation data.
- [x] 7.3 Make generic event rejection, local `recoverable` meaning, server retry ownership, and `LocalRuntime` placeholder explicit across implementation-adjacent OpenSpec artifacts.
- [x] 7.4 Run corrected-head Rust, PostgreSQL integration, web, strict OpenSpec, diff, and CI-equivalent validation; archive only after all required gates pass.
