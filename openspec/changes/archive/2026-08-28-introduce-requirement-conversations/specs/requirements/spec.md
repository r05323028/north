## MODIFIED Requirements

### Requirement: Existing Requirement mutations are revision-aware

Every mutation of an existing Requirement SHALL require `expected_revision`,
including structured edits submitted from the conversation surface. Persistence
SHALL compare it atomically with the current row before applying the domain
operation. A stale value SHALL return a conflict (normally HTTP 409) and SHALL
persist no content, status, revision, audit, message, or other side effect.

#### Scenario: Stale caller cannot overwrite newer state

- **WHEN** a client loaded revision 12, another actor committed revision 13, and the client submits `expected_revision = 12`
- **THEN** the mutation returns HTTP 409 and the revision-13 row remains unchanged

#### Scenario: Conversation edit uses the domain contract

- **WHEN** a requester saves structured fields from a conversation detail view
- **THEN** the server applies the same content-edit rules as the direct requirement endpoint

#### Scenario: Stale conversation edit cannot append context

- **WHEN** a conversation edit targets an older revision
- **THEN** the mutation returns HTTP 409 and leaves both structured state and conversation history unchanged
