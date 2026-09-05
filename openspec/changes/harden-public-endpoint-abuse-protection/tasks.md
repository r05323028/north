# Public endpoint abuse implementation tasks

## 1. Identity and proxy boundary

- [ ] Define socket-peer extraction and canonical IPv4/IPv6 normalization,
      including IPv4-mapped IPv6.
- [ ] Implement fixed `X-Forwarded-For` parsing with configured
      `trusted_proxy_cidrs`; default empty.
- [ ] Walk trusted chains right-to-left, reject malformed/missing chains
      safely, and fall back to immediate peer; ignore untrusted forwarding
      headers.
- [ ] Define IPv4 `/24` and IPv6 `/64` coarse bucket keys without using
      User-Agent, cookies, email local parts, or daemon labels.
- [ ] Add unit tests for direct, trusted, untrusted, multi-hop, malformed,
      all-trusted, and mapped-address cases.

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
- [ ] Apply client bucket and transactionally enforce bounded pending setup
      quota (default maximum 3 unexpired/unclaimed rows per client/network) on
      `/daemon/setup/request` using canonical client/network identity, never
      label alone.
- [ ] Define expired/pending row counting and retain bounded cleanup; rejected
      requests create no setup row or credential.
- [ ] Add concurrent PostgreSQL tests for quota bypass attempts, client/resource
      isolation, no resource creation on rejection, and legitimate success.

## 4. Errors and observability

- [ ] Implement stable HTTP 429 `{ "error": "rate_limited" }` and positive
      integer `Retry-After` without disclosing which control fired.
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
