## 1. Workspace and crates

- [x] 1.1 Root `Cargo.toml` with members north-{domain,protocol,persistence,server,daemon} + north-archtests, shared lints (`unsafe_code = forbid`, clippy all)
- [x] 1.2 Dependency-free stub crates for hosts/protocol/persistence with doc anchors to owning changes
- [x] 1.3 Domain seed: status transition table, readiness model, requirement aggregate (`mark_ready`, `apply_edit` demotion), role matrix + unit tests

## 2. Architecture enforcement

- [x] 2.1 `north-archtests` structural tests: forbidden dependency edges per crate manifest
- [x] 2.2 Dumping-ground crate ban; frontend WebSocket ban (HTTP + SSE only)
- [ ] 2.3 Verify: intentionally add `sqlx` to north-domain, confirm test failure message, revert

## 3. Web app

- [x] 3.1 Next.js App Router scaffold, strict TS, Tailwind v4, shadcn/ui scaffolding (components.json, cn, Button)
- [x] 3.2 ESLint flat config (native eslint-config-next), lint/typecheck/build scripts
- [x] 3.3 npm install green; lint/typecheck/build exit 0

## 4. Docs and workflow

- [x] 4.1 AGENTS.md map; docs/{product,architecture,development} canonical docs; invariant ledger
- [x] 4.2 README quickstart; migrations/README convention; .gitignore
- [x] 4.3 OpenSpec config.yaml context + artifact rules

## 5. CI

- [x] 5.1 GitHub Actions: rust job (fmt, clippy -D warnings, test), web job (lint, typecheck, build), openspec job (validate --all --strict)
- [ ] 5.2 Confirm CI matches locally-passing commands exactly (no invented steps)

## 6. Final validation

- [ ] 6.1 `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- [ ] 6.2 `(cd apps/web && npm run lint && npm run typecheck && npm run build)`
- [ ] 6.3 `openspec validate --all --strict`
