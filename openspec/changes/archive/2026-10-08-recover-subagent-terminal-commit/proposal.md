## Why

子任务执行返回后，输出 artifact、child result、父用量、parent Ended 或完成回执都可能失败。目前 runner/coordinator 会把提交失败折成执行失败并移入普通完成缓存；恢复又可能把查询不可用解释成中断取消。磁盘满还会使旧 writer 失效，直接重试旧 handle 无法完成收尾。

## What Changes

- **BREAKING**：任务查询和等待区分执行 outcome、收尾未确认、正文不可用与交付未完成；收尾失败不再伪装成新的执行 outcome。
- 在既有 Coordinator 中保留唯一收尾责任，统一正常、error、panic、cancel 路径；未确认项不进入可直接淘汰的普通完成缓存。
- 复用 Timeline、writer lease 和通知 inbox 按已提交前缀恢复；ACK 不明时核对原 identity/payload，writer 失效后先完成旧实例 drain 再取得新所有权。
- 为收尾观察和回执等待设置期限与恢复额度，保留仍在运行的底层操作所有权；不由本地提交失败重发 provider 或工具。
- 保留已提交 outcome、真实终态来源及用量不完整性；迟到结果按原 owner 交付，取消或换代的 Goal/Workflow 不被重新唤醒。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`：子任务收尾所有权、exact reconciliation、恢复终态、正文可用性与回执交接。
- `model-sampling`：收尾超时沿用 attempt 结算边界，禁止因本地提交失败重新生成。
- `behavior-goal`：迟到子任务结果保留原 Goal identity/revision，不重开已关闭准入。
- `workflow-execution`：等待未确认不成为可重放的 child 成功结果，也不触发重复派发。

## Impact

涉及 tools task coordinator/backend/types，Shell subagent runner/reconciliation、Session writer 生命周期、completion delivery 及 Goal/Workflow 调用方；实现时更新开发说明。保留现有存储权威和 exclusive writer 协议。

前置为 [observe-session-runtime-health](../2026-10-08-observe-session-runtime-health/proposal.md)。后续 [preserve-uncertain-input-and-recovery-resources](../2026-10-08-preserve-uncertain-input-and-recovery-resources/proposal.md) 补齐跨清理路径的保留门槛；本项完成不意味着自动 GC 已保护所有恢复资源。

本项不引入通用任务调度器、第二份 journal 或磁盘容量保障系统，不重写 provider 的既有重试政策。本次仅建立提案，实施与故障注入尚未执行。
