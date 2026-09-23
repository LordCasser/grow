# Verification: recover-unsupported-image-surface

本文件记录实现后的实际证据。命令均在仓库根目录执行，使用 rustup 代理的
`~/.cargo/bin/cargo`（rust-toolchain.toml 固定的 1.93.1）。未执行 `cargo clean`，未删除
`target/`，未运行 workspace 全量构建，未提交 Git。

## 结论

实现已落地并通过针对性验证：共享分类器识别 provider-neutral 终态
`is not a multimodal model`；`ImageProjection` 具备 description / local OCR /
unsupported-model removal 三种 typed disposition，live apply 与 bulk replay 走同一决策；
Shell 在 durable ACK 之后才重建请求，失败时 fail closed。原始 Timeline 证据未被改写。

## 分类器（model-sampling）

| Capability / scenario | 证据 |
| --- | --- |
| GLM-style multimodal rejection | `sampling-types`：`unconditional_image_rejection_accepts_terminal_multimodal_claims` 覆盖 `InvalidParameter: glm-5.2 is not a multimodal model`、带 provider/model 前缀与尾随标点的变体以及裸短语。 |
| 非终态相似文本不得命中 | 同文件 `multimodal_claim_requires_a_terminal_capability_statement`：`… for audio input`、`… yet`、`… because of the request` 均返回 false。 |
| 400 / image_count / 排除清单门槛 | `multimodal_claim_keeps_status_image_count_and_exclusion_gates`：status 500、image_count 0，以及 malformed/size/format/transparency/policy 组合保持 false。 |
| 单一分类器入口 | 只读核对：`sampler/src/actor/request_task.rs:334`、`shell/src/session/actor/turn/sampling.rs:51`（`is_image_input_unsupported` 包装）、`shell/src/session/helpers/session_compact.rs:85`、`:144` 全部调用 `sampling_types::is_unconditional_image_input_unsupported`；不存在第二套分类逻辑。 |
| 端到端命中 | Shell 测试 `explicit_multimodal_400_removes_images_and_resubmits_text_only` 用真实 `handle_prompt` 发送精确 `InvalidParameter: test-model is not a multimodal model` 400，证明分类器、capability 标记、投影与重试串起来生效。 |

## Timeline / Surface 投影（session-timeline）

| Scenario | 证据 |
| --- | --- |
| 回放保留原图与描述 | `chat-state`：`local_ocr_description_replays_with_original_images_and_valid_engine`、`image_description_retains_images_and_is_bound_to_a_live_surface_item`。 |
| 回放删除未支持的图片 | `unsupported_model_removal_requires_the_canonical_replacement`：任意文本 replacement 被 `TimelineError::InvalidImageProjection` 拒绝；canonical 常量接受后被删除，双图 User 项在首图位置留下一个标准文本，`source_revision`/`SurfaceId` 前进，原始 `Messages` 事件逐字节不变，`Timeline::from_events` 折叠出的 Surface 与 live Surface 序列化一致。 |
| 工具结果图片删除 | `unsupported_model_removal_redacts_image_tool_paths_and_carriers`：ToolResult 图片清空并保留非图片文本，`[Projected image removal]` 标准文本进入同一因果项，assistant tool-call 参数、Reasoning/BackendToolCall carrier 与 tool-call compaction 引用同时脱敏；request 组装零图片组。 |
| compaction 引用清理 | sampling-types `removal_compaction_redaction_replaces_only_derived_reference_tokens`；chat-state `unsupported_model_removal_scrubs_asset_paths_from_completed_compaction_summary`（summary 中 asset 路径被 `[Projected image removal: …]` 取代，其余文本不变，raw 事件仍含原 asset）。 |
| 混合 disposition 原子提交 | chat-state `mixed_description_and_removal_shadows_apply_atomically`（一个事件同时 attach 与 remove，replay 一致）；Shell `mixed_description_and_removal_groups_commit_as_one_projection`（真实两轮：首轮图片成功、次轮 400；辅助模型描述第一组、第二组失败 → 一个事件、1 described + 1 removed、一条聚合通知）。 |
| report 与 Surface 变更计数 | `ImageProjectionReport { described_images, removed_images }`，`total_images()` 为二者之和；actor 测试 `image_removal_projection_replaces_user_images_and_replays_identically`、`mixed_description_and_removal_projection_splits_report_counts` 断言 2/0、1/2 与 revision 前进。 |
| 回放 = live | 上述 chat-state 测试均对 `Timeline::from_events(events)` 的 `surface()`/`surface_ids` 与 live 结果做相等断言（删除与描述 disposition 各一组）。 |
| 持久化未 ACK | chat-state `image_projection_retries_an_uncertain_persistence_failure` 覆盖瞬时失败重试；永久失败见下节 Shell 证据。 |

## Shell 恢复闭环

