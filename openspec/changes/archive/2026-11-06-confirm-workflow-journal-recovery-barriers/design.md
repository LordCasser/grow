# Design: confirm-workflow-journal-recovery-barriers

## 决策来源

复用 `openspec/changes/complete-nondeferred-backlog/design.md`「Workflow journal 的恢复屏障」节（父已亲核现 Workflow::JournalStorage 与 Shell SessionJournalStorage/manager 恢复链），本 change 只实现该既定设计，不重审系统。

## 关键决策

- **文件与目录屏障归 Shell `SessionJournalStorage`**，workflow 只相信 trait 返回值。契约签名保持 `read_bounded(max_bytes) -> io::Result<Vec<u8>>`，只在 trait 文档明确 durable 读取承诺；`validate_suspended_journal` 与 `Journal::load_storage` 均已消费该契约，无需复制 fold 或二次读。
- **同句柄身份**：`read_bounded` 用新的 `ContainedDirectory::open_read_write_existing`（不创建、可读写、拒绝 symlink/reparse 的 regular 文件，复用既有 no-follow 校验）取得句柄，取与 append/truncate 相同的 exclusive 文件锁，从该句柄有界读取，随后在同一句柄上 `sync_data`（macOS 走既有 `sync_file_durable` 语义时按平台要求），再 `run.sync()`（Windows 沿现平台约定：`Ok` 且注释明确无 Unix dir fsync）。不读后另开同名文件 sync（代际可换），不把 create 读当缺失。
- **故障注入用实例绑定 `cfg(test)` seam**：`SessionJournalStorage` 内 `AtomicBool`（file-sync / dir-sync 各一），仅测试代码可翻转；不影响产品配置，不做全局 fault 竞态框架。
- **顺序保持**：append 语义不变（仍 sync 文件后 sync 目录）；恢复读取按 open → lock → bounded read → file sync → dir sync → 返回。
- **NotFound 语义保持**：`open_read_write_existing` 缺文件返回 `NotFound`，上游空 journal 恢复路径不变。

## 测试策略（Tier 3）

- 真实 pinned temp dir 的 `SessionJournalStorage`：append 完整后注入 file-sync / dir-sync 失败，`read_bounded` 同样失败且不返回字节；解除故障后 `read_bounded` 成功，`Journal::load_storage` 得到同一 operation 的 Completed completion，无新 intent/child dispatch。
- missing 不创建、symlink 拒绝（unix）、size bound 保留。
- 同 fd 身份为结构性保证（代码审查），不另做运行时断言。
- Windows 实机不运行，平台差异如实写限制。
