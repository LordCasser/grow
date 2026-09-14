## ADDED Requirements

### Requirement: Session observation does not repair durable projections
普通会话 full/light observation SHALL 从有效 Timeline 派生 title/model 的内存投影，不写 Summary、不获取 writer lease，也不执行 sideband crash repair。显式 replacement-writer load SHALL 在取得独占 lease 后修复滞后的持久投影。观察 SHALL 保留 canonical 数据校验和投影冲突拒绝。

#### Scenario: Observe lagging projections beside a live writer
- **WHEN** 当前 writer 仍存活而 Summary 的 title/model 落后于有效 Timeline
- **THEN** 独立 full/light observation 成功返回 canonical 内存值，Summary 和 sideband 文件不变，观察 adapter 不获得 writer capability。

#### Scenario: Replacement writer repairs the lag
- **WHEN** 原 writer 退出后新 writer 获得 lease 并加载相同会话
- **THEN** 落后的 Summary 被修复为 Timeline 的 canonical title/model。

#### Scenario: Conflicting or malformed canonical data
- **WHEN** Summary title 在相同或更高序号与 canonical title 冲突，或 model change 数据损坏
- **THEN** 观察与 writer load 都拒绝恢复，不以只读模式绕过校验。

证据：crates/codegen/shell/src/session/storage/jsonl/mod.rs 与 tests.rs。

