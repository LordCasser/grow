## ADDED Requirements

### Requirement: Live subagent launch does not reread parent history

Pager SHALL establish the live child route, task prompt, effective workspace and worktree state from the published canonical spawn details without reading the parent Timeline on its event loop. A newly published child without inherited display history SHALL accept its first live input without opening child history. Child control publication and first-prompt admission retain their existing order.

#### Scenario: Ordinary child launches in a long conversation
- **WHEN** Shell publishes a new child or a context fork with no inherited display history before the first child prompt
- **THEN** Pager creates the child view with its published task and workspace, without a synchronous parent or child history read.

#### Scenario: Derived or historical child has earlier presentation
- **WHEN** a derived child inherits a transcript or Pager is reconstructing an existing child
- **THEN** its previous presentation remains available in order, and an omitted live detail does not classify historical child state as an empty transcript.
