## ADDED Requirements

### Requirement: Hook history publication is observational

会话恢复的 Hook 快照 SHALL 仅从已完成 Timeline Hook 事实生成，保留 occurrence 与工具 cause 的身份并明确标记历史快照。发布 SHALL 不重执行历史 handler、不追加 Hook/UI 持久事件、不关闭 rewind 窗口；实时执行保留既有执行与持久化边界。

#### Scenario: 重复发布已完成 Hook
- **WHEN** 一个已有完成 Hook 和可用 rewind 窗口的会话多次发布控制状态快照
- **THEN** 接收端获得相同 occurrence 的历史投影，Timeline 与持久通知不增加，handler 不运行，rewind 窗口保持可用。

证据入口：`crates/codegen/shell/src/session/actor/hook_dispatch.rs`、`crates/codegen/shell/src/session/actor/updates.rs`。
