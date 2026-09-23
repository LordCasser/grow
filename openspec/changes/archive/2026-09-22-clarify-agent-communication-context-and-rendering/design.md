## Context

当前事实见 [pipeline.md](pipeline.md)。ask 在目标 InfoRequest Sideband 使用冻结 Surface；目标主上下文不追加问答。send_subagent_message 从 parent 进入直接 child 的 durable inbox，Received 后 ACK，Consumed 后才进入目标 Surface。调用方工具输入/结果照常留在调用方上下文。

现有 TUI 已有工具原位更新、接收记录、Unicode 预览、Normal/Minimal 和恢复去重。Markdown 缺口在 tracker/notice 将 typed 正文压成 plain details，及 Other/Notice 统一走 `for_plain_text`。回执缺口是 Coordinator 丢弃真实 receipt ID、发送错误被 String 化、Pager 根据文案猜 unknown。

## Goals / Non-Goals

目标：简洁英文 UI；可靠的接收 ACK；意见作为有来源的特殊结果进入双方上下文；Markdown 可读且原文可取回。

不增加第二套消息存储、讨论页面、消费 ACK、自动协商或任意 peer 写权限。ask 不自动写入目标上下文。用户确认 ACK 只表示接收，不等待模型响应。

## Decisions

### 1. 两类操作，回复继续用 send

[意见交换设计](exchange-design.md) 定义目标行为。ask 保持只读 Sideband；send/reply 走正常收件与安全步骤。双方各自保留出站调用/回执和入站意见，不重复注入自己的发送正文。

接收消息无法作为孤立 ToolResult 使用：现有 portable projection 会丢弃它。第二阶段增加有 runtime 来源的 agent-message context item，由 receipt 消费原子生成，再在 request projection 展开为专用 call/result 对。它不是模型发起的工具执行，也不带人类权限证据。不能只改 UI 名称声称已经满足。

### 2. 回执复用已有事实

[回执设计](receipt-design.md) 定义 ID、ACK 时序、错误/取消竞争、内部只读核验和恢复。只返回真实 Received；未确认与拒绝分开，UI 不解析错误文字判断事实。新的 reply 也使用同一 ACK 规则。

现有 active route/turn gate 是新投递门槛；查旧回执独立做 ownership 校验，不为查回执重新启动或写入目标。未确认结果不自动重发；新 tool call 是新操作。

### 3. UI 服从现有 Grow

[详细 TUI 设计](tui-design.md) 是本轮有效方案。去掉上一版常驻路由解释、每行详情按钮、固定 Body/Raw/Data tabs 和新增 Y 复制键。

默认只保留工具/动作、参与方、状态和有界正文；ask 完成显示 Q1+A2，message 显示两行。英文状态短而准确。按需 Data 保留原字段，正文不讲 Sideband/上下文/投递模式。

复用现有 MarkdownContent、BlockViewer、ShortcutsBar、主题与真实键盘入口。原文保留 typed body/answer，不能以展开 tab 后的 MarkdownContent::text() 为复制权威。代码、表格和有界 Mermaid 复用现有渲染。

### 4. 身份和恢复不新增所有者

source 用原 tool-call ID；incoming inquiry 用 `(sourcePeerId,inquiryId)`；message 用 `parent-message:<receipt ID>` 或新消息来源下对应的稳定 receipt key。UI eventId 不是消息 ID。

Timeline/inbox 负责收件、消费和精确 context item；Session/Coordinator 负责授权和调度；request projection 负责 provider 表示；Pager 负责显示。显示失败不否定 durable receipt，replay 不重发或重采样。

消息与既有 Task/Monitor 通知同批出现时，消息按各自 Received 顺序组成一个 AgentMessage 批次，普通通知继续使用原消费表示。每一组的 Consumed 与精确输入在同一事实中提交；两组不构成跨类型事务，也不承诺所有通知类型的全局正文顺序。第二组提交失败时，第一组已消费，恢复只处理仍 pending 的普通通知，不重发消息。这保留了既有通知边界，避免为本次通信改造所有通知类型。

只读 child 的未 authored 发送工具仅获得 `None` 的 reply 调用能力；初始发送仍需要 `Write`，并受直接 ownership 限制。实际回复还须经 runtime 核对收到的 receipt，不因可见工具或正文自称来源而获得授权。

Minimal inquiry 等自己的终态再打印一次；不可变 message receipt 只显示一次；真正的 reply 是新消息，不是旧发送行的“任务完成”。

## Implementation sequence

第一阶段：英文简洁通信行、独立 Markdown section、原文/按需数据、receipt ID 与 typed outcome。保持现有父消息消费表示，验证 UI 与 ACK 边界。

第二阶段：agent-message context item、专用配对工具结果、reply_to 路由授权和安全步骤调度。验证 source/target 请求、provider adapters、Surface 坐标、compaction、恢复及模型输入去重。该部分与纯渲染分别验证。

## Risks / Trade-offs

- Received 不等于进入 provider 请求或完成任务：只承诺持久接收。
- 直接追加 ToolResult 会丢配对：canonical agent message 与 provider 对分开，所有 adapter 必须测试。
- Sideband 答案与前台决定可能不同：不自动作为正式回复写入双方。
- reply 扩展旧的 child 上行约束：只允许已有收件关系中的意见回复，不允许越权/interrupt。
- source 历史输出没有 receipt_id：保留已知 received，缺字段不伪造；未知失败保守显示 Unconfirmed。
- 新 input 类型影响稳定坐标与 compaction：是第二阶段的发布门槛，不用浏览器示意代替验证。

## Migration / validation

本方案已获用户批准并进入实施；tasks 与 verification.md 记录完成及验证状态。现有历史事实不重写；新 schema 不双写旧协议，不降低已有严格加载要求。交互示意只验证 UI 层级，运行时场景见 delta specs 和 tasks。完整核对记录见 [verification.md](verification.md)。
