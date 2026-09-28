## ADDED Requirements

### Requirement: Image budget choices remain stable between reclamation boundaries

在同一存活会话、相同请求投影域内，采样 SHALL 对已因请求图片预算替换的旧图片沿用相同替代内容，并基于应用既有选择后的请求大小判断是否再次回收。纯追加后未达到高水位时 SHALL NOT 因重新读取原图而推进旧图片淘汰边界。再次达到高水位时 SHALL 按时间顺序回收至低水位或可回收图片耗尽；原始 Timeline 图片证据 SHALL 保留，最终协议请求仍受既有字节上限约束。

#### Scenario: Append after the first reclamation
- **WHEN** 一次请求已批量淘汰旧图片，后续请求只追加新消息或图片，应用既有淘汰选择后仍低于高水位
- **THEN** 此前图片与 placeholder 的选择保持不变，不因预算算法额外改写历史、重置 native continuation epoch 或改变其派生 cache key。

#### Scenario: Reach the next reclamation boundary
- **WHEN** 应用既有淘汰选择后的请求再次达到高水位
- **THEN** 按 oldest-first 扩大淘汰范围，后续未达高水位的请求沿用新范围；真实历史投影变化继续经过既有 native 失效判断。

#### Scenario: Multiple image parts in a tool result
- **WHEN** 旧 User 或 ToolResult 包含多个图片及文本 part，并重复构造三种 backend 的请求
- **THEN** 被选择的图片持续位于原因果位置并使用相同替代文本，其他附件和工具关联保持有效，Timeline 原图不被请求预算修改。

#### Scenario: Non-image content exceeds the limit
- **WHEN** 已回收可回收图片但最终编码后的协议请求仍超过现有请求字节上限
- **THEN** 请求按既有大小失败路径终止，不通过恢复原图、绕过上限或无限预算重试发送。

证据入口：`crates/codegen/chat-state/src/actor/request_builder.rs` — `build_conversation_request`、`compact_images_to_byte_budget`；`crates/codegen/sampler/src/client.rs` — `check_and_record_request`。新增跨 build 回归属于本 change 待实现内容。

### Requirement: Image budget choices follow their request projection lifetime

图片预算选择 SHALL 仅适用于其源 Surface 身份和图片呈现方式。源历史或投影域被替换时 SHALL 基于新 Surface 重建选择；单纯 native continuation reset SHALL NOT 清除仍然有效的预算选择。fork 和冷恢复 SHALL 独立建立临时选择，不从历史请求证据恢复 native 状态，也不复活当前 Surface 已明确移除的图片。

#### Scenario: Native protocol recovery without image changes
- **WHEN** 当前路由撤销 native continuation，但源 Surface、图片呈现和预算没有变化
- **THEN** 下一次请求继续沿用既有图片替代选择，撤销的 native 数据不因此恢复。

#### Scenario: Surface or presentation changes
- **WHEN** rewind、compaction、旧 Surface 替换、route 替换、goal 过滤域或图片描述呈现域改变
- **THEN** 旧选择不应用到不同身份或内容的 part，按当前投影建立新选择；恢复和 native 规则继续独立执行。

#### Scenario: Fork, cold restore and resident reconnect
- **WHEN** 创建 fork 或冷恢复 actor，或客户端重新连接原存活 actor
- **THEN** 前两者从各自当前 Surface 建立选择，后者保留原选择；均不得修改 immutable 图片证据。

证据入口：`crates/codegen/chat-state/src/actor/state.rs` — `ContinuationLane`；`actor/mod.rs` 的 route/config 控制入口；`actor/mutations.rs` 的 Surface 替换入口。
