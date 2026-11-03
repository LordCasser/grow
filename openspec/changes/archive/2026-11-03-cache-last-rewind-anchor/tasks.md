## Implementation and verification

- [x] Add the derived Timeline rewind anchor and update it only in the shared validated apply path.
- [x] Use the anchor in prompt cache key generation without changing key bytes.
- [x] Add live/replay/failed-accept equivalence coverage and compare the old scan with the indexed key path.
- [x] Run targeted chat-state tests and the full chat-state library tests with isolated `GROW_HOME` and `RUST_MIN_STACK=16777216`.
- [x] Record actual benchmark samples and validate the OpenSpec change.