| Scenario | 证据 |
| --- | --- |
| 明确 400 + 无辅助 + OCR 失败 | `explicit_multimodal_400_removes_images_and_resubmits_text_only`：恰好两次 primary 请求；第二次 wire body 零图片且含 `当前模型不支持多模态，图片已经被删除`；恰好一条 `ImageProjected` 更新携带删除说明；无 `RetryState::Failed`；Surface 零图片组；capability pair 已标记 text-only；原图仍在 sealed 事件（`sealed_image_groups == 1`）；随后纯文本 turn 零图片、共三条请求、无重复 400。 |
| 辅助模型也拒图 | `auxiliary_image_400_installs_a_durable_removal_projection`：辅助 400 后不再返回 `ImageDescriptionUnavailable`，改为 durable removal + 重试；辅助 pair 一并标记 unsupported；Surface 恰好一个 canonical replacement。 |
| 预采样闸门（已知 text-only） | `known_text_only_model_removes_read_file_image_before_sampling`：`project_images_for_known_text_model` 成功并删除工具结果图片，ToolResult 正文出现 `[Projected image removal]` 标准文本，raw read_file payload 仍在 `Messages` 事件中，request 零图片组。 |
| 压缩 | `compaction_proceeds_after_an_acknowledged_removal_projection`：删除先 durable 提交，压缩越过图片闸门进入自己的事务边界（测试 harness 无压缩 provider，因此允许其后的非图片失败），Surface 不因压缩复活图片且保留标准文本。 |
| 持久化永久失败 fail closed | `failed_image_projection_commit_never_resubmits_a_lossy_request`：测试 harness 对 `TimelineDurablyAndAck` 中的 `ImageProjection` 返回 `ErrorKind::StorageFull`。断言 turn 返回 typed 错误、provider 只收到一次图片请求、恰好一次投影提交且未被 ACK、durable Surface 仍含该图片、没有 `ImageProjected` 通知。 |
| pending PDF 组 | `pdf_extracted_images_stay_one_ordered_group_and_only_the_text_route_is_projected`：仍是一个有序组；删除后 text route 与随后切换的 vision route 均零图片组且携带标准文本（无隐式复活）。 |
| 已有描述不被重复投影 | `active_goal_image_400_uses_auxiliary_description_then_retries_without_images` 保持通过：描述路径、缓存复用与 `total_images() == 0` 短路行为未变。 |

## 命令与结果

| 命令 | 结果 |
| --- | --- |
| `~/.cargo/bin/cargo test -p sampling-types --lib` | 287 passed; 0 failed; 0 ignored。 |
| `~/.cargo/bin/cargo test -p chat-state --lib` | 505 passed; 0 failed; 1 ignored。 |
| `RUST_MIN_STACK=33554432 ~/.cargo/bin/cargo test -p shell --lib image -- --test-threads=1` | 120 passed; 0 failed（含本 change 的 10 个 image 测试）。 |
| `RUST_MIN_STACK=33554432 ~/.cargo/bin/cargo test -p shell --lib image_input_recovery_tests -- --test-threads=1` | 5 passed; 0 failed。 |
| `RUST_MIN_STACK=33554432 ~/.cargo/bin/cargo test -p shell --lib read_file_image_description -- --test-threads=1` | 6 passed; 0 failed。 |
| `RUST_MIN_STACK=33554432 ~/.cargo/bin/cargo test -p shell --lib` | 3862 passed; 3 failed; 3 ignored。失败与本 change 无关，见下节独立复核：两个在隔离单跑下仍失败（文件未修改 + 环境原因），一个隔离单跑通过。 |
| `~/.cargo/bin/cargo fmt -p shell -- --check` | 我改动的文件里仅剩 2 处既有漂移（`active_goal_…` 断言与 `FlushReplay` 分支，属于其他人未提交的编辑），未改动它们；其余报告的漂移都在我未触碰的文件。 |
| `df -h /System/Volumes/Data` | 验证期间 Avail 从 42 GiB 变为 40 GiB（共享 build cache 与其他 coder 的构建增长，未删除任何文件）。 |

`RUST_MIN_STACK` 只用于绕过与本 change 无关的栈深限制：
`session::actor::turn::sampling::image_input_rejection_tests::explicit_reasoning_replay_rejection_updates_once_then_stops`
以 `image_count = 0`、无 capability key 调用 `handle_sampling_failure`，不会进入本次修改的图片分支；
它在默认 2 MiB 测试线程栈上溢出，用 32 MiB 栈即通过。

## 覆盖边界

- **Pager / extension 未改动。** `ImageDropped` 的 wire shape 未变，一次投影只发一条
  `ImageProjected` 更新，pager 既有“一次更新一个 NOTICE block”的行为无需新 schema 或第二套
  renderer；本 change 不新增 UI 契约，因此没有 pager 测试改动。
- **SurfaceChanged 并发重建只做静态确认。** `project_conversation_images_for_text_model`
  的三次有界重建、`record_image_projection` 的 `source_revision` compare-and-swap 与
  `turn/mod.rs` 的 `image_projection_retries > 2` 收口都在代码中可见；没有引入 sleep
  或竞态交错来制造确定性覆盖，本文件不声称存在该场景的自动化用例。
