## Context

见 [proposal.md](proposal.md)。取消是 ACP notification，没有响应体；Pager 当前只给 Submitting/Running 启动 prompt-status watchdog，`TurnCancelling` 在 shell actor 已消失时没有任何收敛来源。首次 load 则先切到临时 Agent，失败处理只解绑 session id，导致可见 composer 没有提交目标。

Usage 的真实主调用已经以 `sampling_usage/attempt_settled` Observation 持久化并带 attempt 去重键，但 `ChatState::from_timeline` 只恢复去重表、不重建账本。子 Agent fold 和 session incomplete 仍是纯内存事实。

## Goals / Non-Goals

**Goals:**

- 保持 Timeline/actor 是终态与累计 Usage 的权威所有者。
- 让取消与 load failure 都回到明确、可继续操作的 UI 状态。
- 让 lifetime、segment、model 与 Agent 四个视角来自同一批结算事实。

**Non-Goals:**

- 不公开逐 Agent Usage UI，也不改变 prompt response/headless 的既有 shape。
- 不把 elapsed timeout 当作伪终态，不在 Pager 中估算历史 Usage。
- 不顺带修改 session writer drain、磁盘空间管理或 subagent 生命周期策略。

## Decisions

### 1. `TurnCancelling` 复用 prompt-status watchdog

取消动作记录现有 reducer-owned status anchor；两秒后发起单个权威查询。Terminal 走 first-wins finalizer，Running 只刷新 anchor，Unknown/查询错误说明 session/prompt 已不可用并恢复 Idle。相比新增 cancel request/response 协议，这复用已有 exact prompt identity、去重和错误处理，改动更小，也覆盖终态通知丢失。

### 2. 临时 load Agent 自带返回 surface

首次 load 创建 Agent 时保存当时 `ActiveView`。成功后丢弃该返回信息；失败时完成 replay 清理后删除临时 Agent并恢复原 surface。原地 reload 没有该标记，继续使用现有 reload-window 事务。相比让无 session id 页面提交时自动新建 session，这不会把用户输入错误地发送到一个新会话。

### 3. Usage 从持久结算重建，而非保存可变快照

恢复时按 Timeline 顺序 fold 已有 main attempt settlement、新增的 child settlement 与 incomplete Observation。结算以 attempt key 或 `subagent_id` 去重并拒绝冲突；aggregate、model、agent 和 segment 在同一 fold 中更新。相比周期性写完整账本快照，事实事件更小，且不会引入快照覆盖/双计费问题。

### 4. segment 边界属于成功的冷 actor resume

冷 load 创建并完成 actor 恢复后、向客户端确认 load 前，shell 请求 chat-state 持久提交 resume boundary；resident reconnect 不写边界。UsageLedger 同时维护 lifetime aggregate 与有序 segment，所有后续 writer 同步 fold 到 aggregate 和当前 segment。

### 5. 分段只进入专用 session-usage 响应

`grow/session/usage` 返回 lifetime `PromptUsage` 加 `segments`。normal 状态通知继续只带 lifetime `PromptUsage`，prompt response/headless 不承载 segments 或 Agent 分项。Usage modal/Minimal `/usage` 使用专用响应绘制 lifetime 与每段摘要。

## Risks / Trade-offs

- [旧 Timeline 没有子 Agent 详细结算] → 不猜测 model/cache/cost；升级后的结算完整持久化，旧主 attempt 可直接恢复，旧子 Agent 仅保留既有终态粗粒度 token 证据而不伪造分项。
- [取消中的 shell 仍正常工作但超过两秒] → Running 响应只刷新窗口，不结束 turn；查询单飞且低频。
- [load 在 boundary 提交后、响应前断线] → actor 仍存活时重连不重复分段；若 actor 同时死亡，下一次冷恢复可能看到一个空段，UI 如实保留该 incarnation 而不合并消费。
- [Usage 事件持久化失败] → 与模型 attempt 相同地 fail closed；不先更新内存再声称累计成功。

## Migration Plan

无需离线迁移。恢复代码直接识别既有主 attempt Observation；新版本开始为子 Agent、incomplete 和 resume 写入新 Observation。回滚只会让旧版本忽略这些 Observation，不修改模型 Surface。
