## Why
前轮 ACK 测试使用内存 sink，不能证明真实 JSONL、OS writer lease 和进程中断后的恢复。用户明确要求实测 replacement 已落盘但 ACK 未确认时的重启路径。

## What Changes
使用隔离目录、真实 SessionPersistence/JsonlStorageAdapter、ChatState durable 入口和独立子进程，验证摘要前后、replacement、Control 重投影与 Completed 的持久化切点。原进程由父测试强制结束，恢复由另一进程执行正式 writer load 和 actor 恢复入口。发现违反契约的根因后才改产品实现。

## Impact
压缩进程恢复测试、必要的直接缺陷修复及开发验证说明。保留现有未提交修改，不扩展为通用故障框架或新的持久化状态机。
