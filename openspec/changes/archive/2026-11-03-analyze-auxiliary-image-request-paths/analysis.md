# 辅助模型图片请求路径审计

调查对象是 2026-11-03 当前工作区，包含前轮 `fix-coordination-image-context` 和其他未提交改动。本轮不修改业务代码或测试。

## 结论

同类缺口还存在于 **recap** 和 **Memory Flush**。两者直接消费含原图的 Surface，portable history 保留图片，未选择已有描述；纯文本路由会收到图片，可能返回截图同款 `messages.content.type ... ['text']`。这由请求构造和 wire 转换可确定；本轮没有对两条真实 actor 入口新增注入 400 的集成测试，未将源码判断冒充线上复现。

前轮修复也有明确边界：协调问答与 `/btw` 优先复用已有文字，但 **未知路由 + 尚未转换的图片** 仍按既定契约尝试原图。若切换后立即咨询，且主转换从未发生，同类首次 400 仍可能出现；咨询不会自行记录负能力或启动转换。原来的修复覆盖已提交文字分支遗漏与转换等待，没有承诺所有图片输入都能在只读咨询中恢复。

此外发现两类路由风险：辅助功能指定模型时沿用主会话 client，以及部分调用先建 client 再独立读取 model。这些可能产生其他 400/404/认证或不支持字段错误；纯文本请求本身不会触发图片类型拒绝。需按独立边界处理。

## 请求矩阵

| 入口 | 图片来源与适配 | 拒绝/失败边界 | 结论 |
| --- | --- | --- | --- |
| 主会话与普通 child 主采样 | 统一请求构造，已知纯文本路由预投影；未知路由先发原图 | 明确图片拒绝后记录精确 transport 能力，vision/OCR、durable ImageProjection、重建请求 | 已有恢复；未发现本次同类遗漏 |
| 协调 `ask_parent` / `ask_subagent` / peer | 接收方冻结历史；共用 `freeze_sideband_context`，等待主转换 ACK、优先选择描述 | 已知纯文本且缺描述则请求前失败；未知路由保留未转换图片并可能首次拒绝 | 已修复描述遗漏；首次未转换边界仍存在 |
| `/btw` | 与协调问答共用冻结入口 | 失败不改主图片状态，不自行恢复 | 同上，符合已归档契约 |
| 手动/自动 recap | `materialize_timeline` → `budget_recap_items`，保留用户/工具图片，不选择描述，不读图片 gate | 单次失败记 Sideband Failed；手动发 unavailable，自动丢弃结果 | **确认的适配缺口** |
| Memory Flush（含压缩前、idle） | frozen Surface → bounded portable window，保留附件；只有输入预算/4 MiB 图片预算，没有能力选择 | 单次失败跳过本次写入，主任务继续；不会转换或重试图片拒绝 | **确认的适配缺口** |
| 同步压缩 | 可携图；已知路由选择描述，手动入口还预投影 | typed 图片拒绝经 `run_compact_inner` 进入一次图片恢复，再启动新事务 | 已有恢复 |
| 后台压缩 | 同一图片能力选择；保留冻结来源与 transport key | 未提升失败可丢弃并回退同步路径；提升后可恢复；换 transport 时不能用旧拒绝标记新路由 | 已有恢复/失效保护 |
| 视觉辅助描述 | 有意只发图片组 + 固定中性 prompt，独立解析辅助路由 | 确认辅助路由也拒图则标记该路由，再走 OCR/既有降级 | 有意携图，非咨询遗漏 |
| AI shell suggest、prompt suggest | system/user 文本，后者先生成紧凑文字 transcript | 失败返回空建议 | 不会由历史附件触发图片类型拒绝；有模型路由风险 |
| Session title | 从首个真实用户文本生成纯文本请求，固定 title route | 独立 title 生命周期 | 未携图 |
| Context Recall | archive/need context 先渲染成文本 JSON Lines，再包成两条文本消息 | 工具返回失败，不恢复图片 | 未携图；配置快照存在待复现疑点 |
| Dream / finalizer、memory note rewrite | 文本 prompt；不复制原 ConversationItem | 独立 Sideband 失败 | 未携图；分别有快照/固定模型路由风险 |
| Laziness、Auto permission classifier | 显式文本 transcript 或结构化文字/工具证据 | classifier 自身错误语义 | 未携图；Auto 专用模型已通过辅助路由 resolver 构建 |

普通 child 使用同一主采样恢复路径；提问 child 的模型是否支持图片不决定接收方咨询请求的内容。触发因素是**接收方冻结历史**及其准备路由。

## A1：recap 未消费已有图片文字分支

优先级 P2；用户可见功能失败，主 Surface 不损坏。

证据链：

- `shell/src/session/actor/recap.rs:226` 直接物化 Surface，`:281` 准备一致的 client/model/window，`:314` 将原 history 送入 budget builder。整个链路没有 `freeze_sideband_context`、图片读锁或描述选择。
- `shell/src/session/helpers/session_recap.rs:75` 与 `:145` 使用 portable projection；`:414` 附件保全断言明确保留图片。
- `sampling-types/src/conversation.rs:3500` 将 `Image { url, .. }` 转成 `image_url`，description 元数据不会自动变成文本；Responses/Messages 在各自 adapter 同样显式处理图像。只把描述存入 Surface 不能改变 wire。
- `recap.rs:367` 真实发请求，`:371` 错误分支记录失败并清 recap 状态，没有图片恢复。

