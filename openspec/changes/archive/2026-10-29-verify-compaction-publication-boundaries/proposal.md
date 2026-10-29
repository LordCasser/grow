## Why
前轮全链路审计保留三个未实测组合；用户明确要求继续故障注入，发现缺陷直接修复。已有契约要求 durable ACK、取消后不发布、异步完成由后继请求投影给出，但尚缺覆盖 promotion waiter drop 后重复提升、terminal ACK 丢失与 post-commit repair、route 变化的组合。

## What Changes
使用现有 actor、Timeline、mock provider 和持久化测试设施补充三组回归。确认违反现有契约的实现后，修复原 owner/发布入口，不增加第二个压缩状态机、持久化队列或重试墙。明确重复等待同一个 generation 与真正取消 generation 的区别。

## Impact
Shell compaction owner、请求投影通知、ChatState/Timeline 持久化边界及相应测试。前序修复和无关工作区改动保留。未发现问题的分支只增加测试和验证证据。
