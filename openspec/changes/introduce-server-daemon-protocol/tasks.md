## 1. Types

- [x] 1.1 Add envelope fields and direction-specific frame enums with `command_id`/`event_id`, `session_id`, directional sequence, `schema_version`, and exact `protocol_version` hello/welcome. JSON codec validates the pure wire values.
- [x] 1.2 Add `command_ack(status=accepted)`, event ACK accepted/rejected status, reconciliation watermark, and `protocol.error` frames; round-trip serialization tests cover every frame family.

## 2. Durable delivery semantics

- [ ] 2.1 Implement server command outbox persistence before dispatch; resend unaccepted rows with the original id/sequence.
- [ ] 2.2 Implement daemon durable command inbox/processed journal; duplicate `message.send` tests prove one runtime submission across reconnect/restart and define `dispatch_started` recovery.
- [ ] 2.3 Implement daemon event journal replay and server post-commit `event_ack(status=accepted)`/durable-rejection `event_ack(status=rejected)`; test duplicate assessment and ACK-after-commit behavior.
- [ ] 2.4 Implement per-session directional sequence allocation, gap buffering/reconciliation, contiguous+sparse ACKs, late-frame no-op, and safe high-water compaction.
- [ ] 2.5 Reject incompatible/unknown frames deterministically with `protocol.error`; test no side effect and retained unacknowledged work.

## 3. Boundaries and validation

- [x] 3.1 Architecture tests confirm protocol purity and no new server↔daemon crate edge after adding dependencies.
- [ ] 3.2 Run `./scripts/validate.sh fast` and `openspec validate --all --strict` after protocol integration tests pass.

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
