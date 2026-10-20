## Context

CommandsSettled 已提供稳定命令证据；Received 索引在消费后仍保留。人工等待已有 Interaction Opened/Resolved 事实，但 suspend_node 每次读取临时 registry，owner 退出会清除它。

## Decisions

1. 按 TaskCompleted source + Ordinal(1) 查询既有回执，存在即复用；不可用仍报错。没有回执才提交挂起正文，不吞掉异正文冲突。
2. 在当前 operation 的 SuspendRequested 边界上重放 Interaction：Opened 建立原 attempt，普通 Resolved 关闭它；仅当前挂起边界后的 Suspended 保留逻辑等待。之前已结束的 attempt 不复活，新 attempt 替换旧 attempt。分类与最终悬挂结算均使用这些持久事实，不新增易失缓存或事件 schema。
3. 仍以真实 tool_result/turn 终态为准；恢复沿已有 checkpoint/prepare/commit/release 路径，等待重新绑定前不得伪造完成或重放副作用。

4. 人工问题 worker 与权限/审批 owner 一致，以 runtime_suspend 判断 Suspended；前台接收器先关闭不能因后台服务 token 尚未取消而记成 Cancelled。

## Verification

跨阶段测试覆盖已有完成通知的 pending/consumed、新结算通知、owner drain 超时、同 operation 重试、恢复及 Permission/Question/PlanApproval。验证终态通知最多一份、原等待保留、无 OutcomeUnknown/tool_result。对已回答/旧挂起 attempt 补反例。保留无 checkpoint 的冷恢复失败边界。
