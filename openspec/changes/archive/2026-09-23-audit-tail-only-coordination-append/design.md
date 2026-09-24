## Design

`merge_coordination_rows_from_tail` only removes a tail row when it finds an existing row with the same `(source_peer_id, inquiry_id)`. A tail-only row remains in `tail.entries`; `append_entries_from` then extends the original entries with it and merges the tail's running set and minimal commit set. The audit records those facts without changing the production merge rule or adding a code test.
