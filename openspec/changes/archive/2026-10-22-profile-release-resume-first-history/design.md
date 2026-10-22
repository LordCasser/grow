## Measurement

使用仓库 Rust 1.93.1、普通 release（opt-level=3；非 release-dist），关闭 incremental，保留生产 JSONL、hash、完整校验、durable barrier 和 replay completion。先构建再直接执行目标，正式计时不与本任务构建重叠。现代 128/512/2048 轮各三次；512 轮短正文和缺失投影各三次。生成、正确性 oracle 与报告处理在 RPC 窗口之外。原 R6 debug 结果仅作历史参考，不当作同工具链算法 A/B。

保留 instrumentation 原始行的 timestamp、elapsed_us 和 load 起止；由结束时间减 duration 重建区间。首条历史为 CountingClient 收到文本；PTY 首历史、末历史和 SessionLoaded 另测，不能互换。父子 timer 不累加。缺失 span、线程重叠和 wall-clock 对齐误差明确标记。

## Scope and invariants

保持全事件校验、正文精确比较、当前 branch 投影、候选 anchor 顺序、resident frontier 和 complete notification barrier。本轮只归因和记录候选，不把未验证的缓存、压缩、writer 状态交接或流水恢复混入实现。只有数据支持的热点列为优先项，固定开销和长会话增量开销分开解释。
