## Context

当前权限 manager 把状态所有权和请求生命周期混在一个 `while recv` 分支里，外部 await 期间无法接收其他 Request 或 Reset/ReleaseChild。分类通道重复这层串行化。GoalUsageWindow 已支持活跃 background attempt 不阻挡前台，但反方向仍会等活跃 foreground，权限 Sideband 也未声明后台身份。

## Decisions

1. 复用 LocalSet 单线程所有权，每个主/子权限域使用一份短借用状态；请求读取冻结快照，模型返回后在短同步区读取并更新最新拒绝计数，人工回应提交到最新域状态，不用整份快照覆盖并发更新。
2. 由 manager 的 JoinSet 管理请求任务。控制命令不等待裁决。域 cancellation token 同时作为权限 epoch：主 mode 变更只撤销 primary，Reset 撤销全部，ReleaseChild 撤销该 child；新 remembered deny 撤销该域其他旧判断。任何外部 await 后先检查撤销，再允许或提交。
3. 根会话权限文件使用单独写入互斥，只在取得写锁后读取最新状态快照。Reset 的默认状态与后续授权沿相同路径保存，避免旧请求覆盖新状态。子域状态只留内存。Shutdown 取消并等待任务和写入完成，再关闭 audit sender。
4. 分类通道为每个请求启动独立 LocalSet task；各自管理 Sideband、deadline、requester cancellation，不持有全局锁等待模型。
5. 权限 Sideband 使用已有 background 标志。Goal fence 对本次 background 准入允许同 epoch、同 active Goal 的运行中 foreground 共存；returned、claimed settlement、旧 epoch、关闭 Goal 仍等待/拒绝。前台继续忽略运行中的 background，但必须等已返回后台用量结算。
6. 一个绝对 deadline 约束全部尝试，首次不按比例分割；恢复 30 秒默认，明确区分调度等待和 provider 真正不回应。无需增加模型或权限配置。

## Validation

- 先用两个 child + primary 的实际 manager/Sideband/mock provider 接线复现 A 挂起导致 B/primary 不能前进；改后 B 和 primary 在 A 未释放时完成。
- 覆盖同源并发、人工 prompt 挂起、mode/Reset/ReleaseChild、迟到 allow 和拒绝计数/remembered grant 更新。
- 开启 Goal 并挂住 root foreground attempt，权限请求仍到 provider；反向前台可在权限 provider 活跃时准入，returned/epoch/budget 屏障保持。
- 20 秒有效首次结果在 30 秒总期限内成功；真实总期限到期 fail closed。

## Risks

并发结果会按完成顺序提交，而不是请求开始顺序；安全状态收紧通过域撤销处理。人工客户端仍可能按自己的 UI 顺序展示提示，这不再占用共享权限 actor 或模型分类通道。模型服务本身的排队和吞吐不在本地调度保证内。
