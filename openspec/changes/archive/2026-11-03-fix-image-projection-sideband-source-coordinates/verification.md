## Verification

- Red: temporarily restored the prior shadows-only ownership predicate and ran `cargo test --locked -p chat-state --lib timeline::tests::image_projection_scrubs_response_carrier_paths_from_compaction_summary -- --exact`; it failed at the tool-call coordinate assertion (`item: 1`). The production fix was then restored.
- Green: `cargo test --locked -p chat-state --lib timeline::tests::image_projection_scrubs_response_carrier_paths_from_compaction_summary -- --exact` — passed (1 test), covering shadow, tool-call and carrier slots, the upper bound, and replay.
- Green: `cargo test --locked -p chat-state --lib timeline::tests::canonical_surface_coordinate_producers_are_complete -- --exact` — passed (1 test), retaining the shadows-only boundary check.
- `openspec validate fix-image-projection-sideband-source-coordinates --strict --no-interactive` — valid.

主任务的真实文件回归 `image_input_recovery_tests::auxiliary::memory_flush_keeps_pre_compaction_sources_and_writes_durable_text` 已通过：工具图片投影后的 frozen source IDs、revision 与 input range 保留，JsonlStorageAdapter 的 Sideband ledger 校验和 session 重载均成功。相关开发者说明已同步到 `docs/development.md`。

本 change 已独立归档；与 recap/flush change 一并核对的归档后全量 strict 校验为 19 passed、0 failed，最终 archive 校验在主 change 的 verification 中记录。
