## Why

用户在Windows打开child历史得到contained regular directory的os error32。实际Pager replay_inherited_updates→stream_session_history_at→with_prepared_history仍使用writer-style open_session_by_id；仓库既有独立Windows观察契约与open_session_by_id_shared_read尚未覆盖此入口。普通namespace pin不分享DELETE，会与仍live的发布句柄冲突。

## What Changes

- 只读历史准备使用既有contained shared-read id resolver，保留验证同一实体、完整Timeline、物理history与duplicate-id检查，不进入writer cache或取得执行权。
- 回归覆盖公共child history/replay入口与Windows持有DELETE发布句柄共存，保留writer namespace pin、no-reparse及损坏/缺失行为。

## Capabilities

### Modified Capabilities

- session-timeline：明确child history和公共replay适用既有Windows独立观察契约。

## Impact

Shell storage只读准备owner与定向tests，必要的现有Windows storage工作流测试选择；不更改Pager呈现/执行恢复，不修改Windows默认共享策略或writer排他边界。用户正常会话不受干扰，测试独立GROW_HOME。macOS不能声称Windows native测试已通过。
