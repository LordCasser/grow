# 独立审查

`/root/rules_recovery_review`只读核对基线、实现、调用方与本机Chrono0.4.45源码，未运行Cargo。发现直接typed日期比较会改变合法扩展年份的旧排序：格式化支持signed year，旧parse仅4digit，所以last-active失效应fallback updated、双方失效应None。现以typed year范围0..9999复现旧key，不拒绝输入、不改显示字段；MIN/MAX、-1/0/9999/10000差分通过。

最终确认：Reverse<Option<UTC>>与旧FixedOffset(UTC)时序等价；Arc ID按内容排序，cached stable sort保留同key输入次序；query→empty-only原normalize去重→limit→原row builder。worktree lookup只是读取，调用方没有要求为丢弃行查询或保持无snapshot承诺的观察次序；无需新增缓存/锁。unified_list后续facet/overfetch逻辑未前移。

最终静态复核通过，无新增阻断问题。测量仅debug pure/materialization、预设label排除DB，不能外推到磁盘/端到端列表。root后补leap秒样本和caller回归，结果见verification。
