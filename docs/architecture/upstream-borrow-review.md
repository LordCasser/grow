# Upstream Borrow Review（grok-build fork 后更新对照评审）

> **Status**: Review conclusions（非实现记录）
> **Date**: 2026-08-14
> **Scope**: 2026-07-28 ~ 2026-08-13 的 upstream/xai-org/grok-build 16 个同步提交
> **Author**: software-architect

本文记录 grok-build 分叉后更新中"可借鉴项"的逐项架构评审结论。它是后续实现任务（coder handoff）与 review 的依据；已采纳项的最终实现事实以各模块契约文档为准，本文只保留决策与排除理由。

## 0. 过滤原则

1. **本地优先**：功能价值若来自 xAI 云端平台（gateway、托管会话、订阅计费、发布式 bundle），不引入。
2. **单一权威来源**：grow 已有同语义机制时，只做审计与导出，不新建平行抽象。
3. **单一调度语义**：任何改动不得引入第二条 turn admission/interjection 路径（`v1.0.0-regression-analysis.md` 的教训）。

## 1. 已排除项（无实现任务）

| 上游功能 | 排除理由（已核实事实） |
|---|---|
| 图像预算滞回（47MB 触发/25MB 回收） | `chat-state/src/actor/request_builder.rs` 已有 `IMAGE_COMPACT_TRIGGER_BYTES`、低水位回收、per-turn 记录 |
| 401 认证归因（构建时捕获凭据） | `sampler/src/attribution.rs` 已有 `Auth401AttributionCallback`（callback 注入，比上游跨 crate 依赖更解耦） |
| 413 图片剥离重试（扣预算不受阻） | `sampler/src/retry.rs` 已有同语义 |
| GROK_EXTRA_CA_BUNDLE | grow 走 OS 信任库（rustls-native-certs/platform-verifier）；私有 CA 场景由系统信任库/`SSL_CERT_FILE` 覆盖；上游该能力服务于其托管 gateway |
| UsageLimit（云端 billing）tab | grow 无 billing/credit_balance 概念（零引用），/usage 为本地 token 统计 |
| telemetry OTLP / mixpanel | xAI 商业遥测，无需求来源 |
| foreign-sessions 发现 | 读取第三方产品状态，无需求来源 |
| workspace-daemon / diag-server / preview-proxy | grow 无 workspace-server daemon 架构 |
| bundle 缓存 | 上游 grok.com 发布式 bundle；grow 的 plugin-marketplace 是不同模型 |
| conv/<id> 分支绑定 | grow 已有 worktree + workflow-workspace 所有权体系 |

## 2. T8 评审：FitRung 五级阶梯 vs 本地 pre-prune 阶梯

**结论：不补"丢最老 history turns"级。** 本地结构已覆盖上游五级的全部能力，且语义保留严格更优。

对照：

| 上游 FitRung 级 | grow 对应物 | 结论 |
|---|---|---|
| verbatim | 不裁剪直传（现有路径） | 等价 |
| 丢最老 history turns | 无直接对应——grow 用 **summary 路径**（LLM 总结保留语义） | 不补。上游敢丢是因为其历史 server 托管可重拉；grow 历史是本地唯一副本，丢弃 = 永久信息损失。总结路径语义保留严格优于丢弃 |
| 前缀裁剪超大 tool result（max_bytes = tokens×4） | `common/compaction/prune.rs` pre-prune 阶梯（model-free，`plan_tool_result_pruning`）+ `item.rs` `truncate_payload_for_compaction` | 已有 |
| 丢最老 step turns | summary 路径的输入裁剪 | 已有 |
| emergency 硬缩最新项 | `item.rs` emergency tail shrink + `truncation-recovery.md`（D1-D8） | 已有 |

上游五级是**纯 token 拟合阶梯**（server 代理硬约束驱动）；grow 是"model-free 裁剪 → 语义总结 → 紧急兜底"三层，约束来源不同（本地持久化 vs 代理 413），结构不能直接对照移植。若未来出现"总结路径本身超限"的可观察案例，再沿 `run_compact_inner` 的 emergency 分支评估。

## 3. T9 评审：work_policy / response_guidelines 模板重组

**结论：不重组模板。** 本地 `prompts/foundation/mandatory-core.md` 的分层已覆盖上游重组的语义：

- 上游 `<work_policy>` 的核心语义（按可逆性/影响面权衡、授权边界、保护用户工作）已在本地 `<action_safety>` 完整表达，且更精确（"One approval is not blanket approval"、"Preserve work that may belong to the user"）。
- 上游 `<response_guidelines>` 的"禁止自创缩写/术语"是弱形式的增量：本地 `<output>` 已有 "prefer accessible language over filler, repetition, or unnecessary jargon"，缺"只用对话中已建立的词汇"这一强约束。

可选增量（非本清单任务）：如未来观察到代理自创术语，在 `<output>` 补一行 "do not coin abbreviations or terminology; use only vocabulary already established in the conversation"。

## 4. T1 重定位：readOnly 标注 → ToolKind 审计

**已核实**：grow 的 `ToolConfig.kind: Option<ToolKind>`（30+ 变体）+ `workspace/src/capability.rs` 的 `CapabilityMode`（ReadOnly/ReadWrite/Execute/All 偏序）+ `kind_allowed` 穷尽 match + `ALL_TOOL_KINDS` 编译期守卫，**已覆盖且强于**上游的 `read_only: bool` 两级标注。照搬上游字段会制造第二权威来源，违反单一权威来源原则。

**T1 实际任务**：审计内置工具 kind 声明完整性（找出语义应为 Read 类但 kind=None/Other 的声明并修正）+ "所有内置工具 kind 非 None"契约测试（MCP/custom 工具 None 是合法例外，需在测试中显式列举）+ 三条权限路径（主 agent All、subagent capabilityMode、workflow subagent）围栏消费一致性验证。

## 5. T2 重定位：StopCancelledReason → 复用 CancellationCategory

**已核实**：grow 的 `shell/src/session/event_types.rs` 已有 `CancellationCategory { HookDenied, PermissionRejected, PermissionCancelled, PermissionTimedOut, MidTurnAbort }`——turn 取消分类的权威来源已存在。上游 `StopCancelledReason` 中的 MaxTurns/NoProgress 是 gateway 概念，grow 无对应。

**T2 实际任务**：hooks 宏表加 `StopCancelled` 事件（Observe-only），payload reason 字段直接序列化现有 `CancellationCategory`（取消路径若缺失用户显式取消的分类，补 `UserInterrupt` 变体）；emit 点在取消分类已知处（`tasks_cancel.rs` 附近）。
