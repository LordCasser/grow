# 上下文与请求路径算法审计

本次只读审计覆盖 chat-state 请求构建（不含 Timeline 恢复）、sampler、sampling / provider 请求编码、common/compaction 和 token-estimation。没有修改生产代码、测试或 backlog，也没有读取真实 session。Atlas 先执行 `project(open)`，随后只做限定目录查询；其调用关系结果仅覆盖当前 Focus 闭包，因此下文调用方另以源码核对。本文不代表全仓穷举。

参考契约：`openspec/specs/session-timeline/spec.md`、`openspec/specs/model-sampling/spec.md`、`openspec/specs/context-compaction/spec.md`。开始审计时已有 change 为 `add-mcp-multi-round-elicitation` 和 `archive`，均不涉及以下候选。

## 优化候选

### C1 — 图像预算复用时避免重复线性查找 SurfaceId

- **证据：** `crates/codegen/chat-state/src/actor/request_builder.rs:99-118` 对 `selection.replaced` 中每个条目调用 `surface_ids.iter().position(...)`，验证和应用占位文本时各查一次。映射定义见 `crates/codegen/chat-state/src/actor/state.rs:324-333`；`SurfaceId` 实现了 `Hash`，见 `crates/codegen/chat-state/src/timeline.rs:85-90`。
- **调用方与触发条件：** 每个 `BuildConversationRequest` 都会进入构建函数，见 `crates/codegen/chat-state/src/actor/mod.rs:971-990`。当同一路由、Goal 和图片描述配置复用含 K 个替换项的选择，且 Surface 有 N 项时，重复查找会放大。
- **复杂度：** 当前查找最坏为 O(N×K)，另有正常的 O(N) 请求遍历。先建索引后查找可降为期望 O(N+K)；若用有序映射则为 O(N+K log N)。
- **最小方案：** 从 `surface_ids` 一次构建 `SurfaceId -> index` 映射，供验证与写入占位文本共用；保留现有 `BTreeMap` 选择顺序。
- **必须保持：** SurfaceId 精确身份、通过 Weak 指针核验原图像 part、任一选择失效时整体清空、part 坐标、从旧到新淘汰，以及仅改请求投影。不能把位置前缀当作替换项身份。
- **验证线索：** `crates/codegen/chat-state/src/actor/request_builder.rs:580-910` 有图像预算测试；`crates/codegen/chat-state/src/actor/tests.rs` 有请求投影测试。实现时应增加多项 Surface / 多项替换的等价测试，并覆盖过期 SurfaceId 与图像身份不匹配。此次未运行测试。
- **置信度：** 复杂度判断高；收益中高，取决于保留的淘汰选择中图像数量。

### C2 — 多条历史修复改为一次重建 Vec

- **证据：** `crates/codegen/chat-state/src/compaction_utils.rs:1069-1100,1141-1147` 先找出损坏 exchange，再逐条反向 `splice`；`crates/codegen/sampling-types/src/conversation.rs:4876-4900` 逐个 `remove` 重复 ToolResult；同文件 `:4726-4785` 对 dangling 调用逐条 `splice`。
- **调用方与触发条件：** `repair_history_with_reason` 串联隔离、去重、清理错位结果和补齐 dangling 调用，见 `crates/codegen/chat-state/src/compaction_utils.rs:1022-1046`。它在封闭写入边界执行，包括请求准备 `crates/codegen/chat-state/src/actor/request_builder.rs:35-38` 和用户输入接纳 `crates/codegen/chat-state/src/actor/mutations.rs:463-480`。这是损坏历史恢复路径，正常历史不会触发实际编辑。
- **复杂度：** 发现编辑项是 O(N)，但应用 K 次中间插入或删除时会反复搬移 Vec 尾部，最坏 O(N×K)，K 随 N 增长时为二次复杂度。一次重建输出可将应用成本降为 O(N + 输出大小)，不含隔离证据的序列化成本。
- **最小方案：** 形成有序编辑计划（含替换内容），一次遍历并写入新 Vec；移动原条目前先用原始范围生成隔离证据。
- **必须保持：** 连续的 tool-call/result 配对规则、重复结果保留最后一项、原顺序、损坏 exchange 作为不可执行证据隔离、结果证据保留和持久化修复边界。不能改变历史 IntegrityRepair 的证明语义，也不能随意合并证据边界不同的相邻范围。
- **验证线索：** `crates/codegen/chat-state/src/compaction_utils.rs` 已有 `malformed_exchange_preserves_ambiguous_results_as_non_executable_evidence`、`duplicate_ids_quarantine_the_whole_exchange_not_just_one_sibling`、`repair_history_dedups_duplicate_results` 和 `repair_history_strips_displaced_result_and_backfills_call`。`crates/codegen/sampling-types/src/conversation.rs:10587` 起有 dangling repair 测试。实现时应覆盖大量交错且互不相邻的损坏、dangling、重复组，并核对结构与顺序。
- **置信度：** 最坏复杂度判断高；收益中等，因为多组损坏较少见，但路径明确负责历史损坏和取消恢复。

