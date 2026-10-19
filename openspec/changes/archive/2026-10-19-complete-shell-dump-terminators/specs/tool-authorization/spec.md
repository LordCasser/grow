## ADDED Requirements

### Requirement: Shell dump readers require complete terminator lines
Persistent shell dump readers SHALL wait for a complete terminal marker line, including its newline, before completing without EOF. Marker detection SHALL NOT repeatedly rescan the complete accumulated payload. Existing read deadlines and rejection of incomplete snapshots SHALL remain in effect.

#### Scenario: Terminator newline arrives in a later pipe read
- **WHEN** Bash or Zsh dump bytes end at the terminal marker and its newline arrives later while a writer remains open
- **THEN** the reader waits for that newline and returns a parseable complete dump without waiting for the writer to close.

#### Scenario: Large function state travels through the pipe
- **WHEN** a large function snapshot is dumped and replayed through the production state descriptors
- **THEN** marker detection checks only a fixed suffix per chunk, and complete dumps retain function state under the existing read deadline.
