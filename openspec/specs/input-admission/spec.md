# input-admission Specification

## Purpose
定义用户输入从接纳到排队恢复的契约。重点约束 input identity、payload 快照、缺失附件的失效处理和 turn 执行前的上下文发布，避免恢复时重复接纳或误用输入。

## Requirements

### Requirement: Durable input recovery
输入恢复 SHALL 复用已经接纳的 input identity 和 payload 快照。

#### Scenario: 重启恢复排队输入
- **WHEN** 已接纳但未执行的输入被恢复
- **THEN** 继续使用原 input_id，不重复执行 Hooks 或新增接纳事件。

证据：`crates/codegen/shell/src/session/actor/input_admission.rs` — `large_input_recovery_reuses_admission_and_snapshot`。

### Requirement: Missing attachment isolation
输入恢复 SHALL 将附件缺失的输入失效处理，同时保留其他可恢复输入。

#### Scenario: 附件被删除
- **WHEN** 排队图片输入的 artifact 缺失，但另一个文本输入完整
- **THEN** 缺失输入失效，完整输入仍可继续。

证据：`crates/codegen/shell/src/session/actor/input_admission.rs` — `missing_input_attachment_is_invalidated_without_blocking_other_inputs`。

### Requirement: Frozen turn behavior
turn SHALL 使用接纳时捕获的 Behavior，且执行前必须完成稳定上下文前缀的持久化发布。

#### Scenario: 前缀发布失败
- **WHEN** handle_prompt 的 ensure_prefix_ready 返回错误
- **THEN** 返回 bootstrap 边界错误，不执行该 prompt。

证据：`crates/codegen/shell/src/session/actor/turn/admission.rs` — `handle_prompt`。

### Requirement: Failed image asset batches reclaim completed files
When user-image batch persistence fails during decoding or writing, it SHALL attempt handle-relative durable removal of assets successfully created earlier in that call, preserve pre-existing files and return the original failure. Cleanup failures SHALL be reported without replacing the primary error.

#### Scenario: Later image cannot be saved
- **WHEN** a later asset write fails after an earlier asset was published
- **THEN** the earlier asset is removed when cleanup succeeds, unrelated existing assets remain and the original write error is returned.

#### Scenario: Invalid later encoding
- **WHEN** later Base64 decoding fails after an earlier asset was published
- **THEN** the same cleanup applies to the completed assets.

### Requirement: Image asset preparation is retry-safe
Repeated preparation of the same ordered normalized image batch SHALL reuse verified assets rather than allocate unbounded duplicate copies. Reused assets SHALL NOT enter a new batch rollback set. A lost durable-message acknowledgement SHALL NOT alone authorize removal of potentially referenced assets.

#### Scenario: Preparation retried
- **WHEN** an ordered image batch is prepared again with unchanged normalized bytes
- **THEN** its verified asset references remain stable.

#### Scenario: Acknowledgement unavailable
- **WHEN** image-bearing message commit acknowledgement is lost
- **THEN** referenced assets remain available pending authoritative reconciliation.

#### Scenario: Concurrent preparation
- **WHEN** two calls prepare the same ordered normalized batch concurrently
- **THEN** they return the same verified published paths and a losing call cleans only its own staging directory.

#### Scenario: Published bytes conflict
- **WHEN** an existing batch file differs from the expected normalized bytes
- **THEN** preparation fails without overwriting or removing the existing batch.
