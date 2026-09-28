# Spec Delta

## ADDED Requirements

### Requirement: Board reorder rejects stale or inconsistent positions

The board reorder operation SHALL require the moved Requirement's `expected_state_version` and SHALL serialize with other board-order and lifecycle writes before validating its current column and target neighbors. The target references SHALL identify the immediately preceding and following cards in that same column after the moved card is removed; null references SHALL denote the corresponding column boundary. Unknown references, the moved card as its own neighbor, cross-column references, non-adjacent anchors, or a stale state version SHALL be rejected without changing order. Concurrent requests based on incompatible snapshots SHALL not silently overwrite one another.

#### Scenario: Stale Requirement version cannot reorder

- **WHEN** a client reorders a card with a stale `expected_state_version`
- **THEN** the server returns a conflict and preserves the current order

#### Scenario: Target from another column is rejected

- **WHEN** a reorder request names a before/after card in a different lifecycle column
- **THEN** the server rejects the request and leaves both columns unchanged

#### Scenario: Concurrent stale reorder is detected

- **WHEN** two requests from one board snapshot choose incompatible positions and one commits first
- **THEN** the later request detects changed anchors or state and returns a conflict instead of silently corrupting order
