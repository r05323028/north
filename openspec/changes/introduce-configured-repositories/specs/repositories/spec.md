## Purpose

Gives admins a simple catalog of source repositories agents may consult —
metadata only, credentials never.

## ADDED Requirements

### Requirement: Admin-managed repository catalog

Admins and Owners SHALL create, edit, list, and delete repositories described
by id, name, url, and description. Non-admin actors SHALL be refused
server-side. Names SHALL be unique.

#### Scenario: Manager cannot add a repository

- **WHEN** a Requirement Manager POSTs a new repository
- **THEN** the request fails on permissions and nothing persists

### Requirement: Metadata only — no credentials

The repository model and its storage SHALL contain no credential material
(no tokens, secrets, keys, or password fields); URLs are stored verbatim.

#### Scenario: Schema rejects credential shapes

- **WHEN** the persistence layer is inspected
- **THEN** no column or field exists capable of holding Git credentials

### Requirement: History outlives catalog entries

Inspection records referencing a repository (id + commit SHA) SHALL remain
interpretable after the repository is deleted from the catalog.

#### Scenario: Delete does not orphan history

- **WHEN** an admin deletes a configured repository
- **THEN** past assessments still name it with its inspected SHA
