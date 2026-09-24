## Context

参见 proposal.md。Timeline 是 Run 的恢复权威；sidecar 只提供同一冻结 Run contract 的可变进度。现有加载会把无法解码的 sidecar 视为 Timeline seed，但存储写入器对所有现存 sidecar 都先解码，再做 revision 比较，因此 seed 写回遇到原损坏字节会失败。冷启动在取得 writer lease 后加载 run，随后才启动 persistence actor；`WorkflowRunStore` 初始化时可以使用现有 ACK 消息等待写入完成。

## Goals / Non-Goals

**Goals:**
- 为损坏 sidecar 建立严格的读取快照条件，避免覆盖 restore 之后出现的新状态。
- 在 actor 初始化成功前等待现有 durable-write 路径及 persistence ACK，并向初始化调用方传递失败。
- 保持可解码 manifest 的现有 revision CAS 与幂等行为。

**Non-Goals:**
- 改变 Timeline/sidecar 的数据权威、恢复候选上限、manifest schema 或有效 sidecar 的合并规则。
- 改造缺失 sidecar 的现有恢复方式，或将修复失败降级为仅日志告警。

## Decisions

- 在 restore 时为解码失败的 sidecar 记录长度与 SHA-256 指纹；修复消息携带该指纹。相较保存完整文件内容，它将每个待修复候选的额外内存限制为固定大小。
- 新增专用存储修复入口，在同一 run 锁内检查 `cleared` 和当前 `state.json` 指纹。只有在锁内仍匹配原损坏快照时才原子写入 seed；文件缺失、被替换或被改写都返回错误。该检查协调使用相同锁的 Grow 写入者；不放宽正常写入器的版本解码和 revision CAS。
- 复用 `WorkflowRunStateAndAck` 的 oneshot 确认通道，但仅在修复消息带有损坏快照时调用专用 guarded storage 操作。恢复初始化等待该 ACK；失败通过现有初始化错误路径返回。
- `WorkflowRunStore::from_restored` 提交需要修复的 run/快照，actor 初始化逐项 await ACK。普通生命周期协调和缺失 seed 仍保留现有异步持久化路径。

## Risks / Trade-offs

- [磁盘在加载后被外部改写] → 锁内指纹比较拒绝修复并显式失败，不覆盖变化后的文件。
- [多个损坏 manifest 同时恢复] → 指纹固定大小；修复逐项等待 ACK，不引入无界并行写入。
- [ACK 失败但原子替换可能已发生] → 初始化报告错误；下一次恢复按当前文件重新校验并走幂等/seed 解析，不声称本次成功。
- [平台目录项同步能力不同] → 复用 `ContainedDirectory::write_atomic` 既有 durable 参数和平台保证，不额外承诺 Windows 目录项 fsync。

## Migration Plan

无格式迁移。部署后冷启动遇到仍未变化的损坏 sidecar 时按 Timeline seed 修复；出现并发修改或文件系统错误时恢复失败并保留诊断。回滚代码不需要改写已修复的当前格式 manifest。
