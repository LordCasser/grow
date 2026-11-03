# 验证记录

## 已完成的只读核对

- 读取 OpenSpec 索引、现有提案及 session-timeline 的 preview/lifecycle/finalization、model-sampling 的 fragment budget、client-surfaces 的 deferred child history 契约。
- 流式汇总现存 205 个 memtrace JSONL；对 PID 86472 重新核对时间点、独立峰值和 purge 前后数值。
- 按 PID/事件类别汇总统一日志，按 canonical subagent 事件与时间截点核对根会话规模；未把日志/磁盘字节解释为 heap。
- 只提取系统 Jetsam 和 Grow IPS 的数值/错误类别；离线转换 shutdown-stall 快照，未对活跃进程采样或改变运行状态。
- 逐项读取前端 view 所有者、coordinator 容量/完成/退休、Shell Shutdown/线程和 persistence 退出路径及既有测试；针对正常退出、卡住和错误结果做静态对抗性推演。
- `git show HEAD:<file>` 确认两个主要保留结构在 HEAD 中也存在。Atlas 局部查询返回的部分符号范围与当前源码不一致，调用位置最终均以实际文件读取为准，没有把薄调用图当作全仓证明。

## 限制

没有恢复旧进程 heap、复现 50G、执行故障注入或运行 Rust 测试；本轮只改分析文档，不为只读分析触发现有工作区的编译。没有生成 Cargo 产物。静态可达路径不代表本次事故的已证实调用轨迹。系统快照、memtrace 与磁盘统计口径分别保留。

## 文档与规范校验

- `git diff --check -- openspec/changes/analyze-multi-subagent-memory-growth openspec/backlog.md`：通过。
- `openspec validate analyze-multi-subagent-memory-growth --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：最终 19/19 通过。首次运行时另一个并行编写中的 change `cover-notification-turn-exit-suspension` 暂未通过，随后该 change 的定向检查和全量检查均通过；本轮没有修改它。

- `openspec archive analyze-multi-subagent-memory-growth --skip-specs --yes`：已归档；没有修改主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：18/18 通过。
- 归档后 `openspec validate --archived --no-interactive`：800/800 通过。
- 归档文件、已完成 tasks 和 backlog 的报告链接：核对通过；离线转换产生的临时系统快照文本已清理，报告不依赖其存在。
