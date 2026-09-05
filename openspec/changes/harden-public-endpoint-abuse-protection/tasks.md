# Public endpoint abuse implementation tasks

## 1. Identity and proxy boundary

- [ ] Define socket-peer extraction and canonical IPv4/IPv6 normalization,
      including IPv4-mapped IPv6.
- [ ] Implement fixed `X-Forwarded-For` parsing with configured
      `trusted_proxy_cidrs`; default empty.
- [ ] Walk trusted chains right-to-left, reject malformed/missing chains
      safely, and fall back to immediate peer; ignore untrusted forwarding
      headers.
- [ ] Define typed IPv4 `/24` and IPv6 `/64` network keys from address bits;
      normalize mapped IPv6 before deriving the key and reuse the exact key for
      process buckets and durable setup quotas.
- [ ] Add unit tests for direct, trusted, untrusted, multi-hop, malformed,
      duplicate-header, all-trusted, mapped-address, and prefix-boundary cases.

## 2. Process-local client limiter

- [ ] Add the smallest concurrency-safe token buckets for the two in-scope
      endpoints, with documented defaults (capacity 5, refill 1/120s).
- [ ] Make clock/token state injectable for deterministic tests.
- [ ] Document process-local scope and reset-on-restart behavior; add no Redis,
      HA, provider registry, or generic limiter service.
- [ ] Prove endpoint isolation and that concurrent requests cannot bypass the
      in-process bucket.

## 3. Resource-specific endpoint integration

- [ ] Apply client bucket before durable creation on `/auth/request-code`.
- [ ] Preserve normalized-email one-active-code, cooldown, supersession, and
      generic code-free response semantics; keep verification-attempt budget
      separate.
- [ ] Add migration for nullable `client_network_key` with an explicit legacy-row
      policy: existing null-key rows retain claim/expiry behavior but are not
      counted for new keyed quotas; require non-null keys on new rows and never
      fabricate identities.
- [ ] Persist the derived typed network key on new setup-request rows and
      transactionally enforce bounded quota (default maximum 3 unexpired/unclaimed
      rows per key) under a deterministic per-key advisory transaction lock; never
      use label alone.
- [ ] Add the keyed pending-count index (`client_network_key`, `expires_at`)
      for unclaimed rows while retaining the existing expiry-cleanup index.
- [ ] Define pending/approved/claimed/expired row counting and retain bounded
      cleanup; rejected requests create no setup row or credential.
- [ ] Add concurrent PostgreSQL tests for quota bypass attempts, client/resource
      isolation, claimed/expired-row behavior, no resource creation on rejection,
      and legitimate success.

## 4. Errors and observability

- [ ] Implement stable HTTP 429 `{ "error": "rate_limited" }` and positive
      integer `Retry-After` without disclosing which control fired; use the
      maximum safe retry delay when multiple controls reject.
- [ ] Keep invalid/auth/setup errors generic and responses free of codes,
      tokens, credentials, raw runtime details, and account enumeration clues.
- [ ] Add safe endpoint/category/allow-reject metrics or structured events;
      redact raw email, labels, forwarding headers, codes, and credentials.
- [ ] Test cooldown versus client-rate rejection as distinct controls.

## 5. Documentation and validation

- [ ] Add/update public-endpoint-abuse-protection, email-auth, and daemon-runtime
      deltas; remove daemon-runtime's deferred public-abuse bullet.
- [ ] Update architecture, persistence, security/invariant, testing, lifecycle,
      and daemon setup docs with trust, storage, restart, and 429 semantics.
- [ ] Run unit, PostgreSQL integration, architecture, and HTTP boundary tests;
      do not mark unexecuted layers complete.
- [ ] Run `openspec validate --all --strict` and relevant `scripts/validate.sh`
      profiles.
