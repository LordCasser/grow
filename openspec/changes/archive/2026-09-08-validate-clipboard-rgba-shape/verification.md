## Verification
Moved the existing encoder out of the nonmacOS platform module under cfg(any(test, not(macos))), keeping the production platform boundary and running pure tests on this host. Existing installed arboard3.6.1 ImageData declares usize width/height; caller now passes those without narrowing. Installed image0.25.10 PNG encoder asserts exact input length.

Red run with the original encoder: 3 passed, 1 failed. A 1x1 image with five bytes triggered PngEncoder's expected4/got5 assertion, observed through catch_unwind. Fixed implementation checks nonzero dimensions, fallible u32 conversions, checked width*height*4, and exact buffer length before output allocation/PNG call.

`cargo test --locked --offline -p client-support --lib rgba_encoding --quiet`: 5 passed. Includes short/long/zero/overflow inputs, unrepresentable dimensions without truncation, and a valid PNG decoded back to its original RGBA pixel.
`cargo test --locked --offline -p client-support --lib clipboard --quiet`: 59 passed, 4 ignored, 0 failed (1.40s). Real clipboard tests remained ignored. git diff --check and strict validation passed.

The nonmacOS arboard runtime was not executed or cross-built on this macOS host. Its field types/call site were checked against local sources; shared encoding implementation and tests ran locally. No claim of an observed real clipboard producing invalid dimensions; the encoder rejection/panic defect was reproduced directly. This change does not bound the backend's initial allocation or valid huge image encoding memory.
