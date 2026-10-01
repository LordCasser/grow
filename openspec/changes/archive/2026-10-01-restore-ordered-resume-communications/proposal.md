## Why

真实会话 `01a0ee47-1ad4-70f0-9c87-d0319a70d814` 的 12 条 Agent 回复仅存于 Timeline。resume 先加载完整正文，再发布全部回执，导致回复集中出现在最新输入后的尾部。用户需要恢复会话现场，而不是一个新生成的通信回放页面。

## What Changes

- 历史通信恢复纳入有序历史重建，用原 receipt 身份与可验证因果锚点安放记录。
- 根会话、child 延迟加载和离线 transcript 复用同一顺序/去重原则；不在加载末尾重新发布已有历史回执。
- 保持原正文/通信样式及已保存终态，恢复来源标识仅留内部元信息，不生成 replay 文案或新执行。

## Capabilities

### Modified Capabilities
- `client-surfaces`：约束 resume 的通信记录顺序和现场恢复。

## Impact

Shell 只读历史规划、load 的通信 snapshot 发布及 Pager child 历史读取。Timeline 是 authority；不修改真实会话、不消费通知、不新增 Hook/模型请求，也不改变 Goal 恢复策略。
