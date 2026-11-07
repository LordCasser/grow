## ADDED Requirements

### Requirement: After-snapshot capture completes within a fixed budget and never fabricates states

End-of-prompt after-snapshot capture SHALL bound the entire asynchronous read-observation round with one fixed total budget covering queueing and read waiting for every touched path, keeping the existing fan-out. The budget SHALL NOT be described as forcibly interrupting tracker lock acquisition or underlying synchronous I/O. Reads that finish within the budget SHALL be recorded; a read error, a non-UTF-8 body, or an expired budget SHALL leave that path's final state unknown (no after-snapshot entry) instead of recording a fabricated confirmed-absent snapshot. A read that confirms the file is absent SHALL record content `None`. A retried capture for the same prompt SHALL discard that prompt's previous after-snapshot entries before inserting new ones. After the budget expires, nothing SHALL write the prompt's rewind point later, and the implementation SHALL NOT claim the underlying read was terminated. The budget SHALL NOT be user-configurable.

#### Scenario: A never-returning read cannot hold the prompt end

- **WHEN** an after-read for a touched path never resolves while its underlying request sender stays alive
- **THEN** end-of-prompt capture still returns within the fixed budget, earlier successful captures stay recorded, the unresponsive path has no after entry (unknown final state, not confirmed absent), and nothing writes the rewind point afterwards.

#### Scenario: Read error or invalid UTF-8 stays unknown

- **WHEN** an after-read fails, or returns bytes that are not valid UTF-8
- **THEN** no after-snapshot entry is inserted for that path.

#### Scenario: Confirmed absence is recorded as None

- **WHEN** the after-read reports that the file does not exist
- **THEN** the after entry records content `None`, and rewind clean detection may treat current absence as clean.

#### Scenario: Retry discards stale after entries of the same prompt

- **WHEN** capture runs again for a prompt whose earlier capture recorded after entries
- **THEN** those stale entries are discarded first, so neither stale known states nor residue of an interrupted round survive next to the new capture.

### Requirement: Rewind conflict detection preserves unknown after-snapshots

Rewind preview conflict detection SHALL judge each path by the latest rewind point that touches it (before-snapshot membership) and only that point's own after-snapshot entry. A touched path whose latest touching point lacks an after entry SHALL report conflict type `snapshot_unavailable`, SHALL NOT be reported clean, and the preview SHALL name the unknown final state instead of claiming external modification, regardless of whether the file currently exists. An earlier point's after entry SHALL NOT be substituted for the unknown one. A confirmed-deleted after entry (content `None`) with the file currently absent SHALL remain clean. Folding later points into an earlier one SHALL remove the earlier same-path after entry whenever a later touching point lacks its own after entry. Forcing the rewind SHALL keep the existing explicit user-confirmed conflict semantics with no new approval step.

#### Scenario: Latest touching point is unknown

- **WHEN** the latest point touching a path has no after entry while an earlier point records one
- **THEN** the preview reports `snapshot_unavailable` and does not report the path clean, regardless of current file state, and never uses the earlier entry.

#### Scenario: Confirmed deletion stays clean

- **WHEN** the latest touching point recorded content `None` and the file is currently absent
- **THEN** the path is reported clean.

#### Scenario: Preview with unknown conflicts refuses clearly

- **WHEN** a preview encounters `snapshot_unavailable` conflicts
- **THEN** it refuses, names the unknown final states without asserting external modification for them, and mutates nothing.

#### Scenario: Force keeps existing semantics

- **WHEN** the user confirms execution on a rewind with conflicts, including `snapshot_unavailable`
- **THEN** the rewind proceeds under the existing conflict-confirmation flow with no new approval step.

#### Scenario: Merge must not resurface a stale known-after

- **WHEN** later points that touch a path without an own after entry fold into an earlier point
- **THEN** the earlier point's same-path after entry is removed, so the merged point presents the path as unknown.

#### Scenario: Unknown survives persistence

- **WHEN** a rewind point with an unknown after state round-trips through the rewind ledger or the lazy historical load
- **THEN** the state remains unknown with no schema change, and metadata-only scans continue to work unchanged.
