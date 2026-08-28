# Design

## Context

Repository rows are referenced by readiness evidence. A hard delete would make
old evidence depend on a missing catalog row, while changing one durable row to
an unrelated URL would make `repository_id + commit_sha` ambiguous. The
configured-repositories change therefore owns metadata and lifecycle only.
Git access, caches, checkouts, dirty detection, and SHA resolution belong to
`introduce-local-repository-inspection`.

## Decisions

### Repository data model

The durable `repositories` row contains:

```text
id             immutable UUID generated at creation
name           trimmed display name; editable
name_normalized
               persistence-only normalized name key; maintained from name
url            trimmed supported Git location; immutable after creation in 0.1.0
description    trimmed display text; editable and may be empty
created_at     server-generated immutable UTC timestamp
updated_at     server-generated UTC timestamp for meaningful changes
disabled_at    nullable server-generated UTC timestamp
```

`name_normalized` is not a separate user identity. It exists so the database
can enforce the same case-insensitive uniqueness rule used by the API. The
migration has no credential, token, password, secret, SSH-key, or
credential-helper column. Row deletion is not part of the normal 0.1.0
management API; the retained row is the historical identity.

### Normalization and validation

Validation happens at the server boundary before persistence and is repeated by
domain/persistence tests. Database uniqueness is the final concurrency guard.

- `name`: trim surrounding whitespace, reject empty, limit to 100 UTF-8 bytes,
  and store the trimmed value. Compute `name_normalized` by Unicode lowercase
  of the trimmed value using the repository's non-locale normalization. The
  unique constraint is on `name_normalized`, including disabled rows.
- `description`: trim surrounding whitespace, allow empty, limit to 10,000
  UTF-8 bytes, and store the trimmed value.
- `url`: trim surrounding whitespace, reject empty, limit to 2,048 UTF-8
  bytes, and validate one supported Git location shape. Store the trimmed
  value exactly; URL immutability means no later canonicalization may change
  its source identity.

Supported 0.1 URL shapes are:

```text
https://<host>/<non-empty path>
ssh://[git@]<host>/<non-empty path>
git@<host>:<non-empty path>
```

HTTPS URLs must not contain any userinfo. SSH URLs may omit userinfo or use
`git@`; an SSH password is never accepted and an SSH user other than `git` is
not a configured-repository credential boundary in 0.1. SCP-style URLs must
use the normal `git@host:path` identity. Host and path must be non-empty. Other
schemes and malformed locations are rejected. This validation is metadata
validation only; the inspection change later determines whether host Git can
access the location.

### Credential-free URL boundary

No credential material is accepted from the URL or any API field. In
particular, reject:

```text
https://user:password@example.com/repo.git
https://token@example.com/org/repo.git
```

Reject any HTTPS userinfo, any URL password, and any SSH/SCP user other than
the literal `git`. Accept these normal SSH identity forms because `git` is a
transport username, not a North-stored secret:

```text
git@github.com:org/repo.git
ssh://git@github.com/org/repo.git
```

The server stores only the validated location string. Daemon-host Git config,
SSH agent/configuration, credential helpers, and file permissions remain the
host credential boundary and never enter North persistence or wire DTOs.

### URL immutability and historical identity

`url` is immutable after creation in 0.1.0. An update request containing a
different URL fails with an immutable-field conflict and makes no mutation.
An Admin replacing a source disables the old row and creates a new row; the old
identity remains tied to prior evidence. If the same display name is required,
the old row must first be renamed or the new row must use another normalized
name because uniqueness includes disabled rows.

`id` is immutable. `name` and `description` may change on enabled or disabled
rows, subject to validation and normalized-name uniqueness. `created_at` never
changes. `updated_at` changes on a successful metadata or lifecycle change.
`disabled_at` is set by disable and cleared by re-enable.

Historical evidence stores `repository_id` and exact full `commit_sha`. The
retained row's current name, description, and immutable URL remain available
for historical joins. Historical UI in 0.1 displays retained current metadata;
the repository ID and commit SHA remain the authoritative evidence identity.
No active-catalog status is required to interpret old evidence.

### Authorization and lifecycle

Admin and Owner are the only roles allowed to create, edit metadata, disable,
re-enable, or read the management list. Requester and Requirement Manager are
rejected server-side with the existing forbidden response semantics before any
mutation. This change grants no management permission through an inspection or
session-context endpoint.

Normal Remove always means soft-disable, regardless of whether readiness
evidence references the row. For an enabled row it sets `disabled_at` to the
server's current UTC time. Repeated Remove on an already disabled row is an
idempotent success that preserves its original `disabled_at`; it never deletes
the row. North 0.1 exposes no hard-delete route, command, or Settings button.

Re-enable is an explicit Admin/Owner operation on the retained row. It clears
`disabled_at`, keeps the same `id`, name, URL, description, and history, and
returns the row to the active catalog. It never inserts a replacement row.

### Management list and active catalog

These are distinct reads with distinct authorization and purpose:

- **Management list:** an Admin/Owner Settings read containing every enabled
  and disabled row, with `disabled_at` and current metadata so status and
  re-enable are possible.
- **Active catalog:** an enabled-only server read for new session context and
  downstream inspection candidates, filtered by `disabled_at IS NULL`. A
  disabled row is never selected for a new inspection.

Both lists sort by `name_normalized ASC, id ASC` for deterministic API, UI, and
test behavior. The active catalog is not a substitute for the management list.
If a non-admin surface needs repository metadata indirectly, that surface keeps
its existing authorization and defines its own read contract; it does not gain
repository-management access through this change.

### Create conflicts and concurrency

The normalized-name unique constraint covers enabled and disabled rows. Create
behavior is explicit:

- enabled normalized-name match → HTTP conflict; no new row;
- disabled normalized-name match → HTTP conflict identifying the retained row
  and instructing the caller to re-enable it; no new row.

Concurrent creates rely on the database constraint and map its violation to
the same conflict response. Update name uses the same constraint. URL changes
are rejected before persistence. Lifecycle operations are transactional and
must not partially update metadata, timestamps, or `disabled_at`.

### Settings UI

Settings shows the Admin/Owner management list, displays enabled/disabled
status, offers create and metadata edit, uses disable for Remove, and offers
re-enable for disabled rows. It has no hard-delete affordance. Frontend checks
are usability only; the server validation and authorization rules remain
canonical. UI behavior does not add Git access or credentials.

### Downstream boundary

`introduce-local-repository-inspection` consumes the active catalog and
rechecks that a selected repository is enabled before starting work. It owns:

- host `git` invocation and authentication environment;
- reusable per-repository cache and fetch/clone behavior;
- unique disposable session/task checkouts;
- concurrency isolation, dirty-tree detection, and disposal;
- full commit SHA resolution and inspection reporting.

This change owns none of those implementation decisions. The protocol change
may carry enabled metadata in server-assembled `session.start` context, but it
does not move repository authority to the daemon.

## Risks / Trade-offs

- **URL replacement requires a new identity** → immutable URL keeps historical
  repository evidence unambiguous; disable/re-enable preserves old identity.
- **Disabled rows accumulate** → retention is deliberate history. Any purge
  needs a future evidence-snapshot and retention decision, not silent delete.
- **Stored URL may be inaccessible** → catalog validation checks shape only;
  host Git access and its errors belong to inspection.
- **Unicode display names need stable uniqueness** → a stored normalized key and
  database constraint avoid locale-dependent duplicate races.
- **Current metadata is not a historical snapshot** → 0.1 treats ID and full
  commit SHA as immutable evidence, while retained current metadata remains
  human-readable.
