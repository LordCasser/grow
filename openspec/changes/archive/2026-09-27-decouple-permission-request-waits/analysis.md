# 权限请求为何等待，以及本次修复的边界

## 20 秒不是生产测量值

`child_judgment_distinguishes_slow_provider_from_stalled_primary` 在 mock HTTP provider 收到请求之后，人为推进 20 秒虚拟时间才释放有效 JSON 响应。它验证第一次合法请求不应被提前切断，不能用来推导截图中模型实际耗时。没有截图会话对应的请求、provider 和 Sideband 日志，目前不能判定该次超时具体发生在哪一段。

## 可从实现和实验确认的依赖

1. `spawn_permission_manager_inner` 原本在单一 `recv` 分支内等待完整请求。分类或人工提示挂起时，其他 primary/child 请求以及 Reset、ReleaseChild、SetMode 都不能执行。即使下一请求只需本地判断，也被占在队尾。
2. `wire_permission_auto_llm_classifier` 原本从分类通道收到一个请求后，等待该 Sideband 的准备、模型和结算全部结束，再接收下一请求。仅修复 manager，多个独立 Sideband 仍会串行运行。
3. `GoalUsageWindow` 原本允许前台忽略运行中的后台 attempt，但入站后台仍会等待同 owner 的前台 attempt。权限 Sideband 又没有标记后台身份。因此启用 Goal 时，Sideband 会在 provider 准入前等待主模型结束。
4. 旧实现把总期限分给多次 attempt，健康但较慢的首次响应可能在总期限到期前被取消。上一轮提高期限和调整比例只改变了等待长度，没有移除前三项依赖，本次删除该切片。

Sideband 本身有独立 timeline、provider future、取消与用量归属。以上等待不是“主 Agent 需要串行思考权限”的要求，而是外层控制流和用量屏障引入的依赖。

## 实现方式

- 命令循环只分配权限域、处理控制命令和收割任务。每个请求由 JoinSet 管理，独立等待模型、人工提示或取消。
- 每个 primary/child scope 保有自己的状态。请求读取快照，外部等待结束后检查 scope cancellation，再短暂借用当前状态提交结果。拒绝计数与 remembered grant 均不使用旧快照覆盖更新。
- Reset、ReleaseChild、主 mode 变更和新的 remembered deny 会使相关旧请求的 token 失效。被撤销的结果不能签发授权，不能重新保存状态。mode 变更只撤销 primary，child 仍按冻结的独立 mode 判断。
- 根权限文件保留串行写入，在获得写入锁后读取最新状态。该文件锁不占据权限请求入口；shutdown 会等待请求和写入结束，再关闭 audit sender。
- 分类器通道只派发独立 Sideband。权限 Sideband 使用现有 background 标记；Goal 同 epoch、同 active Goal 内的 live attempt 可与其并行。returned、settlement、旧 epoch、Goal 关闭等边界仍受约束，不跳过用量确认。
- 默认模型期限回到 30 秒。一个绝对 deadline 包含派发、准备、模型和结算；首次可以用完整剩余期限。只对实际返回的无效结构或可恢复错误最多重试一次。

## 仍然合理的顺序

权限状态提交、根权限文件写入、审计序号和已返回模型用量的结算仍须有序。这些顺序不要求等待另一个正常运行的模型或人工提示完成。审计字段 `queue_depth` 沿用既有 wire 名称，实际统计所有在途请求，不是 FIFO 等待人数。provider 自己的限流/排队、上下文预填充、持久化故障或实际迟迟无响应仍可能消耗总期限；本次并发用例保证的是 Grow 不会把不相关请求的外部等待串起来。

## 审计发现的既有差异

现有两个 manager 测试要求拒绝结果透传 classifier reason，而 HEAD 的 manager 实际只返回固定拒绝文案。已单独用原始 HEAD manager 重跑，得到相同的两个失败。它们既不是本次调度修改引入，也不通过删除断言掩盖；登记 backlog，另立 change 明确理由的产品/安全契约。本次保持该既有行为。