最小修复边界：recap 按自身 reasoning 保留及预算契约，在冻结请求副本选择已确认图片文字，等待主转换 ACK；不能把 portable projector 全局改成丢图，也不能丢弃已完成工具证据。增加三 backend、用户/工具图片和转换等待的真实入口回归。现有 recap client/model/window 已是单份准备配置，无需重做路由架构。

## A2：Memory Flush 未消费已有文字分支

优先级 P2；会丢失本次新记忆提取机会，不删除已有记忆。

证据链：

- `shell/src/session/actor/memory_dream.rs:852` 直接物化 flush snapshot；压缩前也可预先捕获 snapshot。未参与主图片 gate。
- `shell/src/session/helpers/memory_flush.rs:347` 选择完整近期 User turns，`:375` portable projection 保留图片，`:397` 只统计 URL/description 编码大小。图片预算不代表模型能接受图片。
- `memory_dream.rs:538` 使用这些 items，`:604` 发请求，`:618` 任意 provider 错误直接记 Sideband Failed。
- `memory_dream.rs:786` 处理 flush error，未进入 memory 写入成功分支，最终释放 flush lock。`compaction.rs:589` 调用方按本次 flush 已运行推进周期标记，失败不保证该轮再次提取。

触发条件包括切换为纯文本主模型，也包括设置一个不支持图片的 `flush_model`。已有图片描述仍不能阻止原图进入 wire。`MemoryFlushConfig` 默认 enabled=true，flush_model=None（`config-types/src/memory.rs:326`）；是否实际触发还由记忆可用性及阈值等既有门禁决定。

最小修复边界：在明确的 flush frozen source 上选择已有描述，保留 suffix IDs、source revision、完整工具对及预算；不能简单重新抓取“最新上下文”替换压缩前 snapshot。`memory-search` 目前明确要求保留已完成交换的 attachments，修复需要 delta 说明纯文本路由如何用文字保全图片证据，不能直接删除附件断言。单独确定 unresolved 图片的只读失败规则，不把 flush 变成第二个主图片恢复 owner。

## A3：指定辅助模型时只覆盖 model，client 沿用主路由

已确认请求构造事实；跨 provider 配置下的具体错误未做在线或新 mock 复现，归为独立路由风险，不作为图片 400 的直接根因。

| 入口 | client 来源 | model 来源 |
| --- | --- | --- |
| AI shell suggest | `recap.rs:549` 当前主会话 | `:563` override 或硬编码 `grow-build` |
| prompt suggest | `recap.rs:741` 当前主会话 | `:715` 目录 pin/client hint，`:770` 原字符串 |
| Memory Flush | `memory_dream.rs:496` 当前主会话 | `:554` flush_model，否则另读当前 model |
| memory note rewrite | `memory_dream.rs:887` 当前主会话 | `:916` 硬编码 `grow-build` |

若这些模型与主模型不在同一 endpoint/backend/认证域，仅覆盖 model 不会解析所配置模型的 provider 路由；即便同 endpoint，catalog alias 与 provider model ID 的语义也需要明确。网关原本就接受多个 model 时不一定失败。Auto classifier 的 `turn/sampling.rs:1681` 已使用现有辅助模型 resolver，可作为独立修复的现成边界，不应新增全局模型客户端框架。

## A4：多次独立读取配置的并发疑点

`memory_dream.rs:340` 先 prepare client 后在 `:341` 再取 model；flush 也有相同窗口。`context_recall.rs:166` 取 budget/model，`:232` 之后另 prepare client；`compaction.rs:1145` 取 window/backend、`:1243` reconstruct config、`:1304` 再 prepare client。

这些代码形状不等同于已证明的生产竞态。compaction 的前台 lease、Step control 接纳和后台失效检查会限制可达性；本轮未用真实控制入口与 ACK/Hook 延迟证明混用路由。记录为待定向故障注入验证的独立项，不声称多个配置读取就必然发生错误。若实现修复，应从同一 prepared config 派生 client、显式 model、能力 key 和预算，延续已有 recap/咨询规则。

## 验证与限制

1. `cargo test --locked -p shell --lib session::helpers::session_recap::tests::build_preserves_complete_tool_evidence_and_drops_partial_calls -- --exact`：1 passed。验证 recap helper 确实保留 tool 图片；不是已修复证据。
2. 复用上述 Cargo 产物执行 `session::helpers::memory_flush::tests::flush_request_keeps_completed_tool_evidence_without_assistant_followup --exact`：1 passed，断言编码后的请求仍含原始图片 data URI。
3. 同一产物执行 `session::actor::compaction::context_recall_tests::background_image_compaction_rejects_changed_transport_before_recovery --exact`：1 passed，验证旧后台拒绝不会错误恢复新 transport。

本轮没有新增测试、修改生产源码、付费模型请求或性能结论；两处图片缺口由静态调用链与现有附件保全测试支持。没有宣称所有入口经过完整端到端模型验证。没有创建新 target，复用已有缓存；不清理其他并行任务共用的编译目录。

后续先分别闭合 A1、A2 的图片分支消费，再用跨 provider fixture 验证 A3；A4 先证明真实控制路径可达性。每项单独改变行为契约，不混入本次分析。

## 文档校验

分析 change 的 strict 校验通过；归档前全量 strict 为 19 passed、0 failed。使用 `openspec archive analyze-auxiliary-image-request-paths --skip-specs --yes` 归档，无主规范变更。归档后全量 strict 为 18 passed、0 failed；archive 首次只报告本 change 待登记的归档后任务，完成登记后最终为 806 passed、0 failed。审计文档及 backlog 的 `git diff --check` 通过。
