## Purpose

Gives administrators a credential-free repository catalog with soft-disable history, while daemon inspections use isolated disposable workspaces and exact source revisions.

## ADDED Requirements

### Requirement: Admin-managed, soft-disabled repository catalog

Admins and Owners SHALL create, edit, list, and remove repositories described
by id, name, URL, description, timestamps, and nullable `disabled_at`.
Non-admin actors SHALL be refused server-side. Names SHALL be unique. Normal
Remove SHALL set `disabled_at` instead of deleting a repository row referenced
by assessment evidence. Active lists SHALL exclude disabled rows.

#### Scenario: Manager cannot add or remove a repository

- **WHEN** a Requirement Manager attempts repository CRUD
- **THEN** the request fails on permissions and no catalog lifecycle change persists

#### Scenario: Remove disables rather than deletes

- **WHEN** an Admin removes repository X after an assessment recorded X at commit `abc123`
- **THEN** X remains durably addressable with `disabled_at`, active inspection selection excludes X, and history still resolves X and `abc123`

### Requirement: Metadata only — no credentials

The repository model and its storage SHALL contain no credential material (no
tokens, secrets, keys, passwords, or credential-helper contents); URLs are
stored as metadata and Git credentials remain on daemon hosts.

#### Scenario: Schema rejects credential shapes

- **WHEN** the persistence schema for configured repositories is inspected
- **THEN** it contains identity, metadata, and lifecycle fields only, with no Git credential field

### Requirement: History outlives active catalog state

Inspection evidence SHALL retain repository id and exact full commit SHA. The
retained disabled repository metadata SHALL keep that evidence human-readable;
hard deletion of a referenced row is not the normal 0.1.0 removal path.

#### Scenario: Historical assessment stays interpretable

- **WHEN** a repository is disabled after an assessment is recorded
- **THEN** the assessment still names the repository and inspected SHA without requiring an active catalog entry
