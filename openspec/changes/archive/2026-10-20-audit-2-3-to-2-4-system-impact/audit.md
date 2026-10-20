# 2.3 → 2.4 系统影响审计

本轮新增确认 **2 项待修问题：1 项 P1 恢复后再次挂起丢失等待、1 项 P2 未派发任务阻塞 scope 切换**。此前已修的 3 项不重复计数。执行 91 项既有回归与 2 个反例探针，全部达到各自预期；1 项真实来源检查未启用。完整方法、失败修正与覆盖限制见 [verification.md](verification.md)。

## 固定范围

| 版本 | Commit | 提交时间（+08） |
| --- | --- | --- |
| v2.3.0 | ef665ec41eac7c6335658f12392cee0525889acd | 2026-09-30 16:27:23 |
| v2.3.1 | 7b7d56ab98a686223bc659567c79c1363729cd66 | 2026-10-03 10:26:47 |
| v2.4.0 | cd5d88ec7f5d5c42ee18277304c352232dcb4e14 | 2026-10-19 13:05:28 |

2.3.0 → 2.3.1：2 个提交、44 个文件、+3,770/-699；2.3.1 → 2.4.0：11 个提交、634 个文件、+86,708/-8,085。合计 diff 661 个不同文件、+90,433/-8,739，包含测试、文档与规范；分段文件有重叠。不能把这些统计写成全量审查覆盖率。

本轮从 tag 差异出发，并在 HEAD 02432cb6 加本会话已完成挂起/Goal 修复的工作区核对当前可达性。上一轮确认的树冻结身份、节点意图前等待、自动 Goal 快照问题均已独立修复，不再列成此次新增问题。

## 新确认问题

### A1 · P1 · 恢复放行后、原人工等待重绑之前再次挂起，丢失逻辑等待

**实际路径：** `suspension.rs:695` 先写 ExecutionReleased，随后释放 AgentTask 的启动信号；恢复的物理 worker 需再调度才会建立新 Interaction::Opened。此时持久 checkpoint/内存 continuation 仍明确记录 AwaitPermission/AwaitUser，而 registry 尚为空。

若新挂起在新 attempt 建立前获胜，`suspend_node` 在 `:1054` 清空原 cursor.operations，再从 Timeline/registry 重分类。当前折叠在 ExecutionReleased 处清除旧 attempt，尚无新的 Opened；调用已有 ToolStarted，于是 `:1134` 进入 OutcomeUnknown，并写入工具结果。第二次 checkpoint 成功却丢失原人工等待。

**反例：** `resuspend_probe.rs` 使用真实 guard、首轮超时重试、全新 ChatState/EventTracker/SessionActor、真实 prepare/commit/release。只扣住一次 restored worker 启动信号来模拟“已持久放行、worker 尚未被调度”的窗口，再发起新的挂起。Permission、Question、PlanApproval 全部观察到 `AwaitPermission/AwaitUser → OutcomeUnknown`，同时产生此前不存在的 ToolResult。探针不是实际操作系统进程 E2E；调度屏障是明确的故障注入，不把它声称为真实用户操作录像。

**版本归属：** 挂起实现由 `2e69e5db` 引入，v2.3.1 无此模块；v2.4.0 用易失 `pending_interactions.snapshot_attempts()` 并同样清空 cursor.operations，在该窗口也没有新 attempt。当前已修复的同 operation 失败重试与意图前等待，并未覆盖恢复后新 operation 的这一窗口。历史版本未重新编译运行；归属依据固定 tag 源码，当前缺陷依据可执行探针。

**影响与最小方向：** 快速恢复后再次退出，原问题/审批可能不再重建。下一项修复应保留 checkpoint 明确尚未完成的逻辑等待，直到真实回答或工具结果替代，允许新 attempt 接管；不能简单保留全部历史 Suspended，否则会复活已解决等待。需专门覆盖恢复 release → 还未 rebind → 新挂起，以及正常新 attempt、真实终态优先级。

### A2 · P2 · 明确未派发并已释放义务的 task 永久阻塞工作范围切换

**实际路径：** `LocalPlan::mark_link_undispatched` 记录 dispatch_failure，按契约保留 `terminal=false` 且没有 result。用户/Agent 通过有理由的 Released review 处置 required delivery 后，completion blockers 已为空，`validate_transition_from` 的新 scope 检查也明确豁免 dispatch_failure。但 `actor/planning.rs:786` 的 SelectScope 仍用 `any(!link.terminal)`，因此拒绝该合法切换。

**反例：** `planning_scope_probe.rs` 调用真实 Undispatched、Review Released 和 SelectScope 命令。验证该 link 确未派发、没有伪造 terminal/result，completion blockers 为空；新 scope 的 LocalPlan transition 验证通过，actor 却返回 `dispose old executions, waits and mandatory deliveries before switching scope`，且旧 plan 保持不变。

