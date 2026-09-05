# daemon-protocol Specification Delta

## MODIFIED Requirements

### Requirement: Generic runtime events have delivery-only handling until projected

For well-formed `session.started`, `agent.message`, `agent.activity`,
`session.completed`, and `session.failed` events, the server SHALL retain the
existing identity, sequence, payload-integrity, dedupe, and ACK-after-commit
boundary, then invoke the owning server-side clarification/runtime projection
instead of returning `event_handler_not_implemented`. The projection SHALL
commit or durably reject the business fact before its terminal
`event_ack(status=accepted|rejected)`. `session.failed` is an
execution-attempt fact: retry policy may persist `Retrying`/`next_retry_at` or
terminal `Failed`; it never mutates Requirement lifecycle. Duplicate facts
remain replay-safe and inert. `session.resume` remains a server command only for
eligible non-terminal runs; a terminal unknown-outcome run cannot receive it.
Transport replay state remains in reconciliation/watermark records.

#### Scenario: Runtime event reaches its owning projection

- **WHEN** a valid runtime event arrives after its owning clarification/runtime
  projection is available
- **THEN** server coordination commits that projection or a durable domain
  rejection, sends the matching terminal ACK after commit, and does not record
  a generic `event_handler_not_implemented` rejection

#### Scenario: Failure event does not move business lifecycle

- **WHEN** a valid `session.failed` fact is accepted
- **THEN** server policy handles attempt retry/terminal failure idempotently while
  Requirement content, lifecycle, revision, and readiness remain unchanged

#### Scenario: Duplicate runtime event remains inert

- **WHEN** an already acknowledged runtime event is replayed
- **THEN** server returns its known ACK/outcome without repeating conversation,
  activity, execution, or retry effects

#### Scenario: Generic event receives terminal delivery rejection

- **WHEN** a valid generic runtime event arrives after its owning projection is
  available
- **THEN** server commits the owning projection or a durable domain rejection
  and sends its terminal ACK after commit, rather than a generic
  `event_handler_not_implemented` rejection

#### Scenario: Unknown outcome remains a local execution fact

- **WHEN** a daemon reports an unknown outcome after local reattachment fails
- **THEN** the server records the fact, creates no automatic `session.resume`,
  and any later execution uses a new logical run/protocol `session_id` and an
  explicit `session.start` command
