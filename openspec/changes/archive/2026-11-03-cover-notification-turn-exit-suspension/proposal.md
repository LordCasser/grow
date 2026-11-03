## Why

前次调查确认 shutdown 取消了五个 child，但仅用进程启动时间解释，没有定位普通双 Ctrl+D 如何绕过挂起。进一步核对发现：subagent completion 引起的宿主通知回合会将前端标为 viewer，旧退出代码却用这个逐回合展示标志判断进程是否拥有运行工作。

## What Changes

- 通过生产 ACP 通知入口和两次真实按键分派，补充“通知回合变为 viewer 后仍必须挂起”的回归。
- 临时恢复旧退出 guard，确认同一测试失败；撤回该实验 guard 后确认通过。
- 补充准确的事故触发链和开发者说明，区分已证实的代码机制与未持久化的 UI 瞬态。

## Capabilities

没有新契约或运行时行为改动。现有退出所有权修复已移除错误 guard；本次只补跨入口回归和调查说明，`skip_specs: true`。

## Impact

Pager 的既有退出测试、开发者说明与本 change 记录；不修改用户会话或重新执行任务。
