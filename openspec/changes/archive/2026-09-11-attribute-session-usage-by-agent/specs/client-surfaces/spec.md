## ADDED Requirements

### Requirement: Session usage retains agent attribution behind aggregate surfaces

会话用量账本 SHALL 将本会话主 Agent 与每个已结算子 Agent 的已知 token 消费分别归属，同时 SHALL 使总体等于这些消费的累计结果。当前 `/usage`、headless usage 与没有 Goal 的 normal 状态栏 SHALL 只展示既有总体及 provider/model 投影，不公开 Agent 分项。

#### Scenario: Main and child agents consume tokens
- **WHEN** 主 Agent 产生模型消费，两个具有不同 `subagent_id` 的子 Agent 完成并回传各自累计消费
- **THEN** 会话总体包含三者消费各一次，Agent 分项能分别识别主 Agent 与两个子 Agent，并保留每项已知 token 数。

#### Scenario: One child uses multiple models
- **WHEN** 同一子 Agent 的终态累计快照包含多个 provider/model 分项
- **THEN** provider/model 分项继续按模型累计，该子 Agent 分项累计这些模型消费，且会话总体不遗漏或重复任何一项。

#### Scenario: Existing aggregate displays
- **WHEN** 包含子 Agent 消费的会话账本投影到 `/usage`、headless usage 或 normal 状态栏
- **THEN** 当前界面和公共 usage 形状显示包含子 Agent 的总体，但不增加 Agent 分项字段或逐 Agent UI。

#### Scenario: Incomplete child settlement
- **WHEN** 子 Agent 回传已知下界并标记用量不完整，或其用量无法可靠应用
- **THEN** 已知消费仍按原 `subagent_id` 归属，既有 incomplete 规则继续阻止总体被表示成精确完整值。

证据入口：`crates/codegen/chat-state/src/usage.rs`、`crates/codegen/shell/src/session/actor/updates.rs`、`crates/codegen/shell/src/extensions/notification.rs`、`crates/codegen/pager/src/app/status_blocks.rs`。
