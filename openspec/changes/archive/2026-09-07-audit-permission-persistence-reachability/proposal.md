## Why
权限保存保留多种历史策略，必须区分当前会话切换、默认设置和仅测试覆盖的代码，避免把未接入的通知策略当作实际运行行为。

## What Changes
记录生产调用可达性，将 BestEffort 保存策略及其专用结果列为 R8 待确认候选；修正当前会话 setter 的过时持久化注释。不改变行为。

## Capabilities
无契约变化，skip_specs=true。

## Impact
审计记录、临时清单和注释。默认权限仍用 WithRollback 且 session_id=None；不删除仍在使用的会话通知。
