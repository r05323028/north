# Testing

Run everything before finishing a change; CI mirrors these commands.

## Rust (workspace root)

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace        # includes crates/north-archtests structural checks
```

Expectations:

- Domain invariants get unit tests next to the code (`north-domain`).
- New architectural boundary ⇒ new rule in `north-archtests/tests/architecture.rs`
  plus the matching row in docs/architecture/dependency-boundaries.md.
- Warnings are failures in CI (`-D warnings`); keep builds clean locally too.

## Web (apps/web)

```bash
npm ci
npm run lint
npm run typecheck
npm run build
```

Components come from shadcn/ui (`npx shadcn@latest add <component>`); do not fork
them casually. Frontend adds its own tests once feature surface exists (change
introduce-requirement-board establishes the pattern).

## Specs

```bash
openspec validate --all --strict
```

Run before committing change artifacts and before archiving.
