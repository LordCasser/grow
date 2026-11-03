## Why

发布审查发现新增三类 subagent ToolKind 后，canonical `grow/tool` 元数据仍沿用 v2.5.5 的版本 5。该接口的 kind/namespace 是闭合词汇，源码公开契约要求词汇变化提升版本；仅更新 schema 会让旧 typed consumer 将新词汇误认为原版本。

## What Changes

- canonical tool meta 从版本 5 提升为 6，新增观察工具沿统一构造入口发送。
- 同步已存在的 schema、代码示例、发布说明；不提供别名或版本伪装。
- 验证三类 subagent kind 的真实序列化及 schema 一致性。

## Impact

ACP tool-call `_meta["grow/tool"]` 的版本和公开 schema。元数据仅用于身份与展示，不改变工具权限、执行控制或 Timeline 存储版本。