**版本归属：** 同一 `2e69e5db` 的任务规划实现引入，v2.4.0 和当前工作区均有该条件；不是最近挂起修复引入。由于 dispatch_failure link 不能合法转成 terminal，换 scope 的阻挡无法靠等待 child 完成消除。

**影响与最小方向：** 一次明确派发拒绝可使当前会话一直无法选择新工作范围；普通继续工作仍可进行，不是整个会话失效。修复应让 actor 复用/对齐现有 LocalPlan 义务判定，继续阻挡真正活跃或结果不明的执行，覆盖 required/optional link、有无 Released、仍活跃与 ACK 未确认，不伪造子执行完成。

## 变更对整体的影响

| 系统面 | 2.3 → 2.4 的变化与跨模块影响 | 本轮核查 |
| --- | --- | --- |
| 2.3.1 历史与 Replay | Timeline 原通信锚点 → Shell 缺失历史补齐 → Pager 普通/child/离线表面；合成历史走 full replace，不能冒充 live receipt 重投 | 读取通信恢复实现与已归档证据；定向回归另列 |
| Session tree | 从物理退出切换到持久挂起，root/coordinator/child/Workflow/writer 共同保留原身份；运行实例释放与业务终态分离 | 重点检查冻结、checkpoint、ExecutionReleased、下一次挂起的组合边界 |
| Plan / Goal | retained candidate 与 Plan runtime 分离，submission 身份跨切换保留；Control 是冷恢复依据 | 与上一轮三项修复交叉核对，未把已修问题重复计数 |
| 局部 TODO / task | TODO 成为 Timeline 权威；parent link、replan receipt、required delivery、依赖等待共享同一 LocalPlan，但执行仍属 coordinator | 检查明确未派发、交付释放、切换 scope 的 owner 交接；核对旧更新确认不清除新责任 |
| 输入与 Skill | 提交身份、不可变 payload、整组 Skill 原文与 Input Consumed 接纳；权限/取消/体积预算先于模型采样 | 核对实际 caller 资格、持久确认失败、同源恢复和部分发布；运行已有针对性回归 |
| MCP | client generation、工具目录与 meta policy 热更新；不确定 tools/call 禁止自动重放 | 核对 stale discovery/retirement、替代 transport 的身份条件、单次 dispatch；MRTR 在本版本明确暂缓 |
| 询问协调 | 来源询问 durable admission + 启动中查询身份；取消与重试不得创建另一个 inquiry | 对照新增契约与既有测试证据，本轮不声称重跑真实跨机通信 |
| 压缩/图片/缓存 | 后台 compaction owner 排空与用量结算；图片能力按实际 route、持久描述复用；cache 缺口按 canonical source 核对 | 核对取消保留 owner、既有恢复预算和来源身份；全 provider/全设备矩阵未运行 |
| 版本与存储 | Session、Control、Workflow 格式更新，版本说明明确不做旧格式迁移 | 这是已声明的产品边界，不把旧格式不兼容误报为新缺陷；没有修改旧会话文件 |

## 已排除的具体疑点

- 重规划发送方可能再次携带 Received 而非 Dispatching，但接收入口 `receive_plan_guidance` 在生成不可变通知正文之前统一为 Received。没有据此确认“同身份异正文”缺陷。
- 子节点完成/拒绝派发会把待处理 replan 置为 Unapplied；父级旧 handled 确认只能推进对应身份，不会替换新 revision。相关现有回归纳入本轮验证。
- MCP `try_call_tool` 在调用入口只执行一次 `call_tool_cancel_aware_until`；reset 受 originating client/service 身份约束，准备的是下一次独立调用。未发现该路径恢复后暗中重发原调用。
- Skill 的输入正文取自已接纳 immutable payload，source selection 与当前资格分别核对；部分准备失败不直接发布已准备的半组正文。

## 历史证据与覆盖限制

已有 2.3.1 的 Replay/communication history 验证、2.4.0 的 Shell 4,254 与 Pager 7,504 library 测试、6 个 PTY 收尾场景，是当时归档记录，不等于本次运行。参考 `2026-10-01-restore-ordered-resume-communications/verification.md`、`2026-10-19-prepare-v2-4-0/verification.md`、`2026-10-19-finalize-v2-4-0/verification.md`。

Atlas `release_runtime_restore` scoped search 返回单文件 structural 完整定位；调用方与阶段顺序仍以源码核对，不把局部查询作为全仓证明。本轮没有宣称逐行审完 90,433 行新增内容，未跑所有平台发布、真实远端 provider/MCP 服务、MRTR、全磁盘故障和进程崩溃组合。
