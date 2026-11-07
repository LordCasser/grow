# workflow-execution Delta: confirm-workflow-journal-recovery-barriers

## ADDED Requirements

### Requirement: Journal recovery read is durable

Workflow journal 的恢复读取（`JournalStorage::read_bounded`）SHALL 只在返回的字节来自同一打开文件句柄的有界前缀、且该句柄的文件屏障（平台要求的 sync）与所属目录屏障都成功之后返回字节。已存在但屏障失败的读取 SHALL 向恢复调用方传播错误，SHALL NOT 把可读前缀解释为空 journal、Completed 或可恢复执行；缺文件 SHALL 保持 `NotFound` 原语义，读取 SHALL NOT 创建文件或跟随 symlink/reparse 目标。恢复读取 SHALL NOT 在读取后另开同名文件做屏障（文件代际可能已更换）。Shell `SessionJournalStorage` 是文件/目录 durability 的 owner；`validate_suspended_journal` 与 `Journal::load_storage` 只消费 trait 返回值，不复制 fold 或二次读取。

#### Scenario: Append succeeds but sync barriers fail, then cold recovery reads

- **WHEN** append 写完整字节后 file-sync 或 dir-sync 失败（live Run 保持 Pending），随后恢复路径调用 `read_bounded`
- **THEN** `read_bounded` 返回错误且不返回字节；`validate_suspended_journal` / `Journal::load_storage` 不把该前缀当作可恢复的 Completed 状态。

#### Scenario: Faults clear and the same journal is recovered

- **WHEN** 注入的 file-sync/dir-sync 故障解除后再次恢复读取同一 journal
- **THEN** 读取返回原字节，加载得到与故障前相同的 operation 身份（同一 Completed completion），不产生新 intent 或 child dispatch。

#### Scenario: Journal file is missing

- **WHEN** journal 文件不存在时恢复读取
- **THEN** 返回 `NotFound` 原语义（空 journal 恢复路径），且不因读取而创建文件。

#### Scenario: Journal target is a symlink

- **WHEN** pinned 目录内 journal 名字指向 symlink/reparse 目标
- **THEN** 恢复读取拒绝该目标，返回 InvalidData 类错误，不读取目标内容。

#### Scenario: Journal exceeds the byte limit

- **WHEN** journal 文件超过 `MAX_JOURNAL_BYTES`
- **THEN** 恢复读取返回既有 byte-limit 错误语义（UnsafeRestore 路径），size bound 保持。
