## 1. Storage + API

- [ ] 1.1 Migration 0006: repositories table (unique name, `disabled_at`; NO credential columns)
- [ ] 1.2 Admin-guarded create/edit/list/soft-remove endpoints; active list and inspection catalog exclude disabled rows
- [ ] 1.3 Settings UI page (shadcn/ui form + table) distinguishes enabled/disabled without hard-delete affordance

## 2. Validation

- [ ] 2.1 Permission tests (non-admin refused), soft-disable/history test, and repository-schema credential scan
- [ ] 2.2 Full Rust gate + `openspec validate --all --strict`
