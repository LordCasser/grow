## Decision

Plan artifact revision 属于当前文档周期，submission ID 属于整个 session。producer `admit_plan_handoff_notification`、consumer `plan_handoff_notification_matches` 及已归档 Behavior 契约都使用后者。将 Timeline 中过时的 equality 校验改为正 ordinal，保留 `valid_notification_owner` 对合法 hash 和正 artifact revision 的校验。source identity 仍包含 artifact hash/revision/kind，version 再绑定 submission ID，因此不同周期的相同正文不会被历史 receipt 去重。

无需 schema 变化、兼容分支或历史重写：已有合法事件按原字节恢复，新的合法提交不再被错误拒绝。Shell 控制提交、模型审批、精确 handoff 匹配和消费规则保持；只有已获当前 owner 批准的 handoff 才能进入现有执行链。

## Boundaries

截图来自其他用户，没有对应 Timeline，不能断言唯一触发条件。确定性回归已在 revision=1/submission=7 的生产 receipt admission 上观察到 InvalidNotification；另以两个独立 Plan 周期验证 session submission 递增而 artifact revision 回到1。

另发现 pending inbox 为空时，已由当前 Turn 消费的 handoff 也会误报 ACK lost。该路径有独立回归证据，但所需消费归属查询不属于此次身份校验最小闭环，原型已撤回并登记 backlog，不以放行空 pending 掩盖状态问题。
