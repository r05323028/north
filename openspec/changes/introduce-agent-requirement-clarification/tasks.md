## 1. Orchestration

- [ ] 1.1 Server session store + state transitions wired to protocol commands (start/cancel/resume/message.send)
- [ ] 1.2 Context assembly service (structured req + thread + repo catalog)

## 2. Runtime

- [ ] 2.1 Daemon Runtime trait + first concrete impl behind feature; SDK dep confined to daemon
- [ ] 2.2 Event mapping: runtime output → agent.message/agent.activity (coarse-grained filter)
- [ ] 2.3 Completion carries assessment payload → server gate validation e2e test

## 3. Guards

- [ ] 3.1 Test: no CoT/raw tool text reaches message/activity stores
- [ ] 3.2 Archtests still green (domain SDK-free)

## 4. Validation

- [ ] 4.1 Full Rust gate
- [ ] 4.2 openspec validate --strict