### C3 — 缓存 prompt cache key 的 rewind 分支锚点

- **证据：** `crates/codegen/chat-state/src/actor/request_builder.rs:247-263` 每次计算 `prompt_cache_key` 都反向扫描 `timeline.events()` 找最近的 `MessageCause::Rewind`；请求构建在同文件 `:224-230` 调用它。
- **调用方与触发条件：** 每次 `BuildConversationRequest` 都会执行该扫描，调用入口见 `crates/codegen/chat-state/src/actor/mod.rs:971-990`。没有 rewind 的分支会每次扫完整事件历史；普通追加按契约不改变 key。
- **复杂度：** E 个事件时每次请求 O(E)，R 次请求累计 O(R×E)。在 Timeline 或 actor 中维护最近 rewind 序号，可让读取变为 O(1)，并在确认 rewind 时更新。
- **最小方案：** 将最近 rewind 序号作为 Timeline 派生元数据，在现有恢复时重建、在 rewind 被接受时更新；也可使用等价的 revision 缓存。避免每次请求扫描完整事件数组。
- **必须保持：** 普通追加时 key 不变；Timeline ID、provider backend/base URL/model、continuation epoch 或最近 rewind 改变时 key 按现有规则变化；fork 相互隔离；失败或未确认的 rewind 不能发布新锚点。
- **验证线索：** `crates/codegen/chat-state/src/actor/tests.rs:8155-8215` 的 `prompt_cache_key_tracks_timeline_model_and_rewind_lineage`，以及同文件 `:7709-7811`、`:8476` 附近的 route / 稳定性测试。实现时应覆盖大量普通追加后的多次请求，以及失败或未确认 rewind。
- **置信度：** 复杂度判断高；整体收益中等，因为单次扫描开销有限，缓存还需覆盖所有 rewind 与恢复路径。

### C4 — 避免完整性检查后再多做一次 Surface 克隆

- **证据：** `crates/codegen/chat-state/src/actor/mutations.rs:173-184` 在尝试修复前无条件克隆完整 Surface；无须修复时该副本随即丢弃。随后 `crates/codegen/chat-state/src/actor/request_builder.rs:69` 再克隆 Surface 作为 provider 请求内容。
- **调用方与触发条件：** 每个 `BuildConversationRequest` 先执行完整性检查，见 `crates/codegen/chat-state/src/actor/request_builder.rs:35-38` 和 `crates/codegen/chat-state/src/actor/mod.rs:971-990`，包括健康历史。
- **复杂度：** 两次克隆均为 O(N) 个 item；内容由 Arc 持有，所以这里主要是额外的 O(N) 分配和引用计数操作，不会复制图像正文的字节。
- **最小方案：** 调整准备边界，让请求副本在需要时也作为修复候选，或把克隆推迟到确认需要修复之后。不要为省一次克隆再增加一遍完整无界扫描。
- **必须保持：** 只在安全的封闭边界修复；修复变更必须先获得 Timeline 持久化确认，再投影请求；Timeline 写入失败不能返回部分修复请求；健康历史请求语义不变。
- **验证线索：** `crates/codegen/chat-state/src/actor/tests.rs:5840-5970` 有崩溃后和取消后的 dangling 修复测试，`:6428-6448` 有 dangling 状态测试。实现时还应核对健康历史请求不变，以及持久化失败不返回候选副本。
- **置信度：** 多余克隆判断高；收益与实现难度为中等，因为变更涉及修复 helper 的所有权和提交边界。

## 已扫描但无新增候选的模块

- `crates/codegen/token-estimation/src/lib.rs`：`estimate_tokens` 只读 `str::len`，为 O(1) 的 bytes/4 估算；图片和百分比计算也是常数时间饱和算术。契约和单元测试均明确。改为扫描字符会改变估算语义，不属于纯算法优化。
- `crates/common/compaction/src/prune.rs`：`plan_tool_result_pruning` 每项只计数一次，再单次求和和选择，复杂度 O(N)，计数数组占 O(N) 空间。一次性保存计数是为避免重复调用可能昂贵的 host counter。UTF-8 字符边界最多调整 3 个字节。未发现新增候选。
- `crates/codegen/sampler/src/actor/request_task.rs` 与 `crates/codegen/sampler/src/stream/` 下的 `responses.rs`、`messages.rs`、`chat_completions.rs`、`collect.rs`：重试受配置额度、恢复预算、取消和期限约束；协议查找主要用 map/set 或局部扫描。审阅范围内未发现无界重复扫描或克隆热点。
- `crates/codegen/sampling-types/src/conversation.rs` 中 provider 请求投影和编码：请求分段与后端转换主要按 item / segment 线性遍历。Messages 工具 ID 编码有快速路径：中性 ID 已符合 wire 格式时直接返回，不建保留集合，见 `conversation.rs:4985-5036`；需要编码时才用有序集合与映射。未发现另一个高置信度候选。
- 本审计未覆盖 Timeline 批量恢复、其他 crate、provider SDK 内部或全仓；不据此声称全库无其他机会。
