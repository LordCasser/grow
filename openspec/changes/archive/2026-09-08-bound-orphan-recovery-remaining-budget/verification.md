## Verification
`cargo test --locked --offline -p client-support --lib placeholder --quiet`: 51 passed, 0 failed (0.01s). git diff --check passed.

The recovery loop computes remaining before canonicalizing/loading a new candidate, exits at zero, and passes min(per_image_max, remaining) to the already bounded loader. Source-level call wiring plus the prior counted-read tests establish the actual read cap; this change adds no alternate unbounded reader.

New test uses an oversized malformed .png before a valid small PNG. With remaining tighter than per-image budget it now stops before MIME validation rather than reading/classifying the large file and recovering the later image. This intentionally verifies early resource admission semantics. A separate test verifies that per-image rejection continues when that cap is less than or equal to remaining, and the later image preserves display number2. Existing exact aggregate boundary, over-cap stopping, URI dedup, raw attachment preservation, path authorization and loader byte-budget cases remain green.

Aggregate still counts only bytes newly recovered by this call, excluding preexisting attachments. It is not a whole-request/base64-memory budget. No same-path/different-display-number dedup change was made because content dedup alone would lose reference metadata. No real user image or clipboard content was read in testing.
