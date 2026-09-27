# 主/子 Agent 权限链核对

| 阶段 | 主 Agent | 子 Agent |
| --- | --- | --- |
| 资格 | 注册工具、Behavior、RWX 和策略交集；显式 deny 优先于 always-approve | 创建时的 Agent exact identity、不可扩大的 RWX ceiling 和 MCP binding；`review-required` 工具逐次进 Gate |
| Mode | 会话 live `Ask / Auto / AlwaysApprove` | 创建时独立冻结 `[subagents].permission_mode`，默认 Auto；主会话的 live mode 不广播 |
| Auto | 安全启发式可直接允许；未解决请求通过主会话分类 Sideband；分类失败/超时转人工提示 | 初始 RWX 内普通调用可快速通过；锁定调用的 Auto 使用主会话任务上下文 Sideband；判断失败/超时拒绝当前工具且不弹人工提示 |
| 人工提示 | ACP `request_permission`，交互默认 60 秒、非交互默认 10 秒；超时结束当前 turn | 同一 ACP Gate 按 child session 路由；超时只失败当前工具，child 可继续 |
| 执行 | 已允许调用继续正常工具执行 | 允许只签发冻结参数、cwd、epoch 和 identity 绑定的一次性 permit；dispatch 再验，不修改 ceiling |
| 审计 | PermissionEvent 记录终态，原始 detail 不持久化 | 同一事件经 primary audit bridge 成为脱敏 UI-only 子 Agent 权限块 |

## 故障归因

截图中的 `The primary agent permission judgment timed out` 对应 `PermissionManager` 的 child Auto classifier unavailable/timeout 分支，不是人工 ACP 提示超时。子 Agent 默认 Auto，与主会话界面显示的 `always-approve` 独立。旧分类 worker 从出队才开始 30 秒，首次 provider attempt 固定只分一半；较慢模型会在总期限尚未耗尽时被中断。Sideband admission/结算又可能越过该期限。

本 change 将分类请求入队时间交给主会话 worker，统一约束全流程，默认期限 60 秒，首次尝试获得四分之三的当时剩余时间。`SidebandRun::Drop` 的已有终态修复继续处理超时取消；回执接收方关闭后，迟到完成不能形成允许或 permit。人工提示期限及主/子超时后续保持原契约。

## 分拆债务

`PermissionManager` 仍在一个 actor 内串行等待外部分类和人工提示，导致其他权限请求、mode 更新排队。并发重构涉及 remembered grant 提交顺序和 prompt lifecycle，不属于本次超时修复；验收方向记录在 `openspec/backlog.md`。
