# repositories Specification

## Purpose

Gives administrators a credential-free repository catalog with stable historical
identity, explicit soft-disable/re-enable lifecycle, deterministic reads, and
an enabled-only catalog for downstream runtime consumers. Git inspection and
workspace behavior are owned by `introduce-local-repository-inspection`.

## ADDED Requirements

### Requirement: Repository identity and fields are explicit

Each repository SHALL contain an immutable UUID `id`, editable `name`, an
immutable-after-create `url`, editable `description`, immutable `created_at`,
mutable `updated_at`, and nullable `disabled_at`. The persistence model MAY
contain a derived `name_normalized` key for uniqueness, but SHALL contain no
credential, token, password, secret, SSH-key, or credential-helper field. The
URL SHALL be immutable in North 0.1.0 so `repository_id + commit_sha` remains
tied to one source identity.

#### Scenario: URL cannot retarget history

- **WHEN** an update attempts to change repository X's URL
- **THEN** the server returns an immutable-field conflict, makes no mutation,
  and requires disable-old/create-new behavior for a different source

#### Scenario: Metadata update preserves identity

- **WHEN** an Admin updates name or description on an enabled or disabled row
- **THEN** the UUID, URL, created timestamp, and historical repository
  references remain unchanged and only allowed metadata/timestamps update

### Requirement: Repository metadata is normalized and validated server-side

Before persistence, the server SHALL trim all text fields and validate bounds.
`name` SHALL be non-empty and at most 100 UTF-8 bytes. The server SHALL derive
`name_normalized` by applying non-locale Unicode lowercase to trimmed `name`,
and SHALL enforce uniqueness on that derived key across enabled and disabled
rows. `description` SHALL be empty or at most 10,000 UTF-8 bytes after trim.
`url` SHALL be non-empty and at most 2,048 UTF-8 bytes after trim. Frontend
validation SHALL not replace these server and database checks.

#### Scenario: Name normalization rejects a duplicate

- **WHEN** an Admin creates `North Repo` while `north repo` already exists
- **THEN** validation trims the new name and the normalized-name uniqueness
  conflict returns HTTP conflict with no second row

#### Scenario: Empty and overlong metadata is rejected

- **WHEN** a create or metadata update contains an empty name, overlong name or
  description, or empty/overlong URL
- **THEN** the server returns its normal bad-request response and persists
  nothing

### Requirement: Repository URLs cannot contain credentials

The server SHALL accept only supported 0.1 Git location shapes:
`https://<host>/<non-empty path>`, `ssh://[git@]<host>/<non-empty path>`, or
`git@<host>:<non-empty path>`. HTTPS userinfo is forbidden. URL passwords are
forbidden for every scheme, and SSH/SCP userinfo is allowed only for the
literal `git` transport user. Malformed locations, empty host/path, other
schemes, and credential-bearing URL material SHALL be rejected. Normal SSH
identity syntax using `git@` SHALL remain valid. North SHALL store only the
trimmed location string; daemon-host Git configuration supplies credentials.

#### Scenario: Credential-bearing HTTPS URL is rejected

- **WHEN** an Admin submits `https://user:password@example.com/repo.git` or
  `https://token@example.com/org/repo.git`
- **THEN** the server returns bad request, stores no row, and exposes no secret
  through repository persistence or DTOs

#### Scenario: Normal SSH identity is accepted

- **WHEN** an Admin submits `git@github.com:org/repo.git` or
  `ssh://git@github.com/org/repo.git`
- **THEN** shape validation accepts the metadata without treating `git` as a
  stored North credential

### Requirement: Repository management is Admin/Owner-only

Admin and Owner SHALL be authorized to create, update metadata, disable,
re-enable, and read the management list. Requester and Requirement Manager
SHALL be rejected server-side with the existing forbidden semantics before any
mutation. This change SHALL NOT grant management access merely because an
active catalog is consumed by a session or inspection. Any indirect repository
metadata shown by another authorized Requirement/review surface follows that
surface's existing read contract.

#### Scenario: Requester cannot manage repositories

- **WHEN** a Requester attempts any repository management operation
- **THEN** the server returns forbidden and the repository table is unchanged

#### Scenario: Requirement Manager cannot manage repositories

- **WHEN** a Requirement Manager attempts create, update, disable, re-enable, or
  management-list access
- **THEN** the server returns forbidden and no management state changes

### Requirement: Remove is unconditional soft-disable

Normal Remove SHALL always mean soft-disable, whether or not readiness evidence
currently references the row. For an enabled row it SHALL set `disabled_at` to
a server UTC timestamp. For an already disabled row it SHALL be an idempotent
success that preserves the original `disabled_at`. The row, identity, metadata,
and history SHALL remain. North 0.1.0 SHALL expose no normal hard-delete API,
command, or Settings affordance.

#### Scenario: Unreferenced repository is still disabled, not deleted

- **WHEN** an Admin removes a repository with no readiness evidence
- **THEN** its row remains with `disabled_at` set, it leaves the active catalog,
  and no hard-delete operation occurs

