# repositories Specification

## Purpose

Gives administrators a credential-free repository catalog with stable historical
identity, explicit soft-disable/re-enable lifecycle, deterministic reads, and
an enabled-only catalog for downstream runtime consumers. Git inspection and
workspace behavior are owned by `introduce-local-repository-inspection`.

## Requirements

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
literal `git` transport user. North 0.1 deliberately chooses this standard
literal-`git` policy even when a host could clone a URL using another username;
non-`git` usernames are not accepted as repository metadata in this release.
Malformed locations, empty host/path, other schemes, and credential-bearing URL
material SHALL be rejected. Normal SSH identity syntax using `git@` SHALL
remain valid. North SHALL store only the trimmed location string; daemon-host
Git configuration supplies credentials.

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

### Requirement: Repository lifecycle timestamps are explicit

The server SHALL generate all repository lifecycle timestamps in UTC. Creation
sets `created_at` and `updated_at` to the same current time and leaves
`disabled_at` null. A successful metadata change advances `updated_at`;
`created_at` never changes. Disabling an enabled row sets `disabled_at` to the
current time and advances `updated_at`. Disabling an already-disabled row is a
true idempotent no-op: it leaves both `disabled_at` and `updated_at` unchanged.
Re-enabling a disabled row clears `disabled_at` and advances `updated_at`; re-
enabling an already-enabled row is a true idempotent no-op that leaves
`updated_at` unchanged.

#### Scenario: Create initializes lifecycle timestamps

- **WHEN** an Admin creates repository X
- **THEN** `created_at` and `updated_at` are set to the server's current UTC
  time, `disabled_at` is null, and `created_at` never changes afterward

#### Scenario: Metadata change advances only update time

- **WHEN** an Admin successfully changes X's name or description
- **THEN** `updated_at` advances, `created_at` and `disabled_at` are unchanged,
  and the row identity/history remains unchanged

#### Scenario: Disable and repeated disable have distinct timestamp behavior

- **WHEN** an Admin disables enabled X and then disables X again
- **THEN** the first operation sets `disabled_at` and advances `updated_at`,
  while the second succeeds as a no-op with both timestamps unchanged

#### Scenario: Re-enable and repeated re-enable have distinct timestamp behavior

- **WHEN** an Admin re-enables disabled X and then re-enables X again
- **THEN** the first operation clears `disabled_at` and advances `updated_at`,
  while the second succeeds as a no-op with `updated_at` unchanged

### Requirement: Remove is unconditional soft-disable

Normal Remove SHALL always mean soft-disable, whether or not readiness evidence
currently references the row. For an enabled row it SHALL set `disabled_at` to a
server UTC timestamp and advance `updated_at`. For an already disabled row it
SHALL be an idempotent success that preserves the original `disabled_at` and
`updated_at`. The row, identity, metadata, and history SHALL remain. North
0.1.0 SHALL expose no normal hard-delete API, command, or Settings affordance.

#### Scenario: Unreferenced repository is still disabled, not deleted

- **WHEN** an Admin removes a repository with no readiness evidence
- **THEN** its row remains with `disabled_at` set, it leaves the active catalog,
  and no hard-delete operation occurs

#### Scenario: Referenced repository preserves evidence

- **WHEN** an Admin removes repository X after evidence records X at full commit
  SHA `abcdef0123456789abcdef0123456789abcdef01`
- **THEN** X remains readable by ID with `disabled_at`, and the evidence still
  resolves X and `abcdef0123456789abcdef0123456789abcdef01`

#### Scenario: Repeated Remove is idempotent

- **WHEN** an Admin removes an already disabled repository
- **THEN** the operation succeeds without deleting or duplicating the row and
  retains its first disable timestamp

### Requirement: Disabled repositories can be re-enabled by identity

