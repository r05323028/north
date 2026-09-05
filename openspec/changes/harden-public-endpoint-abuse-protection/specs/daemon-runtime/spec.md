# daemon-runtime Specification Delta

## MODIFIED Requirements

### Requirement: Deferred 0.1.0 Hardening

Public abuse protection is no longer deferred. The active
`public-endpoint-abuse-protection` capability SHALL protect only
`POST /auth/request-code` and `POST /daemon/setup/request` with the documented
client identity, process-local client buckets, durable resource-specific
controls, generic 429 responses, and safe observability. Other deferred items
remain unchanged:

- Keyed OTP hashing with a server-side pepper remains a follow-up;
  current high-entropy session and daemon credential hashing is unchanged.
- Idempotent recovery after a committed one-shot setup claim response is lost
  remains deferred; plaintext credential recovery is not added.
- Multi-server/HA connection ownership epochs and durable command redelivery,
  replay, and ACK processing remain deferred where not already implemented.

#### Scenario: Public abuse item is removed from deferred scope

- **WHEN** the daemon-runtime specification is evaluated after this change
- **THEN** public request-code/setup-request abuse protection is an active
  capability dependency, not a claim that daemon runtime owns a generic limiter

#### Scenario: Unrelated hardening stays deferred

- **WHEN** this change is implemented
- **THEN** OTP peppering, lost setup-claim recovery, and HA ownership work are
  not silently added to the public endpoint limiter
