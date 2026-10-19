# Verification

Validated with Rust 1.93.1 on macOS arm64.

- Shell: 4,266 passed, 5 explicitly ignored. Pager: 7,517 passed, 12 explicitly ignored.
- Actual provider-body assertions retain images and the fixed English recognition prompt while excluding history, source-message text, Skill/tool context and tools. Recognition output keeps the image's original language.
- Recovery covers completed Sideband reuse, obsolete-prompt regeneration, acknowledged description/OCR projection, cancellation and cold restore. The legacy-prompt fixture now queues only responses the interrupted original turn could consume and selects the Sideband referenced by the committed projection.
- UI coverage includes the stable animated row, completion/partial/failure, selection/export, RPC-first and durable-first exact-prompt terminal settlement, stale terminal rejection, active/stale reconnect, snapshot completion and real replay-player EOF.
- Fixed runtime display strings in the affected vision paths are English. Historical payloads and user/model/source content are preserved. New recovery emits no NOTICE progress; legacy readers remain for historical replay.
- Formatting, diff whitespace and strict OpenSpec validation passed. See ../2026-10-19-enforce-release-regression-gates/verification.md.
