## ADDED Requirements

### Requirement: Download cleanup requires known stale age
自更新清理下载目录时 SHALL 只删除已确认文件年龄超过清理时限、且满足既有保留策略的旧版本候选；未来时间戳和无法读取的年龄 SHALL 不构成删除依据。

#### Scenario: 系统时钟回拨
- **WHEN** 可清理候选的修改时间晚于当前时间
- **THEN** 保留该文件，其他明确过期候选仍按保留策略清理。

#### Scenario: 正常旧版本清理
- **WHEN** 候选年龄明确超过清理时限
- **THEN** 删除不属于保留集合的旧版本，当前版本、保留的上一版本及新文件保持不变。

证据入口：`crates/codegen/update/src/auto_update.rs` — `cleanup_old_downloads`。不承诺文件系统竞争下检查与删除的原子性。
