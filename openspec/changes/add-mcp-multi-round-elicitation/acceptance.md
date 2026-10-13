# MCP Acceptance Matrix

以下是交付验收矩阵。SDK 阶段已完成 M-24 的本机依赖/API 回归，M-25 探针已执行并发现阻塞；产品交互仍未验收。结果与限制见 [SDK 核查](sdk-integration.md) 和 [verification](verification.md)，不能将探针通过计为产品场景通过。

M-01～M-04、M-09～M-11 优先用 MCP wire/Shell owner 夹具；M-05～M-08、M-12～M-13 覆盖 Pager/PTY 和数据检查；M-14 及真实服务用例记录实际协议/transport。

M-15～M-23 为架构复审补充。M-17 按模型直达/use_tool、direct（有/无 session）、headless 和子会话入口逐项记录；未支持的组合验证明确拒绝，不能从另一入口通过推断支持。

M-24～M-25 是实现前 SDK/依赖适配门槛；不能只用 Grow 自写 mock loop 证明兼容，也不能把所需 SDK 扩展点当作已经存在。

| 编号 | 操作/故障注入 | 可观察的通过条件 |
| --- | --- | --- |
| M-01 | 一个 `tools/call` 连续返回两轮不同 Form | 只有一个模型 tool call/最终 tool result；wire id 各轮不同；答案只送原 key |
| M-02 | 同轮含两个 input request | 两者都可完成；展示顺序稳定，响应 map 不错配，不提前发送缺少必要答案的 continuation |
| M-03 | 输入空必填、非法数字、越界枚举、超大 schema | 提交被阻止或明确失败；后端验证同样拒绝；界面不崩溃 |
| M-04 | 分别点击拒绝询问、取消询问、停止调用 | 返回语义可区分；停止调用清理它的全部 waiter，不取消其他调用 |
| M-05 | URL accept 后服务端仍返回 input_required | UI 仍显示等待；不会产生伪完成；手动检查/取消有效 |
| M-06 | URL 使用 `javascript:`、userinfo、控制字符或欺骗性主机 | 不静默打开；目的地展示明确，危险输入被拒绝 |
| M-07 | 三个并行调用请求输入，第四个触发容量策略 | 无旧请求被无声替换；顺序和容量行为可验证；每个 responder 只结算一次 |
| M-08 | 权限、计划审批与 MCP 表单同时出现 | 权限/现有焦点规则仍成立；原输入草稿完整；后台 session 不夺焦点 |
| M-09 | 用户填表超过单轮网络 timeout | 表单不会被误判为网络超时；明确的人类等待截止仍可终止 |
| M-10 | 取消后送达旧答案；服务重连复用 RPC id | 旧答案被拒绝/忽略，不归属新 service，不打开迟到 URL |
| M-11 | 写工具响应丢失、transport 替换 | 结果显示失败或未确认；不会无条件重复原写操作；旧错误不 reset 新 transport |
| M-12 | UI 断线、Shell 存活；再测试 Shell 被杀 | 两种情况分别处理；不能把旧界面卡片当作持久恢复证据 |
| M-13 | 记录日志/遥测/Timeline 后检查产物 | 看得到关联身份与结果；看不到 token、opaque state、未授权的表单正文 |
| M-14 | 旧 stdio、现代 HTTP、HTTP-auth、ACP 各运行夹具 | 只声明已通过的能力；短启动预算和 discovery fallback 有明确结果 |
| M-15 | 多次 state-only 返回，随后有 Form 但缺 state；重复 key；穿插重连/手动检查 | 退避与总期限有效，wire 不沿用旧 state/答案；取消后不再派发 |
| M-16 | 同实例相同/冲突答案重试，手动检查与自动检查竞争 | 同内容返回原状态，冲突拒绝；每个 logical call 至多一个 continuation 在途 |
| M-17 | 检查 initial/continuation metadata；无交互路由；同轮掺入未支持 sampling/roots | capability 属于该调用；不借用别的会话；整轮在收集/打开前失败，不隐式采样或泄露 roots |
| M-18 | 单个/累计 message、schema、URL、state、答案超限；整轮项数超容量；队中超期 | 各预算边界与合法临界值可验证；无部分接纳和残留 waiter；未显示项也能超期/取消 |
| M-19 | 同 call 两个 key；新轮接纳后旧 guard drop；URL 等待后另一 Form 到达 | 实例独立、不误删；URL 保留检查/取消而不挡 Form；等待时 actor/其他调用可响应 |
| M-20 | 等待时取消 owner、禁用工具、替换 transport 或关闭 storage gate | 回答和检查不派发业务 continuation；资源按原 owner 清理，不重新打开准入 |
| M-21 | fixture 记录 initial/continuation 副作用后断连接、TransportSend 或 HTTP MCP error | 不进入 recover-and-retry；副作用计数不因恢复增加；已确认未发送与结果未确认分别报告 |
| M-22 | false、0、可选未填、非法强制转换、多选枚举、未知 schema | UI/后端同样接纳合法值、拒绝非法值；默认值未经确认不发送 |
| M-23 | MRTR 最终返回 structuredContent + image 或 isError；检查中间 payload 去向 | 原结构化/图片/错误语义和预算保持；中间答案/state 不成为普通模型输入或第二份结果 |
| M-24 | 选定 rmcp 后核对 cargo metadata/tree、feature、lockfile 和直接 trait 依赖；运行 SDK 适配编译及旧 SSE/取消故障 fixture | 统一 mcp::rmcp 入口；reqwest/http/sse-stream 类型兼容；no-provider/ring 与 OHOS 约束保持；无无依据重复版本、双重取消或叠加退避 |
| M-25 | 使用实际 SDK 自动 helper/ClientHandler 跑并行同 key、逐轮取消/期限、整轮接纳、state-only/手动检查和 gate 关闭 | 核实 SDK 扩展点能承载 Grow owner/准入；无法实现的组合记录上游 API 缺口并阻止开放，不用自写协议栈绕过 |
