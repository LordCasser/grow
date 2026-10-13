## Why

Input was not confirmed 只能说明客户端未取得确认。Pager 重发会生成新 prompt_id，而 Shell 当前另行生成 input_id，底层的输入幂等能力尚未形成贯穿 RPC 的身份关联。此外，TTL 清理虽检查 writer lease 和 activity，但这些条件不足以保护原实例已退出、收尾尚未闭合的 session、artifact 与 worktree。

## What Changes

- **BREAKING**：将原 prompt identity 稳定关联到持久 input identity 及不可变请求内容，未知 ACK 后的核对与重试复用原身份；冲突 payload 明确拒绝。
- 恢复草稿保留原提交身份和接纳不明状态；不会自动作为新输入重发，已接纳输入不重复执行接纳 Hook 或再次排队。
- 将未闭合子任务链及未交付结果所需资源纳入清理资格判定；在实际删除边界重新验证身份、引用及并发发布条件。
- 存储写入被确认阻塞时，关闭受影响范围的新执行准入，保留诊断、取消和修复能力；重开准入需要重新验证可用 writer。
- 限制待恢复内容的内存、扫描和并发占用，明确尚未持久化内容在进程退出后的恢复限制。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `input-admission`：跨 RPC 的输入身份、未知接纳核对与 Hook 恢复边界。
- `session-timeline`：恢复资源保留、删除资格及存储故障下的准入边界。
- `client-surfaces`：未确认草稿展示、显式重发语义及清理保留反馈。

## Impact

涉及 Pager prompt/draft，Shell input admission、Timeline 输入事件、session/artifact/worktree 清理与准入调用方；实现时更新开发说明。沿用现有本地草稿存储、Timeline 引用及目录能力校验，不新增独立恢复数据库。

依赖 [observe-session-runtime-health](../2026-10-08-observe-session-runtime-health/proposal.md) 的 typed observation 和 [recover-subagent-terminal-commit](../2026-10-08-recover-subagent-terminal-commit/proposal.md) 的收尾责任与恢复前缀。三项按此顺序实施、验证和归档，本项是整体恢复方案的必要闭环。

不自动删除用户修改、扩大现有磁盘清理范围或承诺满盘断电下零数据丢失。用户主动销毁历史的语义维持既有授权边界；自动维护不能把它当作隐含授权。本次仅建立提案。
