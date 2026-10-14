# Spec Delta

## ADDED Requirements

### Requirement: Board reorder rejects stale or inconsistent positions

The reorder SHALL require `expected_state_version` and serialize with board-order and lifecycle writes before validating the column and anchors. After removing the moved card, only immediately adjacent same-column before/after references are valid; null denotes a column boundary. Invalid anchors or stale versions SHALL be rejected without mutation, and incompatible concurrent snapshots SHALL conflict.

#### Scenario: Stale Requirement version cannot reorder

- **WHEN** a client reorders a card with a stale `expected_state_version`
- **THEN** the server returns a conflict and preserves the current order

#### Scenario: Target from another column is rejected

- **WHEN** a reorder request names a before/after card in a different lifecycle column
- **THEN** the server rejects the request and leaves both columns unchanged

#### Scenario: Concurrent stale reorder is detected

- **WHEN** two requests from one board snapshot choose incompatible positions and one commits first
- **THEN** the later request detects changed anchors or state and returns a conflict instead of silently corrupting order

#### Scenario: Unknown, self, or non-adjacent anchors are rejected

- **WHEN** a reorder names an unknown card, the moved card itself, or non-adjacent anchors
- **THEN** the server rejects the request and preserves the current order
