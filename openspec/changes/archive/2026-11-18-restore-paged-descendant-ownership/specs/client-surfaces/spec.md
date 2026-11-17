## ADDED Requirements

### Requirement: History pages restore exact subagent navigation and terminal state

Each adopted history page SHALL supply the child ownership required by the subagent rows in that page from the same validated cut, including exact ancestors for nested children. A view SHALL bind these facts to the requesting session, load generation and cut. Page adoption SHALL preserve the canonical direct parent and lifecycle-root coordinates, terminal state, and availability of the corresponding child view. A confirmed pre-launch failure SHALL display its outcome independently of Result availability. Child history traversal SHALL verify its Seed and any required Result before use. The existing optional 1 MiB load metadata budget SHALL NOT suppress ownership selected by the page.

#### Scenario: An archived child appears in a page
- **WHEN** an adopted Latest, Before, After or Around page names a child whose spawn is outside the loaded transcript or working set
- **THEN** the exact child can be inspected, its terminal state comes from the same cut, and no additional lifecycle row is inserted.

#### Scenario: The optional load payload is too large
- **WHEN** the 1 MiB budget omits optional load ownership and the client adopts a page containing a child
- **THEN** the child and its required ancestors are still resolved against that page's cut.

#### Scenario: A nested child lacks terminal presentation
- **WHEN** a selected nested child has a matching Ended in the lifecycle root and a valid direct parent edge
- **THEN** the task keeps the canonical terminal, even without a Finished presentation row, and does not offer cancellation or obstruct the root as a running child.

#### Scenario: Startup fails before child storage exists
- **WHEN** the root records an exact terminal for a selected delegation without a child result or a child session
- **THEN** the task displays the recorded outcome and no live cancellation or child detail is offered for the unavailable session.
