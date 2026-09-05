# public-endpoint-abuse-protection Specification Delta

## Purpose

Protects only unauthenticated authentication-code issuance and daemon setup
request creation with explicit client identity, resource quotas, and generic
rejections.

## ADDED Requirements

### Requirement: Client identity is canonical and proxy trust is explicit

For `/auth/request-code` and `/daemon/setup/request`, the server SHALL derive
client identity from the immediate socket peer after canonical IP
normalization. IPv4-mapped IPv6 SHALL normalize to IPv4. Coarse client buckets
SHALL group IPv4 by `/24` and IPv6 by `/64`.

The server SHALL use only `X-Forwarded-For` when the immediate peer is inside
configured `trusted_proxy_cidrs`. The default trusted-proxy set SHALL be empty.
An untrusted peer's forwarding header SHALL be ignored. A trusted chain SHALL be
parsed right-to-left as one complete comma-separated chain of IP tokens;
malformed/missing/empty/all-trusted chains SHALL fall back to the socket peer,
and no partial value SHALL be accepted. User-Agent, cookies, email local parts,
and daemon labels SHALL NOT be client identity. Duplicate header fields SHALL
be joined in wire order before validation.

#### Scenario: Untrusted forwarding header is ignored

- **WHEN** a direct client supplies a forged `X-Forwarded-For`
- **THEN** the server keys abuse controls by the direct socket peer

#### Scenario: Trusted multi-hop identity is normalized

- **WHEN** a configured trusted proxy supplies a valid multi-hop header containing
  trusted hops and a client address
- **THEN** the server selects the first untrusted address from the right,
  normalizes it, derives the fixed family-specific network prefix from address
  bits, and uses that typed bucket key

#### Scenario: Mapped IPv4 identity is equivalent

- **WHEN** one client reaches an endpoint once as `192.0.2.7` and once as
  `::ffff:192.0.2.7`
- **THEN** both requests use the same normalized IPv4 identity and typed `/24`
  network bucket

#### Scenario: Malformed forwarding input fails safe

- **WHEN** a trusted peer supplies a missing, empty, invalid, or all-trusted
  forwarding chain
- **THEN** the server falls back to the immediate peer and never trusts a
  partially parsed value

### Requirement: Public creation endpoints use bounded independent controls

Only `POST /auth/request-code` and `POST /daemon/setup/request` SHALL be
protected by this capability. Each SHALL apply a process-local, concurrency-safe
bucket keyed by endpoint plus client network before durable creation. The 0.1.0
default for each bucket SHALL be capacity 5 with one token refilled per 120
seconds, and buckets SHALL reset on process restart. The endpoints SHALL retain
separate durable resource controls and SHALL NOT claim cross-process or
cross-instance limiter guarantees, add Redis, or use a generic platform.

Request-code resource control SHALL use normalized email identity. New daemon
setup rows SHALL persist the canonical typed client/network key and enforce a
bounded count of unexpired, unclaimed rows for that key (default maximum 3 per
network key in 0.1.0). The transaction SHALL serialize count-and-insert for one
key; daemon label alone SHALL never be a quota key. Pre-existing null-key rows
retain claim/expiry behavior but are not counted for new keyed quotas. Rate-limited
or quota-rejected requests SHALL create no protected resource.

#### Scenario: Auth and setup buckets are isolated

- **WHEN** one client exhausts request-code quota
- **THEN** its eligible daemon setup requests use their own endpoint bucket, and
  vice versa

#### Scenario: Restart resets only process-local buckets

- **WHEN** the server process restarts
- **THEN** in-memory client buckets reset as documented while the persisted
  setup network key, durable email cooldown, code, setup-row expiry, and pending
  quotas remain authoritative

#### Scenario: Durable setup quota survives restart

- **WHEN** unexpired unclaimed setup rows for one typed network key exist before
  a process restart
- **THEN** the pending quota still counts those rows afterward, and a rejected
  request creates no new setup row

#### Scenario: Setup labels cannot bypass pending quota

- **WHEN** one client submits many setup requests with different daemon labels
- **THEN** the bounded pending quota still applies to that canonical client and
  rejected requests create no setup rows

### Requirement: Rejections have one generic rate-limit contract

Client-bucket and resource-quota rejections SHALL return HTTP `429`, stable
machine-readable error `rate_limited`, and a positive integer `Retry-After`
when a safe retry time is known. Body/header data SHALL NOT reveal which quota
fired, whether an email/account/resource exists, pending counts, labels, client
identity, or internal limiter state. Responses SHALL contain no verification
code, setup token, daemon credential, or raw exception.

#### Scenario: Concurrent abuse receives canonical 429

- **WHEN** concurrent valid creation requests exceed a client or resource
  control
- **THEN** rejected responses share the generic 429 contract and no rejected
  request creates a code or setup row

#### Scenario: Legitimate requests remain usable

- **WHEN** a request is within client and resource limits
- **THEN** existing auth/setup success behavior remains unchanged and no secret
  is included in the response

### Requirement: Public protection is observable without secrets

The server SHALL record safe endpoint, allowed/rejected outcome, and coarse
limiter category metrics or structured events. It SHALL NOT log verification
codes, setup tokens, daemon credentials, raw forwarding headers, raw email
addresses, full labels, or unnecessary raw resource identifiers.

#### Scenario: Rejection telemetry is redacted

- **WHEN** a public request is rejected by a limiter
- **THEN** telemetry can distinguish endpoint and safe category but contains no
  code, credential, token, raw email, or forwarding-header value
