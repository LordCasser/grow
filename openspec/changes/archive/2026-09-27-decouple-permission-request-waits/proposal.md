## Why

权限等待存在两层全局队头阻塞：PermissionManager 在请求分支内等待模型或人工回应，主 SessionActor 的分类 worker 又等待完整 Sideband 后才接收下一请求。Sideband 本身并不要求这些串行等待。Goal 的 owner settlement fence 还把正在正常运行的主模型 attempt 当成独立权限 Sideband 的前置依赖。增大超时只会放大这些等待，之前人为延迟 20 秒的 mock 测试没有验证并发调度。

## What Changes

- 权限命令入口持续处理控制命令，每个请求独立等待。按主/子 session 保存权限状态，使用短同步更新提交 remembered grant、拒绝计数和历史，落盘单独串行。
- Reset、ReleaseChild 和主 mode 改变立即撤销适用的在途请求；迟到回应不能授权或回填状态。持久 deny 撤销同域旧判断。
- 分类通道只负责派发，每次模型裁决使用自己的 Sideband 和总期限。
- 权限 Sideband 与同一 Goal 中仍在运行的主模型/其他裁决并行；已返回但尚未确认结算、旧 epoch 或关闭 Goal 的账本屏障仍然有效。
- 默认期限恢复 30 秒；首次尝试可使用完整剩余期限，只对实际无效响应或可恢复 provider 错误重试，不提前中止健康请求来预留重试时间。

## Capabilities

### Modified Capabilities

- `tool-authorization`: 并发请求、控制撤销和单一总期限。
- `behavior-goal`: 独立后台模型工作与运行中的前台 attempt 并行，结算屏障仍约束下一次准入。

## Impact

PermissionManager、主会话分类 worker、Sideband/Goal usage admission、权限文档和定向并发测试。显式 deny、子权限域、精确 permit、临时上下文和使用量持久化仍是同一套权威实现。
