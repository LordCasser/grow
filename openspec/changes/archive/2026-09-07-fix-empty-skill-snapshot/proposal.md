## Why
技能正文冻结把空正文保存为 None，后续读取又将 Some 空字符串视为未加载，导致已冻结技能重新读取可变文件。Workflow 的空技能快照因此不能保持内容稳定。

## What Changes
用 Some（包括空字符串）表示已加载正文，None 表示未加载；保留文件读取错误和无正文 synthetic path 的错误。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能正文快照的空值语义。

## Impact
技能正文加载 helper、类型说明及回归测试；不增加状态或修改 workflow 调度。
