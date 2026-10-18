## Context

Grow 固定 HEAD 使用 rmcp 2.2，现有 PendingKind 为 Permission/Question/PlanApproval。上游固定版本使用 rmcp 3.4，已具备 MRTR、RequestTracker、Shell bridge 和 TUI Form/URL；其 inbox 与 pending UI 存在单槽替换，不是可直接采纳的完整 FIFO。

当前工作树有109条原有状态记录，输入恢复/观察/存储准入改动不等于 HEAD。实现前须重新核对在制接口；本 change 不修改或承诺这些在制改动已发布。规范和代码证据映射见 [review](../archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

## Goals / Non-Goals

目标：一个逻辑工具调用经过多轮 Form 或 URL 后结算一次；并发不串单，取消不假称撤销外部效果，用户能识别数据提交目的地。

非目标：通用 workflow 引擎、自动填秘密、长期后台授权、跨进程自动重放、全量 stdio modern 协议迁移、统一 Dock。stdio 保留实际验证的既有握手；HTTP modern discovery 优先，HTTP-auth/ACP 的支持按互操作测试声明。

## Decisions

### Upstream SDK owns protocol machinery

**先升级官方 `rmcp` 并适配依赖，再接 Grow 产品交互；不自研 MCP 协议栈或 MRTR 协议循环。** `mcp` crate 继续作为唯一集成入口并 re-export `mcp::rmcp`，下游不再各自依赖不同版本 SDK 或复制 wire 类型。

初始核查的 [rmcp 3.4 MRTR API](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.4.0/crates/rmcp/src/service/client.rs) 已提供 `call_tool` / `call_tool_with_mrtr_max_rounds`，通过 `ClientHandler` 处理嵌套请求，负责 state 回传、响应聚合、轮次上限与 state-only 退避。后续选定的 3.5.1 保留这些机制；discovery、协议协商、metadata/headers 编码及新旧服务端协议兼容也由 SDK 提供。Grow 负责选择能力、注入自身调用上下文和验证最终可观察行为，不重复实现序列化、版本分支或第二套重试/退避算法。

3.4 是原设计的参考版本，实际版本和验证结果以下方 3.5.1 记录为准。自动 helper 内部聚合本轮请求；handler context 的 id 来自 input key，token 在嵌套上下文创建，不能当作 Grow 原 tool_call_id 或原 turn 取消 token。M0 必须用最小集成 fixture 核对并行调用归属、逐轮取消/网络期限、整轮预算/能力检查、等待期间 gate、URL 手动检查与 SDK 自动轮次的接入点。**SDK 有 MRTR 不代表这些宿主约束已经接通。** 若选定版本缺必要扩展点，先记录精确 API 缺口并采用上游已提供的扩展或推进上游修复/版本升级；不能退回自写 JSON-RPC/MRTR 循环来绕过。后续“loop/continuation owner”均指 SDK 操作在 Grow 内的宿主所有权，不是自研协议实现。

依赖兼容依据 [已有 dependency-workarounds 记录](../../../docs/architecture/dependency-workarounds.md) §1/§5 和当前 `mcp/Cargo.toml`：

- Grow 已统一 reqwest 0.13；升级时核对 SDK 对 reqwest、http、sse-stream 及 Rust toolchain 的要求，调整直接依赖与 lockfile，避免 trait 类型来自不兼容版本。不能复制上游业务仓库中隔离 reqwest 0.12/0.13 的历史方案。
- 保持 `default-features = false` 和按需 feature；`reqwest-tls-no-provider` / `rustls-no-provider` 加已有 ring provider 是 OHOS 构建约束。不能照抄业务上游的默认 feature、`reqwest`/`rustls` 而重新引入 aws-lc-rs，也不为 URL elicitation 顺带引入完整 OAuth 产品栈。
- 复查既有 `McpHttpClient` SSE 限流和取消守卫是否仍必要：用原故障 fixture 验证新版 SDK。上游已解决的补丁删去；仍需宿主约束则只保留薄适配，并记录原因，避免 SDK 与 Grow 双重取消或叠加退避。

这里的兼容是依赖/trait/feature/SDK 协商适配，不是额外建设兼容框架；没有证据时不虚构一个独立“兼容 crate”。复用标准库、现有 URL/schema/异步 Runtime，只有确需且经核查的直接依赖才进入清单。

2026-10-11 M0 实核已选择并精确固定官方 rmcp 3.5.1，完成依赖/API 适配。实际 helper fixture 证明自动续接和 HTTP discovery 可运行，同时复现调用级能力被覆盖、整轮缺预检、嵌套 token 不关联 owner、drop 不通知活跃 wire 四项限制；网络期限/手动检查/派发前 gate 也缺公开接点。详见 [SDK 集成核查](sdk-integration.md)。交互接入门槛未通过，生产入口暂不开放现代能力，先解决上游扩展。

### One logical owner, multiple wire requests

MCP crate 持有 SDK 调用与原操作的关联上下文；Shell 原工具 owner 负责取消和终态；pending registry 管理实时 waiter；Pager 仅投影和提交回答。协议 continuation 由上游 SDK 驱动，没有第二个 turn owner。

关联至少能验证 session、turn/执行 owner、原 tool call、server 配置/transport generation、本轮 RPC id、input request key 和交互实例 id。上游 mcp-elicit-UUID 是交互 id，不能直接冒充原模型 tool_call_id。新 service 复用 RPC 数字时也必须与旧请求分离。

initial call → input_required → awaiting user/external → continuation → complete/error。每轮使用新的 wire id，requestState 原样回传，仅服务原调用；本轮没有 state 时不沿用上一轮。协议要求的 continuation 不等于网络失败后的盲重试。

一轮只能有一个 continuation 在途；自动检查和手动检查共用同一执行资格。回答按交互实例与轮次单次接纳：完全相同的重复返回原接纳状态，冲突回答和旧轮次回答拒绝。下一轮即使复用 key，也不能复用上一轮答案。先校验整轮 request map 和容量，再发布交互；不支持的 method、模式或超限使整轮明确失败，释放本轮资源，不先打开其中的 URL。

现有 `try_call_tool → recover_and_retry` 会在 TransportClosed/TransportSend 和部分 HTTP MCP error 后重发原参数。这不是 MRTR 的安全性依据。新 MRTR 路径的 initial/continuation 请求一旦可能发送，网络错误、业务/协议错误不授权重发或回到 initial call；transport 恢复仅服务未来新调用。只有可证明未发送时才可按原身份重试，不能以 `readOnlyHint`、相同 requestState 或新 wire id 推断幂等。旧协议路径的通用重试另项审计，不顺带改写全部恢复系统。

### Capability belongs to the requesting operation

2026-07-28 每个 initial/continuation request 的 `_meta` 都必须携带协议要求的版本、客户端信息与真实 capability；不是仅在初始化时全局声明。能力由原调用可用的交互 bridge 决定，不因另一个会话打开了 Pager 而提升。首版仅为可验证有人机路由的会话工具调用声明 Form/URL；direct `grow/mcp/call`（包括无 session 调用）、headless/无人值守入口没有该路由时不声明，也不借用当前可见会话。子会话必须保留自己的 owner 和既有路由，无法交付则明确不支持。

同一 MRTR loop 校验所有 input method；本 change 不新增 `sampling/createMessage` 或 `roots/list` 能力，不自动枚举本地根目录或发起采样。支持矩阵逐项记录协议版本、transport、调用入口与 Form/URL/取消能力；保留旧 stdio 已验证路径是限定本次迁移范围，不建设两套交互实现或通用兼容框架。

### Forms and URL consent

Form 使用规范支持的简单 schema，TUI 验证和后端验证一致；默认值仅预填，提交需人明确确认。禁用秘密字段请求，不自动从本地文件或模型补齐未展示内容。

schema 范围以官方受限表单类型为准，包含单选和多选枚举；不能把不支持的字段降级成无约束文本。必填缺失与可选未填区分，`false`/`0` 不是未填，空值和字符串/数字不能被静默强制转换。M0 固定支持表和边界 fixture；不引入通用 JSON Schema/UI 引擎。服务端字段名/描述不能可靠证明自由文本不含秘密，已识别的凭据请求必须拒绝，同时保留人工审阅和拒绝入口，不能宣称自动识别全部秘密。

URL 先显示实际服务来源、完整可检查目标及规范化主机，再由人同意打开。accept 只代表同意外部交互；只有服务端最终结果能完成调用。提供手动检查、重新打开和明确取消；收起 waiting 卡片不伪造完成。

拒绝危险 scheme、userinfo 和会改变目的地的控制字符清洗；使用结构化 opener。建议默认 HTTPS，localhost HTTP 开发例外在实现前明确。不得预取 URL，opaque state、token URL 和表单正文不默认写普通日志/模型上下文。

### Queue and focus

在原 pending 管理下设置有界 per-session FIFO，容量在真实场景确认后固定。活跃卡片不被新请求替换；溢出返回明确结果；取消只清理同 logical call 项。服务端同轮 map 按稳定顺序显示，答案仍按 key 对应。

现有 registry 是 `HashMap<tool_call_id, PendingKind>`，只有实时元数据，不持有可重连表单正文或答案 owner。MCP 多项交互使用独立实例身份关联原 call；旧 guard 的 drop 只能移除它仍拥有的实例，不能只按原 tool id 删除新轮次。一次扩充现有 runtime 管理即可，不同时新建 Shell 和 Pager 两个有权取消 responder 的队列。重连所需 payload 仅由原 live owner 有界保留，不转成 durable 答案仓库。

FIFO 约束待处理输入的接纳/选择；URL 同意后的外部等待不独占键盘或阻塞后续 Form。数量限制同时覆盖一轮 input 数、会话待处理项，并限制 message/schema/URL/requestState/答案的单项和累计字节；从入队开始计等待期限，不能轮到展示才重新计时。等待不持有 actor admission、MCP state 或 transport 恢复锁，取消、回答及其他会话继续可调度。

沿现有权限优先级和集中键盘 owner 裁决，不抢后台会话焦点。只让当前交互拥有者保存/恢复草稿，焦点移动与取消必须区分。非交互客户端不宣称支持无法处理的模式。

### Cancellation, time and recovery

区分单轮网络 deadline、人类等待和 URL 外部等待；等待用户不消耗单轮网络 timeout。外部 state-only 等待使用有界退避和总期限，具体数值不是标准要求。

取消有活跃 wire 时按原 peer/id best effort 发 cancelled；没有活跃 wire 时关闭 continuation 和所有关联 waiter。结果只说明取消请求/本地等待已终止，不承诺服务器回滚。

迟到错误不得 reset replacement transport；超时及未知结果不自动重放写调用。UI 重连只重建仍由当前 owner 持有的交互，若无法证明活跃则明确失效。首版进程重启显示中断或结果未确认，不能从遗留 requestState 自动续跑。需要跨进程继续时另行定义服务端有效性、身份和幂等证据。

每次 continuation/检查派发前重新核对原执行 owner、transport generation、工具启用/授权资格和 storage admission；等待期间关闭的准入不能被“只是续接”绕过。Form decline/cancel 是本 key 的协议回答，停止整个调用才清除其余项。新的轮次、手动检查及 UI 重连均不重置该 logical call 的总期限。终态投影保留既有 structuredContent、isError、图片/附件预算与 direct 完整结果边界，不能让 MRTR 分支退化为纯文本结果。

### Timeline and HumanIntent

Form/URL 回答是原工具的 typed interaction response，不是 Prompt/Steer/Followup，不再运行一遍 UserPromptSubmit Hook。pending/resolved 仍是实时投影；持久记录只包含足以证明原调用关联和结果的最少事实，不引入第二套敏感答案仓库。

实现前明确新事实能否由既有 tool 记录/引用表达；需要新增事件时由 chat-state Timeline 单一权威管理，并按实际 schema 规则验证，不先把假定类型写进 v25。原调用只发布一份最终 tool_result。

## Risks / Trade-offs

- SDK升级会影响握手/API；必须同时回归旧 stdio 与已支持 HTTP。
- 上游 discovery 后续还有短启动预算修正，不能只摘取早期提交。
- 串行展示不等于串行工具执行；队列必须能关联并行请求而不饿死其他交互。
- URL consent 不等于完成，也不授予 MCP server 广泛工具权限。
- 当前在制输入/存储准入接口可能变化；未知 ACK 不能触发新 prompt 重发。

## Delivery and Validation

M0 选择一个 Form 场景和一个 URL 场景，固定入口/能力矩阵、schema 支持表、各层字节/数量预算和期限；M1 打通受限端到端闭环；M2 完成并发/生命周期；M3 验证真实服务。M1 的内部 fixture 也必须满足身份、无盲重试、取消和有界等待，不把这些推迟到开放 UI 后。M1 不能单独宣布一般可用。

验证分 MCP wire fixture、Shell owner/取消测试、Pager 交互测试和真实 PTY/服务互操作，详见 acceptance。SDK 阶段已运行实际 mcp fixture 与 cli 编译；Shell/Pager 交互和真实服务仍未实施，详见 verification。后续按实际符号运行定向测试。

## Open Questions

真实服务及操作由用户后续指定；SDK 版本、依赖兼容和宿主 API 接点需先验证；队列容量/等待期限需用例支撑；localhost HTTP 例外需明确。上述问题不妨碍立项，但相关功能公开前必须解决，不能把建议当用户已选定的策略。
