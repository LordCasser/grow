## ADDED Requirements

### Requirement: Hidden announcement state has bounded IO admission
Hidden announcement state loading SHALL accept only ordinary files containing at most1,048,576 encoded bytes and SHALL check both metadata and actual bytes read. Rejected input SHALL leave all announcements visible without modifying the source. Persistence SHALL reject snapshots exceeding the same limit before replacing existing state.

#### Scenario: Oversized or special source
- **WHEN** the state source exceeds the byte limit or is not an ordinary file
- **THEN** loading yields no hidden IDs without modifying the source; Unix FIFO admission does not wait for a writer

#### Scenario: Source exceeds its observed size
- **WHEN** actual bytes read exceed the allowance despite an earlier acceptable metadata observation
- **THEN** the bounded reader rejects before parsing more than allowance plus one bytes

#### Scenario: Oversized snapshot write
- **WHEN** an encoded hidden-state snapshot exceeds the allowance
- **THEN** persistence returns an error without replacing the prior committed state
