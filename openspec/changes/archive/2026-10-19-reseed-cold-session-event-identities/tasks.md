## Implementation and verification

- [x] 1. 核对 load、物理 updates、Timeline expectations、replay/noReplay 与 exact append 调用顺序，完成最小修复。冷 writer 在 projection recovery 前从同一 parsed physical snapshot 取 eventId 上界，并在 actor load 的 replay/noReplay 分支前合并 Timeline expectation 上界。
- [x] 2. 运行定向回归：`cold_writer_load_seeds_from_full_physical_update_snapshot` 写入 ACP/Grow 高序号行，并证明 rewind 隐藏的物理行仍支配新事件序号；`cold_load_reseeds_from_expectation_missing_from_cache` 覆盖只有 Timeline expectation 的 crash 窗口。
- [x] 3. 执行 normal/minimal 冻结恢复后 headless continue 真实进程验证及相关 crate 回归。
- [x] 4. 更新开发者说明和版本验证记录，完成严格校验、场景核对及归档。

最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
