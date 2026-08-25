## 1. Persistence foundation

- [ ] 1.1 Add sqlx+Postgres to north-persistence; migration runner in server startup; migrations/0001 users, verification_codes, sessions, instance_settings(owner_user_id)
- [ ] 1.2 Row↔domain mappings stay inside north-persistence; north-domain untouched

## 2. Auth flow

- [ ] 2.1 POST /auth/request-code: create-or-supersede single active hashed code; deliver via CodeDelivery trait (log impl)
- [ ] 2.2 POST /auth/verify: constant-time check, consume code, transactional user insert + instance_settings owner claim (atomic), session row + HTTP-only cookie
- [ ] 2.3 Logout invalidates session server-side; auth middleware extracts current user
- [ ] 2.4 Concurrency test: two parallel verify calls on fresh instance yield exactly one Owner

## 3. Hygiene

- [ ] 3.1 No secret material in any success/error payload; add test asserting absence
- [ ] 3.2 Rate-limit code requests per email (config constant)

## 4. Docs

- [ ] 4.1 Update docs/architecture/persistence.md first-owner section with chosen mechanism

## 5. Validation

- [ ] 5.1 cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
- [ ] 5.2 openspec validate introduce-email-auth-and-owner-bootstrap --type change --strict
