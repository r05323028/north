# Design: bounded protection for public creation endpoints

## 1. Scope and ordering

Only these unauthenticated resource-creating endpoints are in scope:

- `POST /auth/request-code`
- `POST /daemon/setup/request`

Apply basic request parsing/normalization first. For a syntactically valid
request, apply the client-wide limiter before the durable resource-specific
transaction. A rejected request creates no code, setup row, credential, or
other protected resource. Existing cleanup may run only as part of the normal
setup path and does not make a rejected request successful.

The request-code verification endpoint is not rate-limited by this capability;
its existing failed-verification budget remains a separate control.

## 2. Canonical client identity

Use immediate socket peer address as the base identity. Normalize every address
to a canonical IP value before comparison or keying:

- IPv4 stays IPv4.
- IPv6 stays IPv6.
- IPv4-mapped IPv6 (`::ffff:192.0.2.1`) becomes IPv4 `192.0.2.1`.
- A malformed or missing address is not accepted as a trusted client claim;
  use the transport's peer failure path rather than an attacker-provided value.

Client bucket grouping is explicit and fixed for 0.1.0: IPv4 `/24` and IPv6
`/64`. Derive prefixes from IP bits, never by string truncation. Encode the
bucket key with an address-family tag and canonical network bytes (for example,
`v4:<first-24-bits>` or `v6:<first-64-bits>`), so IPv4-mapped IPv6 shares the
same IPv4 key after normalization. The exact normalized address remains the
request's immediate-peer identity; the typed network key is used for coarse
buckets and the durable setup quota. Do not use daemon labels, email local
parts, User-Agent, cookies, or arbitrary headers as client identity.

## 3. Forwarded header trust

North chooses one mechanism: `X-Forwarded-For`. It is ignored unless the
**immediate** socket peer belongs to configured `trusted_proxy_cidrs`; the
default configuration is empty, so direct deployments trust no forwarded
header. The header name is fixed; clients cannot select another forwarding
header.

For a trusted immediate peer, parse one logical comma-separated chain of valid
IP tokens from right to left; duplicate header fields are joined in wire order
before parsing. Remove trusted proxy hops, then use the first untrusted address
as the client address. Normalize mapped IPv6 before the trusted-range check. If
the header is missing, empty, contains an invalid/empty token, or contains no
untrusted address, fall back to the immediate socket peer. A malformed header is
never partially accepted.

Operators must list only proxies that overwrite/append this header correctly.
An untrusted direct caller supplying `X-Forwarded-For` can therefore not choose
its bucket identity. Trusted-proxy configuration is explicit and observable at
startup; no guessed private-network trust is enabled.

## 4. Limiter storage and limits

Use an in-process, concurrency-safe token bucket keyed by endpoint plus grouped
client network. It is intentionally coarse and resets on process restart; this
restart behavior is documented and does not claim cross-instance protection.
Defaults are bounded and configurable:

- request-code: capacity 5, refill 1 token per 120 seconds per client network;
- daemon setup: capacity 5, refill 1 token per 120 seconds per client network.

The implementation calculates a safe integer `Retry-After` from the bucket and
never exposes bucket counts. One process-local mutex/map is enough; do not add a
new cache service or abstraction layer.

Durable resource-specific controls remain separate:

- request-code uses the existing normalized-email one-active-code and cooldown
  transaction. The cooldown is not merged with the client bucket.
- daemon setup uses a transactionally enforced pending quota keyed by the
  canonical typed client/network key. Persist that derived key on every setup
  request row; do not recompute it from mutable proxy configuration later. Count
  unexpired, unclaimed rows for that key, including pending or approved rows;
  claimed rows and expired rows do not count even before cleanup. The 0.1.0
  default maximum is 3 unexpired, unclaimed rows per network key. The create
  transaction takes a deterministic per-key PostgreSQL advisory transaction lock,
  counts, and inserts under that lock so concurrent requests cannot pass the
  same quota. The existing 24-hour expiry cleanup remains bounded. Add a
  partial index over `(client_network_key, expires_at)` for rows with
  `claimed_at IS NULL`; retain the existing expiry index for cleanup.

The setup-key migration SHALL add a nullable `client_network_key` column because
pre-existing rows have no recorded peer identity. Existing rows with a null key
retain normal approval/claim/expiry behavior but are excluded from new keyed
quota counts; the application SHALL require a non-null key for every new row and
must not place legacy rows in one shared attacker-visible bucket. Legacy rows
age out under the existing retention policy.

A valid setup request includes a bounded daemon-label length check, but the label
is display data only and is never the sole quota key. Rejected client or pending
quota checks happen before setup-row insertion. Concurrent requests serialize
on the per-key transaction lock so two requests cannot both pass a pending quota
check.

No Redis, shared cache, HA lease, or distributed-rate guarantee is implied. A
multi-process deployment must place this boundary behind one trusted edge or
accept per-process buckets; it must not pretend process-local state is global.

## 5. Stable rejection contract

All limiter/quota rejections use:

- HTTP `429 Too Many Requests`;
- `Retry-After` with a positive integer seconds value when a useful retry time
  is known (use the safe maximum when multiple controls reject); and
- a minimal stable JSON body such as `{ "error": "rate_limited" }`.

The body, status, and headers do not identify client-vs-resource quota, email
existence, pending-row counts, daemon labels, client identity, or internal
limiter names. Do not return verification codes, setup tokens, daemon
credentials, or raw exception text. Existing generic auth/setup errors remain
generic; successful request-code responses remain code-free.

A request rejected by the client bucket, email cooldown, or setup pending quota
is still a rejection with no protected-resource creation. Existing cooldown
responses may be normalized to this 429 contract only where needed to avoid
revealing which email/resource is active; verification-attempt errors remain
their existing generic contract.

## 6. Observability

Emit counters/structured events with endpoint (`request_code` or `daemon_setup`),
allowed/rejected outcome, and coarse limiter category. Category names may be
`client_bucket`, `email_cooldown`, or `pending_setup`; they are operator
telemetry, not response data. Never log codes, daemon credentials, setup tokens,
raw `X-Forwarded-For`, raw email addresses, full daemon labels, or raw resource
identifiers. If correlation is needed, use a short non-reversible request ID.

## 7. Testable behavior

Use an injectable clock/token-bucket state for unit tests, real PostgreSQL
transactions for durable quotas, and explicit peer/header inputs for identity
tests. Cover concurrency, proxy spoofing, mapped IPv6, process restart reset,
endpoint/resource isolation, no-row-on-rejection, successful auth/setup, 429
shape, Retry-After, cooldown-versus-bucket distinction, and secret-free
responses. The task checklist is the implementation ledger.
