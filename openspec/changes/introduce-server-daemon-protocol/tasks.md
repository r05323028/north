# Tasks

## 1. Types and control frames

- [x] 1.1 Add distinct `CommandEnvelope`/`EventEnvelope` fields with `command_id`/`event_id`, `session_id`, directional sequence, `sent_at`, and `schema_version`; keep connection/control schemas separate and validate exact `protocol_version` hello/welcome. JSON codec validates the pure wire values.
- [x] 1.2 Add baseline `command_ack`, event ACK accepted/rejected status, reconciliation watermark, and `protocol.error` payload/frame coverage for the existing catalog.
- [ ] 1.3 Make `protocol.error` bidirectional: add the `DaemonFrame` variant, preserve the `ServerFrame` variant, define no-severity terminal semantics, and add round-trip/validation tests for both directions.

## 2. Durable delivery slices

- [ ] 2.1 Define durable command-outbox schema with immutable envelope/payload digest, globally unique `command_id`, unique `(session_id, server_command_seq)`, and retained ACK/tombstone fields.
- [ ] 2.2 Implement transactional server sequence allocation plus outbox insertion; prove rollback does not consume a sequence value or create a committed hole.
- [ ] 2.3 Implement durable server `command_ack` processing, identity/digest validation, contiguous `command_ack_through_seq`, and ACK conflict handling.
- [ ] 2.4 Implement ascending resend of unacknowledged outbox rows with original ID, sequence, session, and serialized payload; test lost ACK/reconnect.
- [ ] 2.5 Implement daemon durable command inbox keyed by global `command_id` and `(session_id, server_command_seq)` with payload digest and duplicate lookup.
- [ ] 2.6 Implement daemon `received` → `dispatch_started` → `terminal` state transitions; persist `dispatch_started` before runtime invocation and record completed/failed/unknown outcome metadata.
- [ ] 2.7 Implement restart recovery: continue `received`, reattach `dispatch_started` by stable runtime operation identity, and never blindly resubmit an unknown side-effecting operation.
- [ ] 2.8 Emit and journal explicit unknown execution outcome as `session.failed { recoverable: false }` with stable command/runtime identity and server-owned next-step policy.
- [ ] 2.9 Enforce `message.send` command/message identity mapping, immutable content, and at-most-one automatic runtime submission across reconnect and daemon restart.
- [ ] 2.10 Implement atomic daemon event-sequence allocation plus journal append; prove failed append leaves no committed sequence hole.
- [ ] 2.11 Implement ascending event replay from the daemon journal with original event ID/sequence/payload and durable replay eligibility.
- [ ] 2.12 Implement server event transaction: identity/digest dedupe, immutable evidence/business effect or durable rejection, and ACK only after commit.
- [ ] 2.13 Implement terminal `event_ack(status=accepted)` and `event_ack(status=rejected)` handling; rejected ACK must not retry and both outcomes must update reconciliation state.
- [ ] 2.14 Add command/event same-ID, same-sequence, payload-mismatch, and cross-sequence conflict checks with no side effect and no ACK for conflicting frames.
- [ ] 2.15 Implement per-session bounded gap buffering/reconciliation with `max_gap_buffer_entries_per_session`, no business effect before gaps close, and retryable overflow behavior.
- [ ] 2.16 Implement reconciliation merge for command watermark, event contiguous watermark, sparse event ACKs, ascending command resend, ascending event replay, and late duplicate inertness.
- [ ] 2.17 Persist command/event watermarks and ID/digest tombstones across daemon/server restart; prove no late replay can reapply a business effect.
- [ ] 2.18 Implement safe compaction: payload removal only at durable boundaries, retained `processed_through_seq`, retained event dedupe protection, and no time-only tombstone expiry for relevant sessions.
- [ ] 2.19 Add fault-injection integration tests for lost ACKs, reconnect, daemon restart after each journal state, crash-after-dispatch unknown outcome, gap overflow, duplicate delivery, and compaction/restart.

## 3. Boundaries and validation

- [x] 3.1 Architecture tests confirm protocol purity and no new server↔daemon crate edge after adding dependencies.
- [x] 3.2 Run `./scripts/validate.sh fast` and `openspec validate --all --strict` after protocol integration tests pass.

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
- [x] 6.6 Align architecture/docs/OpenSpec contracts; durable outbox/journal/session coordination remain this change's unchecked implementation scope.
- [ ] 6.7 Add bidirectional protocol-error integration tests proving daemon-reported and server-reported violations close only the current connection and retain unacknowledged durable work.
