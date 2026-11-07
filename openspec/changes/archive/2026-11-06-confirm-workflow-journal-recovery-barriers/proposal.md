# Proposal: confirm-workflow-journal-recovery-barriers

## Why

F5 已在 main 归档：共享 fold 严格一次 completion、禁止回退。剩余缺口是恢复读取屏障（Session F4）：`SessionJournalStorage::append` 在写完字节后执行 `sync_data` + 目录 sync，若 sync 失败，live 端 Run 仍为 Pending，但完整字节可能已可读；当前 cold 恢复的 `read_bounded` 直接返回可读字节，`validate_suspended_journal` / `Journal::load_storage` 会把尚未确认 durable 的前缀当成 Completed/可恢复执行，在未确认 durable 之前推进 Run。

## What Changes

- Shell `SessionJournalStorage` 成为恢复读取的文件/目录 durability owner：`read_bounded` 在 pinned `ContainedDirectory` 内用不创建、可读写、拒绝 symlink/reparse 的既有 regular 文件语义取得句柄，取得与 append/truncate 一致的 exclusive 文件锁，从同一句柄有界读取，且仅在 `sync_data`（平台要求的 sync）与目录屏障成功后返回字节；任一失败原样传播，不得吞为空、Completed 或可恢复执行。
- 缺文件仍按 `NotFound` 原语义处理；不创建文件、不把"读"变成 create；不读后另开同名文件 sync（代际可换）。
- `storage/mod.rs` 增加最小 `open_read_write_existing` 帮助方法（Unix openat `O_RDWR|O_NOFOLLOW` 不创建；Windows 复用既有 read/write no-follow 打开与 reparse 校验语义）。
- `workflow::JournalStorage` trait 文档明确：恢复读取返回成功的字节必须来自已完成同一打开文件前缀 file+dir durability 的 durable 读取；签名不变，`validate_suspended_journal` 与 `Journal::load_storage` 继续消费该契约，不复制 fold、不二次读。
- Windows 目录屏障沿用 `ContainedDirectory::sync` 现平台约定（Windows 目前 Ok 且注释明确无 Unix dirfsync），不虚称更强保证。session lease / 原 writer retirement 门保持，读取屏障不授予恢复并发 writer。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `workflow-execution`：明确 journal 恢复读取的 durability 屏障（delta 见 `specs/workflow-execution/spec.md`）。

## Impact

- 代码：`crates/codegen/shell/src/session/workflow/manager.rs`（`SessionJournalStorage::read_bounded` durable 读取 + 恢复测试）、`crates/codegen/shell/src/session/storage/mod.rs`（两平台 `open_read_write_existing` 帮助方法 + 单元测试）、`crates/codegen/workflow/src/journal.rs`（仅 trait 文档）。
- 生产 fold/engine 逻辑不改；不建新 journal/registry；无 schema 变化。
