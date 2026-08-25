# Design

## Context

One concrete agent SDK now; more later. Boundary lives between session
orchestration and runtime invocation. Session ownership, durable command
semantics, and server retry authority are canonical in
`harden-distributed-system-architecture`.

## Decisions

- `north-server` owns sessions, `session.daemon_id`, command outbox rows,
  execution state, and all Requirement/business effects; `north-daemon`
  executes and reports facts.
- Runtime trait inside daemon: `prepare(context) → run(session) → stream`
  events. One impl initially; no plugin registry.
- Context assembly is server-side: structured requirement + recent conversation
  - enabled repository catalog metadata only. The runtime receives a
  session/task-specific disposable checkout when inspection is needed.
- The server selects an eligible daemon before `session.start`; all subsequent
  commands retain command id/sequence on retry and all events retain event
  id/sequence on replay. A different daemon cannot resume the session.
- Assessment production ends with `requirement.assessed`; server deduplicates,
  validates the event's revision through the domain, persists evidence and any
  valid transition atomically, then acknowledges it. A stale/invalid fact gets
  a durable rejection ACK without a Requirement transition.
- A daemon crash or socket reconnect does not reset server attempt count. The
  daemon reports local recoverability; only server policy sends `session.resume`
  or declares execution failure.

## Risks / Trade-offs

- **Runtime outcome is unknown after a crash** → command id is the runtime
  operation id; reattach when possible and do not duplicate side-effecting
  `message.send`.
- **Repository inspection is not a sandbox** → dispose dirty checkouts and
  report violations; keep the process-level limitation explicit.
