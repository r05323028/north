# Spec Delta

## Purpose

Lets each North page offer its own personal preference editor and access policy while keeping saved values private to the signed-in user and separate from shared instance configuration.

## ADDED Requirements

### Requirement: Personal preferences belong only to the signed-in user

The server SHALL associate each preference set with the authenticated user's persisted identity and a page key. Read and update operations SHALL derive the owner from the active session; clients SHALL NOT choose or override the owner. Admin or Owner role SHALL NOT grant access to another user's personal preferences. Unauthenticated requests SHALL return HTTP 401, and attempts to access another user's preference set SHALL be refused without changing data.

#### Scenario: User reads own preferences
- **WHEN** an authenticated user reads preferences for a supported page
- **THEN** the server returns only that user's saved values, or that page's documented defaults when no values exist

#### Scenario: Administrator cannot read another user's preferences
- **WHEN** an Admin or Owner attempts to read or update another user's personal preferences
- **THEN** the server refuses the request and leaves the target preferences unchanged

#### Scenario: Unauthenticated access is refused
- **WHEN** a request without a valid session reads or updates preferences
- **THEN** the server returns HTTP 401

### Requirement: Each page owns its preference editor and permission policy

Each page that supports personal preferences SHALL define its own supported fields, editor behavior, and read/update authorization policy. The server SHALL enforce that page's policy on both reads and updates. A preference policy SHALL NOT grant access to the page's underlying business data or change another page's authorization. For the Requirement workspace in this change, any authenticated role MAY manage that user's own preferences.

#### Scenario: Page editor saves only its declared fields
- **WHEN** a user saves a preference using a page-specific editor
- **THEN** only fields supported by that page are accepted; unsupported fields are refused without partially changing saved values

#### Scenario: Page permission remains authoritative
- **WHEN** a user lacks access to a page that has a preference editor
- **THEN** reading or updating that page's preferences is refused under that page's authorization policy

#### Scenario: Workspace preferences do not change Requirement permissions
- **WHEN** a user changes a Requirement workspace preference
- **THEN** the preference changes only presentation defaults and does not grant or remove permission to view or mutate Requirements

### Requirement: Requirement workspace stores explicit personal display defaults

The Requirement workspace SHALL persist per-user defaults for board/list view and List status, creator, and updated/updated_asc sort; search text SHALL remain transient.

#### Scenario: Missing preferences use workspace defaults
- **WHEN** a user opens the Requirement workspace with no saved preferences
- **THEN** the workspace opens in board view and the List defaults to all statuses, all creators, and updated sort

#### Scenario: Saved display defaults return after reload
- **WHEN** a user saves List view with a status or creator filter and sort, then reloads the workspace
- **THEN** the saved view and List defaults are restored and canonical Requirement results use those filters and sort

#### Scenario: Search text remains transient
- **WHEN** a user searches for Requirements and reloads the workspace
- **THEN** the search text is not restored from personal preferences

#### Scenario: Invalid preference values do not partially save
- **WHEN** a user submits an unsupported view, status, sort, or field
- **THEN** the update is refused and the prior saved preferences remain unchanged

### Requirement: Personal preference updates are isolated

Saving preferences for one page SHALL NOT alter preferences for another page or any shared instance configuration. Updates SHALL become canonical only after successful persistence; a failed update SHALL leave the previously saved values available.

#### Scenario: User changes one page's preferences
- **WHEN** a user saves preferences for one supported page
- **THEN** preferences for other pages and shared settings remain unchanged

#### Scenario: Failed save preserves prior preferences
- **WHEN** persistence rejects a preference update
- **THEN** the editor reports the save failure and a subsequent read returns the previously saved values

#### Scenario: Stale preference responses do not replace local edits
- **WHEN** a preference load or save response arrives after newer local edits
- **THEN** the editor keeps the newer values and does not report those values as saved by the older request

#### Scenario: Conflicting saves do not overlap
- **WHEN** a preference save is pending and another save is requested
- **THEN** the editor permits only the pending save and keeps subsequent edits local until a later save
