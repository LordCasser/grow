## ADDED Requirements

### Requirement: Orphan image reads honor remaining recovery budget
占位图片恢复 SHALL 在读取前使用单图上限与本次恢复剩余总预算的较小值作为读取上限；预算耗尽时停止恢复。

#### Scenario: Remaining budget is tighter
- **WHEN** 剩余总预算小于单图上限且候选文件超过该余额
- **THEN** 有界拒绝并停止后续恢复，保留已恢复图片，不为先验证MIME而完整读取超额候选。

#### Scenario: Per-image cap is the limiting factor
- **WHEN** 单图上限小于或等于剩余额度且候选超过单图上限
- **THEN** 该候选失败后仍允许后续合法更小图片恢复。

#### Scenario: Exact remaining budget
- **WHEN** 有效图片大小等于剩余额度
- **THEN** 完整恢复该图片，并在下一候选前停止；已有附图及图片编号保持。
