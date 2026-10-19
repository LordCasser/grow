# Image fallback routing audit

## Scope and evidence

本审计覆盖直接 user/read_file/PDF 图片、历史图片、模型切换、辅助视觉配置解析与降级、持久化投影，以及 session-scoped negative image-input capability 如何绑定 primary 与 auxiliary transport。图片本体、fallback 算法和 ImageProjection 生命周期不在本 change 中重做。

主实施路径已在干净 HEAD snapshot 确认两条可复现故障：

1. **同一 primary runtime 换 alias 后丢失拒绝。** baseline 回归在两个 alias 指向同一实际 transport 时失败，第二个 alias 未共享拒绝事实。静态核对同时确认反向风险：同 alias 更换 model/backend/endpoint/query 仍读取旧拒绝。
2. **Auxiliary 同 alias 更换 endpoint/backend 后仍被跳过。** Auxiliary alias 对应的旧拒绝状态未随解析后的 transport 改变而失效；新视觉 runtime 被误判为已知拒绝，无法获准入。

反向身份错误也由同一根因确定：两个 alias 解析为同一 runtime 时，alias 字符串不同，不能共享该 runtime 的拒绝事实。

实现方案使用已有 `ModelImageInputKey`（provider-facing model、backend、endpoint/query fingerprint）生成标准 JSON storage key，复用现有字符串 resource 集合。旧 alias 字符串自然不会命中新 key；不迁移、不删除 resource 中其他值。用户可见标签仍使用 catalog alias。

## Existing coverage

下列条目表示测试源码中已有覆盖点，不代表本 change 已运行这些测试。

| Area | Existing test | What it covers | Limit |
| --- | --- | --- | --- |
| Direct user image and successful visual auxiliary | `crates/codegen/shell/src/session/actor/tests/image_input_recovery_tests.rs::active_goal_image_400_uses_auxiliary_description_then_retries_without_images` | Primary sends original image; configured visual route describes it; projection retains image and description; rebuilt primary request carries text without image; Goal usage remains complete. It then switches to another provider and checks original-image eligibility and same-actor description reuse. | Existing route/helper uses one mock host and Messages backend; it does not exercise auxiliary alias resolution after endpoint/backend/query changes. |
| Direct user image without configured auxiliary | `.../image_input_recovery_tests.rs::explicit_multimodal_400_removes_images_and_resubmits_text_only` | Existing fallback reaches a text-only projection and later text request does not resend the removed image. | Does not deterministically establish a successful local OCR projection. |
| Auxiliary explicitly rejects image | `.../image_input_recovery_tests.rs::auxiliary_image_400_installs_a_durable_removal_projection` | Primary and auxiliary receive 400; capability is recorded; final removal projection permits a no-image primary retry. | Does not cover ordinary auxiliary transport failure, timeout, empty output or sideband persistence failure. The local OCR result is not injected. |
| Projection acknowledgement failure | `.../image_input_recovery_tests.rs::failed_image_projection_commit_never_resubmits_a_lossy_request` | Failed durable ImageProjection prevents retry and preserves the image on the durable Surface. | This is the final ImageProjection gate, not failure to persist the image-description Sideband. |
| Mixed groups | `.../image_input_recovery_tests.rs::mixed_description_and_removal_groups_commit_as_one_projection` | Description and unresolved removal are committed in one projection. | Does not vary auxiliary transport identity. |
| Historical/tool images and current versus switched route | `.../image_input_recovery_tests.rs::current_chat_text_only_type_rejection_converts_new_tool_images`, `switched_chat_text_only_type_rejection_converts_historical_tool_images`, and `assert_chat_tool_image_fallback` | Exercises actual Chinese text-only rejection with a tool-result image. The current-route branch selects primary route before inserting the image and checks route revision remains stable; the other branch inserts the image first. | The helper inserts canonical tool-result conversation data directly; it does not run read_file dispatch and image-result admission end to end. |
| read_file result when primary is already known text-only | `crates/codegen/shell/src/session/actor/tests/read_file_image_description_tests.rs::known_text_only_model_removes_read_file_image_before_sampling` | Uses `handle_bridge_tool_success` for an image result and verifies the pre-sampling projection while immutable Timeline evidence remains. | Does not cover the primary HTTP rejection and visual fallback after that read_file result. |
| Unknown primary is not preempted | `.../read_file_image_description_tests.rs::configured_auxiliary_does_not_preempt_unknown_current_model` | Configured but unresolved auxiliary does not remove a read_file image while current primary is unmarked. | Does not cause a primary rejection or exercise configured auxiliary invocation. |
| PDF groups and removed-image route switch | `.../read_file_image_description_tests.rs::pdf_extracted_images_stay_one_ordered_group_and_only_the_text_route_is_projected` | Preserves ordered PDF image grouping; verifies removal stays removed after switching models. | Does not exercise capability alias sharing across transports. |
| Model catalog refresh | `.../read_file_image_description_tests.rs::live_model_reload_updates_every_next_turn_sampler_knob` and selected-route cases in `crates/codegen/shell/src/session/actor/model_switch.rs` | Verify configured auxiliary alias and route knobs can update. | They do not make a subsequent image fallback request and assert the resolved auxiliary endpoint/backend/query used by that request. |
| Runtime key shape | `crates/codegen/shell/src/session/actor/turn/sampling.rs::image_input_identity_includes_canonical_query_route` | Checks query ordering does not change the key and a different deployment query does. | Does not verify that Shell stores and looks up this typed key instead of an alias, nor that backend/endpoint changes affect a real auxiliary call. |
| Description reuse and recovery | `.../image_input_recovery_tests.rs::active_goal_image_400_uses_auxiliary_description_then_retries_without_images`, `crates/codegen/shell/src/session/image_describe.rs::describe_request_and_cache_cover_all_pdf_page_images`, and `crates/codegen/shell/src/session/storage/jsonl/tests.rs` completed-description recovery cases | Covers in-memory reuse, cache key/source revision behavior and storage-level Sideband recovery validation. | No actor-level cold restore test confirms a completed Sideband is reused without a new auxiliary request. Cross-aux-route description reuse is valid behavior and is not itself a defect. |
| OCR and deadlines | `crates/codegen/shell/src/session/image_describe.rs::local_ocr_tests::{cancellation_terminates_the_local_ocr_process,local_process_retains_text_and_rejects_empty_or_failed_output}` and `crates/codegen/shell/src/session/actor/turn/sampling.rs::image_description_deadline_tests` | Covers OCR process helper behavior and the generic absolute deadline wrapper. | Does not cover the complete fallback orchestration after cancellation/deadline, nor a deterministic successful OCR projection. |

