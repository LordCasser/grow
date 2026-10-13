## Context

动机与范围见 [proposal](proposal.md)。以下是现状证据，不表示拟议行为已经实现：

| 入口 | 现状与缺口 |
| --- | --- |
| `crates/codegen/shell/src/extensions/session_admin.rs::handle_prompt_status` | 只查运行 handle，再等待 SessionCommand 响应；没有独立的调用期限。 |
| `crates/codegen/tools/src/implementations/grow_build/task/backend.rs::inspect/query/cancel` | 通信失败与不存在共用 `None`/NotFound，不能用于死亡判断。 |
| `crates/codegen/pager/src/app/root/dispatch/turn.rs::poll_stalled_prompt_submissions` | 遍历顶层 agents；子页面和无 current_prompt_id 的 child 生命周期没有完整覆盖。 |
| `crates/codegen/pager/src/app/root/dispatch/task_result.rs::PromptStatusResolved` | Running 下 Unknown/Err 刷新观察时间；结果没有保留 session binding 和运行实例归属。 |
| `crates/codegen/shell/src/session/handle.rs`、`actor/spawn.rs::SessionThread` | 已有 residency、实例 owner 与 actor/persistence drain，适合持有运行观察。 |

现有 `Cancelling turns reconcile against authoritative prompt state` 允许查询错误后退出 cancelling。本 change 保留输入可编辑性，同时明确查询失败不等于取消已完成、不能自动派发下一项。`Session observation does not repair durable projections` 和完整 writer drain 契约继续有效。

## Goals / Non-Goals

- 在有限等待内给出有来源的状态；保持持久 outcome 与临时观察分别可见。
- 观察不依赖被观察的长操作完成；不以查询动作触发恢复写入。
- 本项不补写 terminal、不替换 writer、不自动取消长任务。关闭与修复由后续 change 的 owner 执行。

## Decisions

### 1. 以原有 owner 的快照表达事实

在 Session 生命周期 owner 和 subagent Coordinator 的既有记录中维护紧凑快照。字段分为：执行阶段、已确认提交位置、交付阶段、最近成功观察及实际进展、错误分类/阶段。未知值显式保留。快照只含 identity、计数、时间和受限诊断，不复制模型输出或工具参数。

SessionActor/runner 在阶段变化及退出时向 owner 更新快照；owner 也观察 actor/runner 的退出或 panic。读取快照不能排在模型、工具、hook 或持久化 ACK 后面，相关长操作仍在原 owner 管理的工作任务内完成。无需新 supervisor 服务或持久健康日志。

快照中有更新的 confirmed frontier 时只前进；实例结束后的残留注册表不得重新证明它仍在运行。进程重启后先标观察未知，再加载可验证持久事实。

### 2. 查询分类与身份

查询结果区分 Found、Absent、Unavailable、TimedOut、InvalidEvidence。Absent 必须注明不存在的范围，例如 runtime instance 不存在并不能证明 durable session 或 input 不存在。授权失败按既有安全边界返回，不泄漏其他会话是否存在。

请求/结果保留 session ID、可用的 child/prompt identity、Pager binding epoch 和 writer incarnation。查 child lifecycle 不强制要求 current_prompt_id；prompt 级查询必须匹配 prompt。首次观察未知 incarnation 时由当前绑定采用；后续不同 incarnation 需经明确换代/重新绑定再采用。迟到响应不能清除新一代的 in-flight 标记。

无运行 handle 的信息读取通过现有只读 storage observation 获取 session metadata 和已验证 Timeline 摘要，返回来源与限制，不构造 executor、运行 hook 或争抢 writer lease。文件读取使用有界 worker，超时后未结束的 worker 继续持有许可。

### 3. 有界观察与独立新鲜度

默认控制查询总期限为 5 秒，包含并发许可等待和回复等待，采用调用方更早的期限。Prompt 与 input-admission 状态 RPC 共用 4 个工作许可；超时释放客户端等待，但后端未退出的 RPC 继续持有许可及目标登记。目标登记最多 8 个（包含许可等待者），同一目标最多一个实际在途或等待中的查询。每轮 watchdog 最多发出 4 个 Prompt 查询；未入选或仍被旧 worker 占用的候选者设置 250 ms 的短重试时间，避免大树反复遍历时形成无界待执行队列或忙轮询。目标登记会在真实 RPC worker 退出后才释放，因此 replay 使旧查询 generation 失效时也不会重复启动同目标 RPC。轮询继续按现有 agent/tree 顺序扫描，已在途目标和有短重试时间的目标自然让出后续候选者；打开当前子页面可优先调度。

保留现有 submitting/cancelling 短窗口和 running 30 秒失活窗口。分别维护 last_confirmed_observation、last_work_progress、next_probe_at；Unknown/错误只影响 backoff，不刷新前两者。重复失败退避并封顶，推送只保留最新快照，避免消息或定时任务随失败次数增长。

不把 heartbeat 当作工作进展，也不以无输出判定死锁。UI 的状态可解释性期限和 provider/tool 的运行期限分别管理。

### 4. Pager 收敛规则

复用根和嵌套 AgentView 的目标定位，统一执行核对 reducer。页面显示“执行中”“执行已返回，提交受阻”“已完成，交付待确认”“状态暂不可确认”等与证据一致的描述；没有具体等待证据时不回退为模型等待。

取消查询失败后可编辑草稿、浏览记录、重新核对或再次请求停止；保留取消意图与目标身份，不调用成功 finalizer、不自动 drain 下一条输入。只由同目标的 canonical terminal 关闭任务。瞬态运行健康消息不使用 durable Finished、事件 cursor 或模型上下文权威。

## Risks / Trade-offs

- 快照可能滞后 → 展示来源、时间和实例；有界查询不能保证底层工作在期限内结束。
- 通知通道或整个服务失联 → 客户端依自身单调时钟进入未知状态，推送不承担唯一恢复责任。
- 活跃会话只读扫描较慢 → 先返回 owner 快照；文件读取按预算执行，失败不变为“记录不存在”。
- 改变查询结果形状 → Shell、tools 和 Pager 同一 change 更新，不维持混淆语义的兼容分支。

## Migration Plan

先完成 typed query 和 owner snapshot，再接入 Pager 全树核对及取消路径。持久格式无需为本项引入健康记录。恢复收尾能力随后由第二项接入这些观察字段。归档本项只确认观察闭环；不能据此宣称原故障已经可以自动补交结果。
