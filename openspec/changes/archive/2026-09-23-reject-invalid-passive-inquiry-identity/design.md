## Design

The receiving notification handler is the boundary that still has the original `UiNotice`; when its structured audit is invalid or `correlation_id` is empty, it should append the raw notice and stop before constructing a `CoordinationRow`. This preserves the observed fact and avoids inventing a dedup key.

`ScrollbackState::upsert_coordination_row` is an internal coalescing primitive. It should accept only `Some(CoordinationRow)` with non-empty `source_peer_id` and `inquiry_id`; invalid input returns `false` without changing the scrollback. The production notice path owns the visible fallback. This prevents an invalid identity from entering running state or colliding with another malformed row.

Valid row lifecycle stays as currently projected. `finish_all_running` already excludes passive rows, and tests assert foreground completion cannot stop one; replacing the generic renderer running state is outside this identity-safety fix.
