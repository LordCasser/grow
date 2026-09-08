# context-compaction Specification

## Purpose
定义压缩对模型可见上下文的影响。约束选定 Surface 范围的替换、后台生成与前台发布的边界，以及取消后迟到结果的处理，避免异步压缩覆盖新的上下文事实。

## Requirements

### Requirement: Range scoped compaction
压缩 SHALL 只替换选定 Surface 范围，保留未选中内容的 identity。

#### Scenario: 局部压缩
- **WHEN** 只压缩部分上下文
- **THEN** 未选中 Surface identity 保持不变。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `partial_compaction_preserves_unselected_surface_identity`。

### Requirement: Boundary publication
异步压缩 SHALL 冻结生成输入，并由前台在闭合 Step 边界发布结果。

#### Scenario: 前台仍在执行
- **WHEN** 后台压缩结果已生成而 Step 尚未闭合
- **THEN** 后台不能自行改写当前 Surface；边界提交重新检查 authority 和 model。

证据：`crates/codegen/shell/src/session/actor/compaction.rs` — `PreparedCompaction`。

### Requirement: Late result invalidation
取消后的异步压缩 SHALL 丢弃迟到的 provider 结果。

#### Scenario: 取消后才收到结果
- **WHEN** 压缩任务已被控制转换取消
- **THEN** 迟到结果不能提交到上下文。

证据：`crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs` — `async_compaction_cancel_discards_late_provider_result`。
