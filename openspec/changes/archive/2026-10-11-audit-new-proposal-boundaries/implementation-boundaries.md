# 待实现 change 的实现责任

核对日期：2026-10-11。审计开始时只有两个待实现行为 change：MCP 多轮交互、保留计划。`record-upstream-review-2bdd1d6` 是已完成任务的纯文档 change；本审计也是纯文档，不算第三项产品实现。backlog 的 Remote/Agent Host 等没有启动，不扩入本次。

## MCP：SDK 实现协议，Grow 接宿主语义

| 事项 | 应复用谁 | Grow 要做什么 / 不应做什么 |
| --- | --- | --- |
| MRTR wire 类型、state 回传、响应聚合、轮次上限和退避 | 官方 rmcp 自动 helper + ClientHandler | 接用户响应和原调用上下文；不另写 MRTR loop、serde wire 类型或第二套退避 |
| discovery、版本协商、metadata/标准 headers、新旧 MCP 服务端兼容 | 官方 rmcp | 选择真实 capability、配置预算并验证；不自己写协议协商/fallback 框架 |
| SDK 升级的 trait/feature 兼容 | Cargo 与现有 mcp crate | 核对 reqwest/http/sse-stream/toolchain/lockfile；维持单一 `mcp::rmcp` 入口、no-provider TLS/ring/OHOS 约束 |
| SSE 重连和取消 | 优先 SDK；已有薄适配需复验 | 上游修好即删除对应 workaround；仍有宿主需要则保留最小适配，不能双重取消/叠加退避 |
| schema、URL 与异步机制 | SDK typed schema、标准/现有 URL/schema 库、Tokio | 实现表单控件、后端验证、用户同意和展示策略；不写通用 JSON Schema/URL parser 或外部 OAuth 产品栈 |
| logical call 与交互身份 | 现有 Shell tool owner、MCP transport identity、pending 管理 | **Grow 新增**同调用多项/多轮关联、单次回答和实例清理，不能把 SDK input key 当原 tool ID |
| 并发队列、焦点、草稿、URL waiting | 现有 Pager 会话路由与交互管理 | **Grow 新增**有界排队、外部等待让出焦点和对应 UI；没有两个可独立结算 responder 的队列 |
| 取消、权限/存储 gate、冷恢复与 Timeline | 现有 Grow owner/准入/结算 | **Grow 接通并补边界**，每次 SDK 业务派发受宿主资格约束；只保存最少事实，不建持久答案仓库 |
| 模型结果、typed 图片、direct 结果 | 现有 MCP→tool_runtime→Shell 投影 | 回归现有语义，不为 MRTR 写第二套结果路径 |

最需要纠正的是原任务“实现 MRTR loop”的含义。官方 [rmcp 3.4 README](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.4.0/README.md#multi-round-trip-requests) 已说明自动驱动；本次改为先完成 SDK/依赖适配，再接通 Grow。此版本只是固定 API 证据，不声称是当前最新或无需验证的最终选择。

API 缺口不能隐藏：固定 [service/client.rs](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.4.0/crates/rmcp/src/service/client.rs) 中自动处理藏在 helper 内，嵌套 context 使用 input key 和新 token。已有文档不足以证明能直接获得原 Grow call/每轮取消/整轮预检。M-25 因而是实现前的接入验证门槛；缺口通过官方扩展、上游修复或版本升级处理，不在 Grow 重写协议。这些是宿主集成待核定项，不是已证明上游 bug。

依赖兼容有本地历史依据：[dependency-workarounds](../../../../docs/architecture/dependency-workarounds.md) §1/§5 记录 reqwest 0.13 统一与 TLS/OHOS 限制；[取消设计](../2026-09-23-cancel-abandoned-mcp-tool-requests/design.md) 记录为何 Grow 曾用精确 peer/id 守卫。不能直接搬业务上游 Cargo feature 或沿用对旧 rmcp 的 workaround 结论。

## 计划：Grow 产品语义，复用现有 Grow 基础设施

| 事项 | 分类 | 本次边界 |
| --- | --- | --- |
| 四阶段、正文 hash/大小/编码校验、不可变 artifact | 已有实现 | 使用 behavior/artifact helper，不新建计划数据库或文件索引 |
| 冻结 snapshot CAS、恢复审批、live ResampleStep、idle PlanHandoff | 已有实现 | 接通新入口并扩充身份，不重复实现执行引擎或 scheduler |
| 不依赖原 waiter 的候选引用及跨 Behavior 资格 | **Grow 新增** | 同一 Control/Timeline owner 中的独立生命周期；更新 snapshot 校验，不放宽所有非法 runtime 字段 |
| 保留/defer、再次审阅、修改/放弃/批准区别 | **Grow 新增** | typed 决定、持久接纳后 EndTurn；没有第五 PlanPhase 或独立 Workflow |
| 决定幂等、未知 ACK、替换竞争 | 已有 Control 模式 + **Grow 扩展** | 不可变身份/action/feedback，核对原决定；不经普通 prompt 假装幂等 |
| canonical 预览、查看请求归属和版本评论 | 已有 viewer/异步读取 + **Grow 扩展** | 接一个后端权威读模型，去掉 legacy plan.md 权威；不新增一组模型工具 |
| GC、fork/导入/rewind | 已有所有权路径 + **Grow 扩展校验** | 引用保护覆盖整个 session；历史正文不携带执行资格；不新建 GC/历史引擎 |
| Goal、Rhai Workflow、工具权限 | 保持既有 owner | Plan 的批准不自动激活 Goal、生成 Rhai 或扩权 |

不需要外部“计划工作流框架”。外部库只承担标准文件、哈希、序列化及异步机制；“谁批准哪个提交、何时结束等待、恢复后能否执行”是 Grow 的语义，不能交给通用 SDK 猜测。

## 实施顺序与结论

两项方向符合单一 owner、最小闭环和复用原则，但前提是按上表拆分责任。MCP 先过 SDK/依赖适配门槛；计划先复用 canonical 读取和已有恢复测试，再闭合候选/defer/决定，不先造 UI 缓存权威。原上游评审记录没有产品实现任务；无需为它增加运行时代码。

保留必要的身份和不变量，不扩成通用交互/工作流平台。仍需实施前确定真实 MCP 用例、SDK 版本/API 接点、容量期限，以及 Plan 的具体 Control schema/API；本次不以未经验证的默认值替代这些决定。
