## Context
result_tx.closed 与 shutdown 在通知 hook select 中都返回 false，随后 break。ACP 等待阶段已有正常下一轮行为。

## Decision
closed 分支直接 continue，自动释放本次 pending guard；其他失败分支不改。把原启动块原样移到私有函数供生产和测试共用。

## Validation
真实 worker 先接收已取消请求，再处理第二个有效问题，由 mock ACP 返回 Cancelled；验证第二个响应完整到达。
