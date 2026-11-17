# Design

Pager 在应用已绑定会话、加载代次和子任务成员的后代恢复批次时，使用现有直属子任务所有权中的 Spawned 和 Ended。缺失的视图经既有 Spawned reducer 创建，缺失的终态经同一个 Finished reducer 收尾。已经收到的终态优先，身份不匹配的记录不用于补齐。

合成更新复用 `grow/taskStateOnly` 标记，让分页正文保留原始位置和提交边界。状态投影与原来一样不拥有子任务执行权限。后代磁盘读取继续校验 Seed 和 Result 链。
