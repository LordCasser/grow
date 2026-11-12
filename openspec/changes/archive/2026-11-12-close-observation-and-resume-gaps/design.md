## Decisions
Timeline 在接纳事件时索引生命周期事实的位置，resident 观察复用 runtime tree snapshot。嵌套来源继续核对准确 Seed/Spawn 链接。历史查询在同一 deadline 内与 runtime 查询并行，只允许验证过的历史终态抢先返回，普通错误或缺项不能抢占有效运行时响应。

仅对实际返回答案的 Completed 子任务，在工具结果的同一 Timeline 提交中记录交付 ID。折叠同步移除已接纳回执，并跳过迟到回执的待发状态。Shell 在 inbox gate 下提交和清理回执正文。结论工件继续可读。命令结算保存 snapshot 的实际起止时间，旧记录缺字段时使用现有记录时间。历史投影版本随之更新。
