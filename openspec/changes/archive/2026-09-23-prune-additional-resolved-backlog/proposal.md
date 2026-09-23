## Why

backlog 仍把四组已完成的修复过程与真正未解决的问题混写，读者可能将它们误判为当前债务。对应归档和主规范已经记录完成行为。

## What Changes

- 移除已完成的第二轮恢复布局、原生终止来源分离、portable 工具往返保留，以及不同原始工具 ID 的 Messages 编码记录。
- 保留长会话输入 p95、Responses phase、结构化 portable tail、重复原始 ID 歧义等未完成边界。
- 只整理 backlog；不改变代码、主规范或产品行为。

## Impact

文档维护，`skip_specs: true`。已完成事项仍可在既有归档验证记录中追溯。
