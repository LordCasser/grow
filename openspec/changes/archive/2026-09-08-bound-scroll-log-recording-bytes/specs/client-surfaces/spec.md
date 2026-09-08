## ADDED Requirements

### Requirement: Scroll recording bounds each capture by complete lines
Each scroll recorder SHALL accept at most67,108,864 encoded bytes including newline separators. It SHALL check capacity before each line, preserve previously accepted complete lines, flush buffered content when stopping for the limit, and report inactive without automatic rotation.

#### Scenario: Exact limit or next line exceeds capacity
- **WHEN** a line exactly exhausts the allowance or the next complete line would exceed it
- **THEN** recording stops at a complete-line boundary and previously accepted buffered lines are flushed

#### Scenario: First line cannot fit
- **WHEN** the first line exceeds the allowance
- **THEN** disable before opening or truncating the target

#### Scenario: Restart recording
- **WHEN** recording is toggled on again after its budget was exhausted
- **THEN** the newly constructed recorder receives a fresh allowance without removing prior capture files
