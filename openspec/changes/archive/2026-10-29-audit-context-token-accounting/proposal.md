## Why
用户要求重新全面分析上下文管理、token 管理和计数。审计确认三种口径已分开，但新反例发现 Goal 作用域与图片缓存生命周期耦合，切 route 后可把当前长指令错误 shadow，压力从 46K 降为 36。

## What Changes
完整记录请求投影、provider 校准、usage、cache、恢复与展示链路；将已有 active_goal 字段从 ImageBudgetSelection 迁到请求状态，清图片缓存不再清 Goal 请求作用域。补 route replacement / image invalidation 的失败反例与后继作用域变化测试。其余未证实问题和策略差异只记录，不改账本、输出预算或近似算法。

## Impact
ChatState 内部状态与请求构建、上下文回归和规范说明。没有新持久化字段或重复作用域记账；独立债务登记 backlog。
