# Harden unauthenticated resource creation without adding platform infrastructure

## Why

`/auth/request-code` and `/daemon/setup/request` can create or refresh durable
work before authentication. Existing email cooldown, one-active-code, setup
expiry, and verification-attempt controls protect individual resources but do
not bound client-wide automation or define proxy identity. North 0.1.0 needs a
small, explicit boundary that cannot be bypassed by spoofed forwarding headers
or attacker-controlled daemon labels.

## What changes

- Add one `public-endpoint-abuse-protection` capability covering only the two
  unauthenticated creation endpoints.
- Define the normalized effective client address from the socket/proxy boundary,
  including trusted `X-Forwarded-For`, IPv4/IPv6 normalization, and
  IPv4-mapped normalization. Derive the primary limiter key as PostgreSQL `CIDR`
  IPv4 `/32` or IPv6 `/64`. Persist that same value as the setup quota key.
- Add process-local coarse client token buckets plus existing/durable
  resource-specific controls. No Redis, provider registry, HA, or generic
  abuse platform.
- Key request-code resource protection by normalized email. Persist the same
  CIDR primary limiter key as daemon setup's durable quota key and enforce
  bounded unexpired-unclaimed setup quota, never by an attacker-controlled
  daemon label alone.
- Preserve one active code, cooldown, supersession, bounded verification-failure
  budget, generic auth errors, and secret-free responses.
- Return one generic HTTP 429 contract with stable `rate_limited` code and safe
  `Retry-After`; do not reveal which quota fired or enable enumeration.
- Record safe endpoint/category/count observability without codes, credentials,
  raw email, or unnecessary resource identifiers.

## Capabilities

### New Capabilities

- `public-endpoint-abuse-protection`: client identity, bounded client/resource
  controls, generic 429 responses, and redacted observability.

### Modified Capabilities

- `email-auth`: request-code issuance gains separate client abuse control
  without changing code/cooldown/verification semantics.
- `daemon-runtime`: public request abuse is active rather than deferred;
  daemon runtime keeps setup lifecycle and credential ownership.

## Non-goals

No Redis/distributed limiter, HA coordination, provider registry, generic
workflow engine, authenticated endpoint policy, verification endpoint rewrite,
email provider, or daemon-label trust model.

## Dependencies

Consumes current `email-auth` and `daemon-runtime` persistence/endpoint
contracts. The deferred daemon-runtime abuse bullet is removed by this change;
other deferred hardening remains deferred.

## Documentation impact

Update security invariants, persistence, architecture, testing, daemon setup,
and email-auth documentation with the exact trust boundary, process-local
restart behavior, resource keys, and 429 contract.
