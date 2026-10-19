## Why

Plan interview 的 “Chat about this” 和 “Skip interview” 会丢弃已输入的自由文本。Pager 只发送 partial answer 标签；即使用户输入内容，工具收到的也只是 `Other`，后续模型无法看到补充信息。

## What Changes

- 让两个 Plan partial response 沿用 `annotation.notes` 携带自由文本。
- 工具结果格式在保留既有选项标签的同时向模型呈现 notes。
- 为 Pager 投影、wire round-trip 和模型可见格式补充回归。

## Capabilities

### Modified Capabilities
- `client-surfaces`: Plan interview partial actions preserve entered notes.

## Impact

只调整现有 `ask_user_question` response 的 annotations 投影和消费路径，不增加问卷协议或 Plan 生命周期状态。
