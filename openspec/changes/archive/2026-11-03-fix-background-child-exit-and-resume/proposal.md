## Why

双 Ctrl+D 正常退出必须保全运行中的 subagent。前序调查已找到 notification Turn 的 viewer 标志被旧退出代码误作 runtime 所有权、跳过挂起的缺陷；需要交付并验证从该触发条件到新进程原身份续跑的完整修复。

## What Changes

- 确认 Quit 统一经过 runtime 挂起确认，不从逐回合 viewer 标志判断是否可以直接退出；保留失败后的 retry 和显式 force。
- 增加真实 PTY 回归：后台 child A 完成唤醒 parent 的通知回合时，child B 仍运行；双 Ctrl+D 仅挂起，冷 resume 自动继续原 B。
- 覆盖普通和 minimal 模式、持久 checkpoint/原身份/唯一交付/命令不重放，并登记 CI。
- 构建本次可运行程序，执行相关回归和 OpenSpec 校验，补充开发者说明。

## Capabilities

### Modified Capabilities

- `client-surfaces`：明确宿主通知回合被标为 viewer 时仍必须确认 runtime 挂起。
- `session-timeline`：明确通知回合与另一运行中的后台 child 一起挂起、按原身份恢复。

## Impact

Pager 退出入口、既有树挂起/恢复集成、PTY 测试和 core-regression。复用现有生命周期、等待和 writer 所有者，不增加状态表、恢复协议或配置开关。不改写用户旧 cancelled 终态。
