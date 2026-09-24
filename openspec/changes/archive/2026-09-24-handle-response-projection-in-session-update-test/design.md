# Design

The test's reverse `find_map` scans session updates for the latest token metadata. Add an explicit `ResponseReplayProjection(_) => None` match arm. Returning `None` lets the scan continue to the earlier ACP or Grow notification that carries `totalTokens`.

No production code or session update semantics change.