Admin and Owner SHALL be able to re-enable a disabled repository. Re-enable
SHALL clear `disabled_at` on the same UUID row, advance `updated_at`, preserve
name, immutable URL, description, `created_at`, and history, and return the row
to the active catalog. It SHALL never create a duplicate identity row.
Re-enable of an enabled row is a true idempotent success/no-op and SHALL leave
`updated_at` unchanged under the timestamp convention above.

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
active runtime catalog SHALL be an enabled-only internal server/persistence read
filtered by `disabled_at IS NULL`, used for server-assembled `session.start`
context and downstream repository-inspection orchestration. It is not a public
browser repository-management endpoint and is not a daemon endpoint from which
the daemon independently fetches a catalog; relevant enabled metadata arrives
through `session.start`. North 0.1 SHALL not create a new HTTP catalog surface
merely because this internal read is called a catalog. The management list is the
explicit Settings-facing API. Both reads SHALL sort by `name_normalized ASC, id
ASC`. Disabled repositories MUST NOT disappear from management merely because
they are excluded from active runtime selection.

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
historical joins. Historical evidence is authoritative as `repository_id` plus
exact full `commit_sha`; URL is immutable in 0.1 and therefore keeps source
identity stable. Name or description edits may change current displayed
metadata, but SHALL NOT change repository ID, URL, or recorded commit SHA. Old
assessments do not claim name/description snapshots. Disabling does not alter
previous evidence, and re-enable clears `disabled_at` on the same identity.
The evidence remains interpretable without active-catalog membership.

#### Scenario: Historical assessment remains readable after disable

- **WHEN** repository X is disabled after an assessment records X and full SHA
  `abcdef0123456789abcdef0123456789abcdef01...`
- **THEN** historical reads resolve retained X metadata and the exact SHA even
  though new active selection rejects X

### Requirement: Readiness citations require durable identity and run provenance

Every `repositories_reviewed.repository_id` accepted into readiness evidence
MUST identify an existing durable configured-repository row. The server
readiness/persistence path SHALL reject an unknown repository identity as a
durable assessment rejection before evidence or Requirement promotion commits;
it SHALL not fabricate a repository row. Readiness owns whether the citation is
acceptable for the Requirement, while configured repositories own identity,
existence, and lifecycle. `introduce-local-repository-inspection` owns source
inspection and production of the exact commit SHA. `north-protocol` carries
`repository_id` and complete Git SHA-1/SHA-256 `commit_sha` as typed facts; it
SHALL not access repository persistence.

New inspection work SHALL require the repository to be enabled at selection and
start. Evidence from an already-running/in-flight inspection SHALL remain
eligible for readiness acceptance after an Admin disables the repository, provided
the durable row still exists and the citation was valid for that run: it was
included in the server-assembled `session.start` repository context or was
otherwise explicitly inspected under that session's authorized context. Disable
alone SHALL NOT invalidate the citation, and the repository need not be enabled
when the assessment arrives. This uses existing session context and
inspection-result contracts; it does not create a separate provenance subsystem.

#### Scenario: Unknown repository citation is rejected

- **WHEN** an assessment cites a non-empty but unknown `repository_id`
- **THEN** readiness records a durable rejection, does not insert accepted
  evidence or promote the Requirement, and does not create a repository row

#### Scenario: Disable during in-flight inspection preserves valid evidence

- **WHEN** `session.start` includes enabled repository R, inspection begins, an
  Admin disables R, and the later assessment cites R with the exact commit SHA
- **THEN** new selection excludes R, but readiness may accept the citation
  because R's durable row remains and R was valid for that session/run

#### Scenario: Disabled or unknown repository cannot start new inspection

- **WHEN** a new inspection selects a disabled or unknown repository
- **THEN** selection fails before inspection begins

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
without implementing host Git behavior. The server/readiness layer validates
repository citation existence and session/run acceptability; the local
inspection change produces the source commit SHA. The protocol carries these
values as typed facts. `introduce-local-repository-inspection` owns Git
invocation/authentication, cache/fetch/clone, disposable checkouts, concurrency
isolation, dirty-tree detection, disposal, and full commit SHA resolution/
reporting. This change SHALL not duplicate those tasks.

#### Scenario: Catalog does not start inspection work

- **WHEN** a repository is created, updated, disabled, or re-enabled
- **THEN** only catalog metadata/lifecycle changes occur; no clone, fetch,
  checkout, dirty-tree scan, or source inspection starts