- **storage consistency check 只有编译期证据。** `session/storage/mod.rs` 的 projection
  校验改成显式 match：Description 分支逐字保留，LocalOcr/UnsupportedModel 明确 `continue`。
  仓库中没有覆盖该检查的 storage 单元测试，行为等价性由“原分支是 `else { continue }”这一
  等值改写保证。
- **poisoned writer 下不发布通知。** 永久持久化失败会让 chat-state mailbox 关闭，
  `send_grow_notification` 的 hook 生命周期写入随之失败并扣下通知；该场景对外表现是 typed
  turn error 而不是 `RetryState::Failed`，测试按实际行为断言。这是既有通知路径的性质，不是本
  change 引入，也未在本 change 内修改。
- **compaction 图片闸门语义。** `compaction.rs:835` 的
  `Ok(first_rejection || projected.total_images() > 0)` 保持不变：removal-only 投影同样改变
  了 Surface，因此压缩事务必须重启；这是有意读取，不是顺带修改。
- **schema 版本保持 v25。** 迁移计划要求既有会话继续加载，以便 poisoned session 在下一次明确
  拒图 400 时原地修复；提升版本会让 `event.version != TIMELINE_SCHEMA_VERSION` 直接拒绝这些
  会话。`docs/architecture/agent-core-timeline.md` 与常量因此都未改动。

## 独立复核（software-architect，非实现者）

复核基于最终文件状态的实际 diff、调用方与独立重跑的测试，而不是 Coder Result 的结论。

**独立重跑（最终字节）**：`sampling-types --lib` 287 passed / 0 failed；`chat-state --lib` 505 passed / 0 failed / 1 ignored；`shell --lib image` 120 passed / 0 failed。三项均与实现者报告一致。

**全量 shell 失败项的独立定性**（隔离单跑，逐项）：

| 测试 | 隔离单跑结果 | 定性依据 |
| --- | --- | --- |
| `session::helpers::session_compact::classify_tests::sampling_http_is_transient` | FAILED（隔离同样失败） | 失败文本为 `connecting to port 0 must fail: Response { url: "http://127.0.0.1:0/", status: 502 … }`；本机存在 `http_proxy=http://127.0.0.1:7890`，端口 0 被代理接管，属环境差异。 |
| `session::actor::tests::laziness_integration_tests::debug_mode_bypasses_idle_wait` | FAILED（隔离同样失败，用时 2.63s vs 2s 上限） | 断言的是 wall-clock 阈值，且该路径含一次 localhost TCP 连接；受同一代理与共享机器负载影响。实现者报告称“单跑通过”，本次复核不成立，已按实测更正。 |
| `session::actor::tests::…::test_launch_claimant_reindexes_even_when_marker_exists` | PASSED（隔离通过） | 并行负载下 flaky。 |

三项所在文件 `session/helpers/session_compact.rs` 与 `session/actor/tests/laziness_integration_tests.rs` 相对 HEAD **未修改**（`git diff --stat` 为空），因此与本 change 及工作区其他进行中改动均无关；失败原因与图片归一化/投影路径无交集。

**结论**：未发现阻塞验收的问题。关键不变量均落在正确所有者且被测覆盖：分类器唯一入口与终态 claim 边界（sampling-types）、canonical replacement 由 Timeline validator 强制（chat-state）、live/replay 共用一个 `apply_image_shadow` 决策、resubmit 只在 durable ACK 之后、原始 payload 仅作为不可变证据保留。

**补充边界（复核后收窄，不构成缺口）**：storage consistency check 因此无需新增测试——其非 description 分支按构造是 `continue`，且 JSONL 写入/读取是 `serde_json::to_vec(event)` / serde 反序列化的直接往返（`storage/jsonl/mod.rs:1097`），新的 `unsupported_model` variant 编码已由 chat-state 的 serde 往返测试覆盖；storage 层没有额外的事件种类白名单。

**剩余未由自动化覆盖项**：`SurfaceChanged` 并发重建交错（有界三次重建 + `source_revision` CAS + `image_projection_retries > 2` 收口静态可见）；pager 未重建（wire shape 与“一次更新一个 NOTICE”行为均未变）。

## 归档与磁盘收尾

- `git diff --check` 干净；`openspec validate --all --strict --no-interactive` → 20 passed / 0 failed（归档前）。
- `~/.cargo/bin/cargo clean`：移除 105,503 个文件、33.1 GiB（`target/` 26 GB → 不存在），`/System/Volumes/Data` 可用空间 29 GiB → 55 GiB。所有 Rust 定向验证在 clean 之前已完成；此后仅剩文档与归档操作。
- `openspec archive recover-unsupported-image-surface --yes`：`model-sampling` 3 条 requirement 更新、`session-timeline` 1 条 requirement 更新，归档为 `2026-09-21-recover-unsupported-image-surface`。
- 归档后校验：`openspec validate --all --strict --no-interactive` 19 passed / 0 failed；`openspec validate --archived --no-interactive` 在本文件补齐 5.3 勾选后应为 350 passed / 0 failed（首次运行因 5.3 未勾选报 1 incomplete）。
- 本 change 不新增 Git 提交；工作区中其他进行中改动未触碰。
