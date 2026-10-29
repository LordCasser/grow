## Why
用户明确要求闭合前轮留下的四项：低于本地估值的 provider 校准、冷恢复首个请求前的 Goal 作用域、普通/技能请求输出预留差异、恢复 usage 快照到 Pager 的整体验证。

## What Changes
可信完整 provider 回执成为上下文锚点，包括低于 heuristic 的值；校准保留一份 anchor/local 参考，防止压缩后负差分饱和变为假零。冷恢复从持久化控制授权初始化 Goal 请求作用域并在恢复协调后同步。普通和技能请求统一使用已校准输入加 post-clamp 显式输出上限，无显式上限时不虚构后端默认值。恢复 ledger 通过实际快照和 Pager 处理器验证 lifetime/segments/cache/incomplete/迟到消息，修复实测缺陷。

## Impact
ChatState 校准与作用域、Shell 冷恢复和最终请求准入、Pager 快照消费及相关回归。保留全部已有工作区修改。每个修复复用现有事实与入口，不引入第二个账本。
