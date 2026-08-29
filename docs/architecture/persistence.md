# Persistence

Relational database, single source of truth for durable product and server
reliability state. Migrations: versioned SQL in `../migrations/`, applied by the
server at startup.

## Durable vs ephemeral

| Class | Examples | Rules |
| --- | --- | --- |
| Durable business | users, roles, requirements (+revisions), readiness assessments, conversations, messages, configured repositories, human review decisions | never TTL-deleted; deletion is a product decision |
| Durable coordination | daemon registrations, `session.daemon_id`, session execution state/attempts, server command outbox, event dedupe/rejection records, sequence watermarks | transactionally maintained; command payloads may be compacted only at the protocol's acknowledged sequence boundary |

Migrations 0003–0005 implement requirements and transition audit,
one-to-one conversations/messages, and immutable revision-bound readiness
evidence. Migration 0010 adds positive `requirements.state_version` with a
backfill of 1 for existing rows. Migration 0011 records the accepted assessment
Ready-generation identity, marks unverifiable legacy generations as unknown, and
replaces the evidence foreign-key cascade with restrictive deletion.
`accepted_state_version` equals `requirements.state_version` only while that
Requirement remains Ready and reviewable; later review transitions advance the
Requirement without mutating historical evidence. Migrations 0007–0009
implement daemon registration/setup-request, execution-session/outbox records,
and requirement binding used to authorize assessment events. Migration 0013
adds the configured repository catalog. Migration 0014 adds immutable outbox
payload fingerprints, command/event contiguous watermarks, and durable server
event identity/outcome records. Readiness
evidence rows are append-only; database triggers reject direct mutation of
evidence, repository source identity, and command outbox payloads. Requirement
delete is restrictive so evidence never changes via a cascade. Requirements
with readiness evidence must be retained (or receive a future tombstone design).
Registration rows retain hashed credentials, owner identity,
protocol/capability metadata, connection liveness, and revocation timestamps.
The server updates liveness only for the authenticated connection identity;
status is Live only with a heartbeat from the last 45 seconds and an active
connection marker. Revocation clears live access without changing session owner.
Admin/Owner users can list all registrations; other users see only daemons they
created. Verification codes commit failed-attempt counts under row lock and are
consumed after five failures. Setup rows older than 24 hours are removed in
bounded batches using an expiry index when setup requests are created or polled.

| Ephemeral (TTL) | runtime events, tool activity, transient execution logs | GC'd by a boring TTL job; expiry must never invalidate a Requirement |

The daemon also keeps a local transport journal for command inbox and event
replay. That journal is not server business state, not `north-persistence`, and
not a second database authority. Its command `terminal` state records the local
processing/dispatch outcome of one command, not execution-session completion;
`session.completed`/`session.failed` events report session outcome separately.
Its processed-command high-water tombstone is retained for the durable session
and is not expired by time alone in 0.1.0.

Invariants:

- Ephemeral runtime data is never the sole source of truth for requirement or
  execution policy state.
- TTL/GC deletes only ephemeral records; retention window is configuration.
- Server command outbox rows are inserted before dispatch and remain eligible
  for resend until `command_ack` is durably recorded. The immutable complete
  envelope, payload digest, ACK processor, contiguous watermark, and ascending
  reconnect resend are server-owned persistence behavior. The daemon's local
  Journal retains command/event identity and replay state outside this database.
- `revision` identifies canonical structured content; `state_version` identifies
  mutable Requirement state. Existing-row mutations atomically compare
  `expected_state_version`; real mutations increment state_version once, while
  content edits increment revision too. Stale callers receive a conflict with
  no side effects.
- `requirement.assessed` validates event identity, session binding, directional
  sequence identity, and `requirement_revision` against the current Requirement
  revision before running readiness gates. Accepted evidence creates/binds
  `assessment_id` and records the resulting Ready-generation
  `accepted_state_version`; neither is an inbound assessment concurrency token.
  Dedupe, repository citation existence/provenance, domain gates, immutable
  evidence, successful state-version lifecycle transition, and resulting row
  update share one transaction. Event identity or sequence conflicts are
  protocol errors without an ACK; well-formed invalid/stale facts commit a
  durable rejection record before `event_ack(status=rejected)`. Review actions
  use `assessment_id`, `expected_state_version`, and the exact current Ready
  generation.

## First-owner bootstrap

On a fresh instance, `instance_settings` has one singleton row (`id = 1`)
with nullable `owner_user_id`. Verification inserts a new user as Requester,
then claims that row in the same transaction with:

```sql
UPDATE instance_settings
SET owner_user_id = $user_id
WHERE id = 1 AND owner_user_id IS NULL;
```

A transaction that updates one row wins the claim and promotes its user to
Owner; concurrent losers keep Requester. The verification-code consumption,
user insert, owner claim, and session insert all commit or roll back together.
No application-level "check then insert" races.

## Repository identity

Configured repositories contain metadata only: immutable UUID `id`, trimmed
editable `name`, persistence-only normalized name key, immutable-after-create
URL, editable `description`, timestamps, and nullable `disabled_at`. The
normalized name is unique across enabled and disabled rows. Server validation
bounds/trims metadata and rejects HTTPS userinfo, URL passwords, and SSH/SCP
users other than literal `git`; North 0.1 intentionally chooses this standard
literal-`git` username policy. Daemon-host Git configuration remains the
credential boundary.

Create sets `created_at = now`, `updated_at = now`, and `disabled_at = null`.
Metadata changes advance `updated_at`; `created_at` never changes. Disabling an
enabled row sets `disabled_at = now` and advances `updated_at`. Disabling an
already-disabled row is an idempotent no-op that leaves both timestamps
unchanged. Re-enable clears `disabled_at` on the same identity and advances
`updated_at`; re-enable of an already-enabled row is an idempotent no-op with
`updated_at` unchanged.

Normal Remove always soft-disables, including an unreferenced row, and never
hard-deletes it. Management reads include enabled and disabled rows for
Admin/Owner. The active catalog is an internal server/persistence read for
server-assembled session context and downstream inspection, includes only
`disabled_at IS NULL`, and is not an independent daemon catalog endpoint.
Both reads use `name_normalized ASC, id ASC` ordering.

Readiness evidence retains `repository_id` and exact full commit SHA, and every
cited ID must resolve to an existing durable row before accepted evidence or
promotion. Unknown IDs are durably rejected without fabricating a repository.
A citation from an in-flight run remains eligible after disable when the row is
retained and the ID/SHA was valid for the existing session context or authorized
inspection result; only new inspection selection requires enabled state. URL
replacement requires disable-old/create-new to keep repository identity stable.
Historical UI uses retained current metadata; name/description snapshots are not
claimed, and disabling/re-enabling does not alter prior evidence.

## Ownership mapping

- Domain types live in `north-domain`; row↔domain mapping lives in
  `north-persistence` (it may depend on domain for that mapping, on nothing else).
- Hosts never hand-roll SQL outside persistence.
- Repository credentials never enter server persistence; daemon host Git
  configuration remains the credential boundary.
