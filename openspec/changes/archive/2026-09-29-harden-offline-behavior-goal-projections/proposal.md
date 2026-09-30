# 离线 Behavior / Goal 展示边界

## Why

第二轮专项核对发现 CurrentModeUpdate 被离线 tracker 忽略，Control terminal 也未展示；Goal cleared 使用空 ID，离线 map.remove 无法清除之前的目标。Workflow 没有复用 revision 护栏。它们不会触发真实执行，但会让 replay/export 错报当前协作模式或残留旧目标，不能以只读隔离替代展示正确性。

## What Changes

- 只读投影识别 Normal/Clarify/Plan/Workflow/Goal 及记录中的 Plan phase；控制成功/拒绝只展示已有 terminal。
- Goal 使用单个当前展示状态；clear 清除，旧目标更新不能复活，未知状态忽略，保留已记录预算/用量。
- Workflow 按已有 revision 语义去重与防回退；离开 Workflow Behavior 不推导 Run 终态。
- 校验捕获 Timeline 的 Control snapshot，并在结尾补齐最新只读 Behavior/Goal snapshot；不声称恢复缓存缺口处的精确交错。
- 添加 Goal 设置/退出/重启/停止、自动续轮、Plan confirmation 与 Workflow 晚到事件的测试和审计矩阵。

## Impact

仅 Shell 离线来源与 Pager 展示/播放。不改 BehaviorCoordinator、Goal continuation、权限、控制请求或真实执行状态机。历史 pending interaction 的模态框/按键细节和完整 Workflow cache 重建独立登记，不在本次构造恢复 runtime。
