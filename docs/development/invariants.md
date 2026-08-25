# Invariant ledger

Every invariant lists how it is enforced today. When you add one, extend this
ledger AND add enforcement (test, domain type, constraint) — documentation alone
is not enforcement.

| # | Invariant | Enforcement |
| --- | --- | --- |
| 1 | Daemon reports facts/events; server owns business state transitions | crate boundaries (archtests); server-side transition APIs |
| 2 | Browser never communicates directly with the daemon | `browser_never_opens_websockets` structural test; no daemon URL reaches the client |
| 3 | Requirement state survives daemon disconnects and runtime-log expiry | durable vs ephemeral split (persistence.md); TTL GC never touches durable tables |
| 4 | Ready valid only for exact assessed revision; edits demote Ready→Discussing | `Requirement::mark_ready` + `apply_edit` unit tests |
| 5 | Accept / Request Changes / Reject / Reopen are human-only, reviewer-gated | `Role::can_review`; server enforces on transition endpoints |
| 6 | Request Changes ≠ Reject; both start from Ready | lifecycle edge table (`status.rs`) |
| 7 | Conversation is context, not source of truth | product doc; UI reads structured fields |
| 8 | Repository inspection read-only; credentials never reach the server | daemon-local git; no credential fields in protocol/persistence |
| 9 | Stable ids on every command/event; at-least-once + idempotent processing | protocol envelope contract (server-daemon-protocol.md) |
| 10 | Execution failure ≠ requirement failure; Failed only after retry budget exhausted | execution-state model separate from lifecycle |
| 11 | First account becomes Owner atomically; no self-promotion | DB unique/partial-constraint claim; `assign_role` rules |
| 12 | Ephemeral runtime data never sole source of truth | retention design (persistence.md) |

Review-level (until surfaces exist): frontend owns no lifecycle logic; agent SDKs
stay out of north-domain (structural once SDK deps would appear).
