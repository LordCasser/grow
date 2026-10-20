## Context

`SessionActor::suspend_node` 先提交 `CommandsSettled`，再将每个后台命令的结算结果送入 Notification inbox，最后保存 Runtime checkpoint。`NotificationSource::TaskCompleted` 是终态事实；Timeline 只接受 `NotificationSourceVersion::Ordinal { value > 0 }`。当前挂起分支传入 `Opaque`，与常规任务完成路径不一致。

## Decision

挂起分支使用 `Ordinal { value: 1 }`。同一 task identity 的终态只产生一个逻辑通知，重复挂起沿现有 receipt 去重与持久 `CommandsSettled` 复用，不增加新的状态或重放命令。

## Verification

核对修正后的 source/version 与 Timeline 校验分支一致；运行现有命令结算重试、通知持久化测试及 OpenSpec 严格校验。真实挂起端到端恢复仍需在使用新二进制的进程中验证，当前存活的旧进程不会热更新。
