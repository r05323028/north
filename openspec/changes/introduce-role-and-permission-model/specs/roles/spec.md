## Purpose

Defines who may do what across North: four ordered instance roles whose
permission edges are enforced centrally and survive every future endpoint.

## ADDED Requirements

### Requirement: Single persistent role per user

Every user SHALL have exactly one role: Owner, Admin, Requirement Manager, or
Requester. Newly created accounts (after the first-owner exception) SHALL be
Requester. The role SHALL be stored durably with the user record.

#### Scenario: New accounts default low

- **WHEN** any post-bootstrap account is created
- **THEN** its persisted role is Requester

### Requirement: Review decisions are reviewer-gated

Accepting, rejecting, requesting changes on, and reopening requirements SHALL
be permitted only for Requirement Manager, Admin, and Owner, enforced
server-side on every transition path.

#### Scenario: Requester cannot review

- **WHEN** a Requester calls a review transition on a Ready requirement
- **THEN** the request fails with a permission error and state is unchanged

### Requirement: Administration is admin-gated

Repository configuration, daemon settings, instance settings, and user
management SHALL require Admin or Owner, enforced server-side.

#### Scenario: Manager blocked from settings

- **WHEN** a Requirement Manager attempts repository CRUD
- **THEN** the request is refused regardless of UI affordances

### Requirement: Assignment rules prevent escalation

Role changes SHALL follow: only Owner/Admin assign; Admin cannot grant Owner;
nobody modifies their own role. All three rules SHALL hold at the API boundary
(reusing the domain's `assign_role` semantics).

#### Scenario: Self-promotion is impossible

- **WHEN** an Admin targets their own account for any role change
- **THEN** the assignment fails with a self-modification error

#### Scenario: Admin cannot mint Owners

- **WHEN** an Admin assigns Owner to another user
- **THEN** the assignment fails; only Owner can grant Owner

### Requirement: UI reflects but never replaces authorization

The frontend MAY hide actions by role, but every enforcement point SHALL be
server-side; client-side hiding alone SHALL NOT be relied upon anywhere.

#### Scenario: Forged request still refused

- **WHEN** a crafted client bypasses hidden UI and calls a forbidden endpoint
- **THEN** the server rejects it on role grounds alone
