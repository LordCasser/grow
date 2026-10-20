# 允许已明确处置的未派发执行切换工作范围

## Why

`LocalPlan::validate_transition_from` 已将显式记录的 dispatch failure 视为已处置执行，但 actor 的 `SelectScope` 仍拒绝所有 `terminal == false` 的 link。由此，必需交付已 Released、或可选交付明确未派发后，合法的新范围仍无法提交。

## What Changes

- `SelectScope` 复用 `LocalPlan::validate_transition_from` 判定旧范围是否已处置。
- 保留对活动/结果未知执行、等待、未闭合必需交付和未处理 replan 的拒绝。
- 常驻回归覆盖必需/可选未派发 link 与 Released 状态矩阵，并确认拒绝路径不会伪造 terminal/result。

## Impact

- Affected spec: `task-planning`
- Affected code: session actor planning scope selection and its dispatch tests.
