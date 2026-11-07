# Tasks

- [x] storage/mod.rs：两平台 `open_read_write_existing`（不创建、可读写、no-follow regular 校验）+ 单元测试（不创建 missing、拒 symlink、可写句柄）。
- [x] manager.rs：`SessionJournalStorage::read_bounded` 改为 durable 读取（同 fd lock/read/file sync/dir sync），实例绑定 `cfg(test)` fault seam（append/truncate 语义不变）。
- [x] workflow/src/journal.rs：`JournalStorage` trait 文档明确恢复读取 durability 契约。
- [x] Tier3 恢复测试：真实 pinned temp dir 注入 file-sync/dir-sync 失败→`read_bounded` 失败；解除后恢复同 operation Completed、无新 intent/child dispatch；missing/symlink/size bound。
- [x] 运行相关测试（manager journal、storage、workflow journal），`openspec validate --all --strict --no-interactive`（按父指示改为 single-change strict）。
- [x] 导出 /tmp 专属 patch，显式列本项路径 + SDD，排除继承 dirty。
