# Tooling

## prek (Git hooks)

[prek](https://github.com/j178/prek) is the hook runner. It consumes the
standard `.pre-commit-config.yaml` format (a separate `prek.toml` is not part
of its supported configuration surface).

```bash
# install hooks (once per clone)
prek install --install-hooks -t pre-commit -t pre-push -t commit-msg
prek run --all-files        # run everything manually
```

Hook policy:

- **pre-commit**: fast deterministic checks — file hygiene and rustfmt.
  Encourages frequent commits; never runs suites. Strict OpenSpec validation
  runs in `validate.sh fast/ci`, pre-push, CI, and act.
- **commit-msg**: Conventional Commit subject via
  `scripts/check-commit-message.sh`.
- **pre-push**: single entrypoint `scripts/pre-push-validation.sh`
  (native fast gate + web build + act CI parity). Escape hatch:
  `NORTH_PRE_PUSH_SKIP_ACT=1` — documented exception; remote CI still gates.

## act (local CI parity)

Runs the real workflow jobs in Docker before pushing:

```bash
act -W .github/workflows/ci.yml -j rust
NORTH_PRE_PUSH_JOB=web ./scripts/pre-push-validation.sh
```

Limitations are listed in docs/development/ci.md. Remote CI remains
authoritative.

## CodeGraph (conditional)

If `.codegraph/` exists at the repository root, prefer CodeGraph over broad
grep/find/source exploration when locating or understanding code:

```text
MCP:   codegraph_explore "<question>"
Shell: codegraph explore "<question>"  # versions that expose explore
This CLI: codegraph context "<question>" / codegraph query "<symbol>"
```

Locate symbols, ownership, dependency paths first; verify exact behavior
against current source before making correctness claims. If `.codegraph/`
does not exist, skip — never generate an index unprompted.

## Graphify (conditional)

If `graphify-out/` exists:

1. read `graphify-out/GRAPH_REPORT.md` before broad architecture exploration;
2. `graphify query "<question>"` for targeted codebase questions;
3. `graphify path "<source>" "<target>"` for dependency paths;
4. `graphify explain "<node>"` for focused symbol/concept inspection;
5. verify important conclusions against current source;
6. after code changes: `graphify update .` when Graphify is configured.

Treat `graphify-out/` as generated data: never hand-edit generated artifacts,
never infer semantics from filenames alone. Missing `graphify-out/` → skip.
