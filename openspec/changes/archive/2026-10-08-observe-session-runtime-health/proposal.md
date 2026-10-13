## Why

会话执行实例退出或提交链受阻后，持久 session 仍可能存在，但查询接口只报告 unknown session，Pager 则继续显示 Waiting for response。现有查询将不存在和通信失败合并，部分等待没有期限，子页面也未完整纳入 watchdog，用户无法判断执行、提交和观察分别停在哪里。

## What Changes

- **BREAKING**：session/prompt/subagent 状态查询区分已确认状态、确认不存在、不可达、超时及无效证据；调用方不能再把 `None` 或普通错误解释为任务死亡。
- 用现有生命周期 owner 提供有期限、可失效的只读观察，分开执行、持久提交、交付和观察新鲜度；运行 handle 缺失时仍可读取授权范围内的持久 session 信息。
- Pager 覆盖根与嵌套子页面，按 session、prompt/child、绑定 epoch 与运行实例核对响应；失败不刷新最后成功观察时间。
- 明确取消查询失败只结束界面阻塞，不确认取消完成、不推进下一项执行；更新既有 cancelling 契约。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`：运行观察与持久事实的边界，查询结果分类及身份归属。
- `client-surfaces`：子页面状态核对、过期观察、取消反馈和无 handle 会话信息展示。

## Impact

涉及 Shell session 生命周期、状态扩展接口、subagent backend，以及 Pager effects/reducer/子页面状态；实现时同步更新 `docs/development.md` 的诊断说明。无需新增持久状态库。

这是三项工作的第一项，无前置 change。后续 [recover-subagent-terminal-commit](../2026-10-08-recover-subagent-terminal-commit/proposal.md) 使用本项的观察语义；[preserve-uncertain-input-and-recovery-resources](../2026-10-08-preserve-uncertain-input-and-recovery-resources/proposal.md) 使用它区分输入接纳不明和明确拒绝。

范围限定为状态观察闭环；写者重建、子任务收尾、输入去重与清理保留分别由后续 change 负责。本次用户请求仅建立提案，实施任务保持未完成。
