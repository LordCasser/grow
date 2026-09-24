## ADDED Requirements

### Requirement: File-backed skill body reads are bounded
显式读取文件中的技能正文 SHALL 最多消费 1 MiB 加一个探测字节；源文件超过 1 MiB 时 SHALL 返回明确的超限错误，不得返回、注入或冻结截断正文。该约束适用于普通正文加载和工作流正文快照。`load_skill_content` 已携带的内存正文快照继续按现有规则返回；工作流快照创建继续读取当前文件。

#### Scenario: File-backed body is exactly at the limit
- **WHEN** 显式加载的技能文件总大小恰为 1 MiB
- **THEN** 加载完整正文，不因探测边界拒绝该文件。

#### Scenario: File-backed body exceeds the limit
- **WHEN** 普通正文加载或工作流正文快照读取总长度超过 1 MiB 的技能文件
- **THEN** 返回包含文件路径和大小上限的错误，不提供部分正文。

#### Scenario: Preloaded body snapshot is already available during content loading
- **WHEN** `load_skill_content` 收到携带已加载正文快照的技能条目
- **THEN** 返回原快照，不重新读取或应用文件大小上限。

证据：`crates/codegen/tools/src/implementations/skills/skill.rs` — `load_skill_content`、`load_skill_with_body`；调用方为 `crates/codegen/shell/src/session/slash_commands.rs`、`crates/codegen/agent/src/prompt/skills.rs` 与 `crates/codegen/shell/src/session/workflow/tracker.rs`。
