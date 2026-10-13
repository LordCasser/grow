# 新增提案架构复审

日期：2026-10-11。结论：两项能力方向符合 Grow 原则，但原稿不足以直接进入实现；本次已把职责划分和关键失败路径补入各自未来契约。只改文档，没有证明产品能力已实现。

完整的“上游/现有 Grow/新增 Grow”划分见 [实现责任表](implementation-boundaries.md)。本报告的源码行号对应本次工作树，HEAD 为 `7b7d56ab98a686223bc659567c79c1363729cd66`，不代表源码全部来自该提交。

## 主要发现与处理

| 优先级 | 原稿盲区与后果 | 当前证据 | 已补全 |
| --- | --- | --- | --- |
| P1 | “实现 MRTR loop”未约束 SDK/依赖责任，容易重复协议机制，或照搬业务上游 feature 引回构建问题 | `mcp/src/lib.rs` 单一 SDK 入口；`mcp/Cargo.toml:10` no-default/no-provider；dependency-workarounds §1/§5；官方 rmcp 已有自动 MRTR | 优先 SDK helper/handler、兼容依赖和最小 feature；M-24/25 核对 SDK 接点，缺口先由上游解决 |
| P1 | 原 retry 可能在服务已执行后重新派发，新 MRTR 不能仅在外面加循环 | `mcp/src/servers.rs:1910` 错误分类、`:1946` try_call_tool、`:2009` recover_and_retry；已有测试含 HTTP 错后 calls=2 | MRTR 协议续接与错误重试分开；M-21 记录可能已发送的 initial/continuation，不以 transport 恢复证明可重放 |
| P1 | 一个 tool id 的 pending 元数据不足以容纳同轮多项、迟到 guard 和重连 payload；URL FIFO 可能挡住后续输入 | `shell/src/session/pending_interaction.rs:30` HashMap；`:106` insert、`:140` remove | 实例身份、同 owner 清理、有界 runtime payload、整轮接纳、URL waiting 释放焦点、单次回答/派发；M-16/18/19 |
| P1 | 跨 Behavior 保留违反现有 Control 校验，defer 未规定解除 tool-loop 后的实际行为 | `behavior.rs:137` PlanRuntime、`:216` runtime_fields_match_selection、`:305` select_behavior、`:684` finish_plan；`actor/tool/preparation.rs:1392` disposition | 同 owner 的独立候选生命周期；defer 提交后 EndTurn，无自动执行/重新审批；P-17/18 覆盖 commit 崩溃窗口和冷恢复 |
| P1 | 稳定决定身份缺少 payload 冲突和超前查询的负面证明；跨周期/历史复制可能复活授权 | `actor/tool/mod.rs:925` handoff 按 hash/revision 去重；`input-admission` 已明确 unknown 不等于未接纳；fork/rewind 是独立已有入口 | action/feedback 不可变；未知决定先核对；来源 session 的审批不可复制；P-19/20/21/23 |
| P2 | canonical 读取未覆盖异步迟到、旧版评论；只保护 blob 仍可能被整 session GC 删除 | `pager/.../plan.rs:77` 同步 legacy 读取；`behavior.rs:945` 8 MiB artifact 边界；已归档 destructive maintenance 契约 | 复用 artifact helper、有界异步查看与请求归属；评论留在原版本；目录/剩余引用一起保护；P-22/24 |

另外补齐：逐调用 capability 与无交互入口、unsupported nested method、等待期间权限/存储 gate、state-only/缺失 state、typed false/0/多选枚举、最终 structured/image/isError 结果保全。它们是当前能力的闭环，不新增 sampling/roots、OAuth 平台或通用表单框架。

## 原则判断

- **如非必要，勿增实体**：保留一个候选引用有必要，不能复用会随 Behavior 清空的 runtime；提交/正文/决定身份分别解决周期、内容和重试问题，但都归同一 owner，不扩成三个服务。
- **优先 Runtime/官方 SDK**：MRTR/协议协商交给 rmcp，Grow 实现自己独有的授权、交互和生命周期。计划已有执行交接无需外部框架，更无需再造一份。
- **一个权威入口**：MCP 仍经 mcp crate；计划仍经 SessionControl/Timeline。Pager 是投影，legacy plan.md、pending registry 和原始回调都不是持久权威。
- **最小闭环**：MCP 先证明 SDK 接点和依赖兼容，再接 UI；Plan 先修 canonical 来源，再闭合 defer/决定/恢复。不能让 M1/P2 开放能力后才补身份、取消或恢复。
- **债务拆分**：旧协议普通 MCP call 的宽泛 retry 另记 backlog；不在本次重构恢复系统。Memory、Dock、Remote/Agent Host 和通用 Workflow 不纳入。

## 核对范围与限制

阅读了当前主规范相关条目、两项提案全部文档、Shell/MCP/Plan/Pager 调用入口及既有测试位置。Atlas scoped 查询用于定位，但其结果出现与当前工作树不符的行号和未命中，最终证据按直接源码核对；没有声称全仓调用图完整。

协议依据复核自 [MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr)、[Elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation) 和固定官方 SDK 源码；未扩大原 `2bdd1d6` 主题评审范围，未宣称它是 remote latest。使用 Luna 子代理进行两轮只读格式/映射/矛盾检查，架构结论以源码核对为基础。

原有 M-01～M-14 增为 M-01～M-25，P-01～P-16 增为 P-01～P-24；共补充19条验收行，并补强既有要求。所有产品实现与验收仍未执行。真实服务、精确 SDK/依赖选择、容量/期限和具体 Control schema 仍是实施前任务，不因本次文档通过而自动确定。
