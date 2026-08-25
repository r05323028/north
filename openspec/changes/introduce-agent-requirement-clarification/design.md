# Design

## Context

One concrete agent SDK now; more later. Boundary lives between session
orchestration and runtime invocation.

## Decisions

- north-server owns sessions/state machine; north-daemon executes.
- Runtime trait inside daemon: prepare(context) → run(session) → stream
  events. One impl initially; no plugin registry, just a second impl when a
  second runtime appears.
- Context assembly server-side: structured requirement + recent conversation +
  repository catalog (metadata only).
- Assessment production is part of the runtime contract: session ends with
  either completed (+assessment payload) or failed.

## Open Questions

Which concrete SDK ships first — implementation detail, decided at build time.
