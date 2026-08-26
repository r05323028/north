## 1. Orchestration

- [ ] 1.1 Server session store selects/persists `daemon_id` before first durable command and wires state transitions to start/cancel/resume/message.send
- [ ] 1.2 Context assembly service converts server snapshots into typed `session.start` DTOs (full requirement fields, bounded conversation excerpt, enabled repository metadata) and allocates session/task checkouts; credentials and domain types never cross the wire

## 2. Runtime

- [ ] 2.1 Daemon Runtime trait + first concrete impl behind feature; SDK dep confined to daemon
- [ ] 2.2 Event mapping: runtime output → agent.message/agent.activity (coarse-grained filter)
- [ ] 2.3 Completion carries assessment payload → server dedupe/current-revision/domain transaction → post-commit ACK integration test

## 3. Guards

- [ ] 3.1 Test: no CoT/raw tool text reaches message/activity stores
- [ ] 3.2 Archtests still green (domain SDK-free; daemon has no business retry authority)
- [ ] 3.3 Integration: duplicate command/reconnect does not duplicate message submission; concurrent workspaces and dirty-checkout disposal are proven

## 4. Validation

- [ ] 4.1 Full Rust gate
- [ ] 4.2 openspec validate --strict
