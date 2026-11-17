## ADDED Requirements

### Requirement: Selected direct subagents retain their terminal state during paged resume

Pager SHALL use the matching canonical terminal at the selected history cut to settle any unfinished task row for a direct child selected by descendant restoration. The task state SHALL be independent of whether the transcript page containing its terminal has arrived. A synthesized lifecycle update for this purpose SHALL NOT add a transcript block, advance the history cursor, or dispatch child work. An already displayed terminal for the same child SHALL be preserved.

#### Scenario: A selected child ended outside the loaded transcript page
- **WHEN** paged resume selects a direct child and its matching canonical terminal, while no terminal transcript row has been loaded
- **THEN** the task row retains the recorded outcome and elapsed time, the child is idle, and the task does not expose cancellation or make the root wait for a running child.

#### Scenario: Restore overlaps a terminal or an open child
- **WHEN** a selected child already has a terminal in the view, or a different selected child has only a canonical spawn
- **THEN** restoration keeps the existing terminal and leaves the open child active without creating additional transcript lifecycle rows.