#### Scenario: Referenced repository preserves evidence

- **WHEN** an Admin removes repository X after evidence records X at full commit
  SHA `abc123`
- **THEN** X remains readable by ID with `disabled_at`, and the evidence still
  resolves X and `abc123`

#### Scenario: Repeated Remove is idempotent

- **WHEN** an Admin removes an already disabled repository
- **THEN** the operation succeeds without deleting or duplicating the row and
  retains its first disable timestamp

### Requirement: Disabled repositories can be re-enabled by identity

Admin and Owner SHALL be able to re-enable a disabled repository. Re-enable
SHALL clear `disabled_at` on the same UUID row, preserve name, immutable URL,
description, timestamps/history, and return the row to the active catalog. It
SHALL never create a duplicate identity row. Re-enable of an enabled row is an
idempotent success or no-op under the existing management response convention.

#### Scenario: Re-enable restores the same row

- **WHEN** an Admin re-enables repository X
- **THEN** X's UUID and historical references remain unchanged, `disabled_at`
  becomes null, and X appears in the active catalog

#### Scenario: Requester cannot re-enable

- **WHEN** a Requester or Requirement Manager attempts re-enable
- **THEN** the server returns forbidden and `disabled_at` remains unchanged

### Requirement: Management list and active catalog are distinct

The management list SHALL be an Admin/Owner read containing enabled and disabled
rows, current metadata, and `disabled_at` for Settings lifecycle controls. The
active runtime catalog SHALL contain only rows where `disabled_at IS NULL` and
is the source for new session context and downstream inspection candidates.
Both reads SHALL sort by `name_normalized ASC, id ASC`. Disabled repositories
MUST NOT disappear from management merely because they are excluded from active
runtime selection.

#### Scenario: Settings sees disabled history

- **WHEN** an Admin opens the repository management list after disabling X
- **THEN** the list includes X and its disabled status, while the active catalog
  excludes X

#### Scenario: Active catalog excludes disabled rows

- **WHEN** server assembles a new session context or inspection candidate list
- **THEN** only enabled rows appear, in deterministic normalized-name/ID order

### Requirement: Duplicate create has explicit conflict semantics

A create whose normalized name matches an enabled row SHALL return HTTP conflict
and create no row. A create whose normalized name matches a disabled row SHALL
also return HTTP conflict, identify the retained repository where authorized,
and direct the caller to re-enable that identity; it SHALL not silently create a
second durable row. Concurrent create/update races SHALL rely on the database
unique constraint and map to the same conflict semantics.

#### Scenario: Disabled name cannot create a duplicate

- **WHEN** an Admin creates a repository with the normalized name of disabled X
- **THEN** the request conflicts, X remains the only row, and the response
  directs the caller toward re-enable rather than duplicate creation

#### Scenario: Concurrent names remain unique

- **WHEN** two authorized creates concurrently normalize to the same name
- **THEN** at most one commits and the other receives the repository conflict
  response without partial metadata

### Requirement: Historical repository identity remains interpretable

Disabling SHALL retain the repository row and current metadata needed for
historical joins. Readiness evidence SHALL retain repository ID and exact full
commit SHA; the evidence SHALL remain interpretable without active-catalog
membership. Name or description edits may change current displayed metadata,
but SHALL NOT change repository ID, immutable URL, or recorded commit SHA. The
configured-repositories change SHALL not implement source inspection or SHA
resolution.

#### Scenario: Historical assessment remains readable after disable

- **WHEN** repository X is disabled after an assessment records X and full SHA
  `abc123...`
- **THEN** historical reads resolve retained X metadata and the exact SHA even
  though new active selection rejects X

### Requirement: Settings UI reflects catalog lifecycle without hard delete

Repository Settings SHALL be available to Admin/Owner, show enabled and
disabled rows, expose create and name/description metadata editing, use Remove
to disable, and expose re-enable for disabled rows. It SHALL show validation and
conflict errors and SHALL have no hard-delete affordance. UI checks are only
usability; server authorization, validation, uniqueness, and lifecycle rules
remain authoritative.

#### Scenario: Settings offers recovery, not deletion

- **WHEN** an Admin views a disabled repository in Settings
- **THEN** the UI shows its status and a re-enable action, and provides no hard
  delete action

### Requirement: Git inspection ownership remains downstream

Configured repositories SHALL publish enabled metadata to downstream consumers
without implementing host Git behavior. `introduce-local-repository-inspection`
owns Git invocation/authentication, cache/fetch/clone, disposable checkouts,
concurrency isolation, dirty-tree detection, disposal, and full commit SHA
resolution/reporting. This change SHALL not duplicate those tasks.

#### Scenario: Catalog does not start inspection work

- **WHEN** a repository is created, updated, disabled, or re-enabled
- **THEN** only catalog metadata/lifecycle changes occur; no clone, fetch,
  checkout, dirty-tree scan, or source inspection starts