## Required identity regressions

The focused tests for this change should demonstrate:

- Two catalog aliases that resolve to the same full `ModelImageInputKey` share one recorded rejection.
- The same alias resolving to a changed provider-facing model, backend, endpoint or query route does not inherit the old rejection.
- An auxiliary alias reconfigured to a new route is actually invoked through that route; if it resolves to the exact rejected primary key, it is skipped to avoid recursive invocation.
- Primary and auxiliary rejections are recorded against the frozen key of the request that received the rejection; compaction queries the same transport-keyed state.
- Existing direct-image, read_file/PDF, historical-image, model-switch and durable projection acknowledgement tests continue to cover the existing fallback layers. Use targeted identity/route tests where they provide deterministic evidence; do not claim every failure kind has an end-to-end test.

## Remaining limits and status

- Auxiliary API/network failure, empty response, timeout, Sideband admission/persistence failure and user cancellation are not all covered as full fallback integration scenarios. OCR helper tests and deadline-wrapper tests are narrower unit evidence.
- Existing removal integration cases can reach real local `tesseract` through `PATH`; without an injected OCR executable they do not prove the successful OCR branch deterministically.
- Cold actor reuse of a completed image-description Sideband remains covered at storage lookup level, not at actor request-count level.
- 新增 read_file 回归从实际 `handle_bridge_tool_success` 接入图片结果，串联 primary HTTP 拒绝、跨 endpoint/backend 的 configured auxiliary、持久化 ACK gate 与重试；它没有派发文件系统 `read_file` 执行本体。历史图片回归仍直接插入 canonical `ToolResult`。
- No live session or installed binary is part of these tests. 修复前两条 regression 已复现；最终执行结果与隔离验证范围见 verification.md。

## Added adversarial coverage

- `image_input_rejection_cache_tracks_runtime_changes_and_aliases`：同 transport 的 alias 共享拒绝；同 alias 的 model/backend/endpoint/query 四种独立变化不继承旧拒绝；恢复原 transport 后仍命中。
- `auxiliary_image_rejection_follows_resolved_route_and_blocks_recursion`：auxiliary 明确拒绝后跳过原 transport；同 alias 更新 endpoint/backend 后重新解析可用路由；新 alias 不能绕过旧 transport 拒绝；解析到 rejected primary 的 alias 不递归。
- `read_file_image_fallback_uses_refreshed_auxiliary_and_waits_for_projection_ack`：未知 primary 的拒绝恢复和已知 text-only primary 的发送前修复均实际调用配置的 Responses 视觉服务；辅助请求 model/image 与 endpoint 正确。持久化 ACK 暂停时主请求不提前发送；ACK 后正文使用描述且无图，Timeline 仍保留原图。
- `background_image_compaction_rejects_changed_transport_before_recovery`：后台 compaction 的 alias-only 变化保持身份；model/backend/endpoint/query 变化使旧请求失去恢复资格，避免把旧拒绝归到新配置。

## Dispositions

确认缺陷集中于同一根因：负向图片能力状态按 catalog alias 而非实际 transport 绑定。修复复用现有 `ModelImageInputKey`，未新增 fallback 框架。后台 compaction 的 existing pending model identity 同步覆盖完整 transport，使拒绝恢复拿到冻结身份。未发现需要替换现有 auxiliary → OCR → removal 顺序的证据。OCR、取消与冷恢复的分层测试边界如上，不能扩写为全流程验证结论。
