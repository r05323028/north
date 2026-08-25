## 1. Route + panes

- [ ] 1.1 /requirements/[id] shell, three tabs, deep-linkable
- [ ] 1.2 Conversation pane (post/receive via SSE notifications); on reconnect/refocus refetch canonical state. Overview pane (fields verbatim + inline edit w/ revision notices); Activity pane (coarse entries)
- [ ] 1.3 Repo citations + Ready/execution badges

## 2. Boundaries

- [ ] 2.1 Fault-injection test: transcript unavailable ⇒ Overview unchanged
- [ ] 2.2 Snapshot test: no CoT/raw-tool rendering paths exist
- [ ] 2.3 E2E: SSE disconnect/missed/duplicate hints refetch canonical state and never duplicate messages or Requirement mutations

## 3. Validation

- [ ] 3.1 npm lint/typecheck/build
- [ ] 3.2 openspec validate --strict
