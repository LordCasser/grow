# Design: Sequence session picker list fetches

## Decision

Use the existing `session_picker_list_seq` as the single generation for plain and query list fetches. Increment before each fetch and on every dismissal. The effect echoes the captured sequence in its result; handlers reject stale sequences. A current result may update the active Agent modal, or the welcome picker when the welcome screen is active. If neither surface is visible, it is discarded. Keep the FTS deep-search sequence separate.

This change makes the existing sequence comment true and removes tests that explicitly preserved arrival-order overwrites. It does not add a new registry or attempt to solve the independent cwd/owner-identity mismatch.
