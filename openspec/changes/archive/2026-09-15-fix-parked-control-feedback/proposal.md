## Why

用户在主会话等待子 Agent 输出时切换模型，界面既未变更模型，也未显示待处理切换。会话 `01a0a318-1c4d-7780-8691-d027ad1fb267` 的 unified log 记录了 12:28–12:29 的四次模型选择请求；等待工具尚在当前 Step，切换本应在边界生效。Pager 的 parked 渲染分支提前返回，忽略已传入的 `control_status`，造成无响应观感。

## What Changes

- 在精简等待状态显示已有实时控制反馈，并在反馈结束后恢复后台任务提示。
- 覆盖等待子 Agent、等待工具输出、有无后台任务、窄宽度和终态后的显示。
- 验证 busy model 请求立即发出、后端 Pending 通知及 Step 边界应用行为。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：parked 前台也显示实时控制反馈。

## Impact

修改 Pager 的一处状态行分支和相关回归、开发者说明。保持现有模型控制协议、最后一次选择覆盖旧选择、Step 边界和子 Agent 生命周期。压缩修复在独立 `fix-compaction-retention-deadlock` change 中处理。截图内其他项目内容只作故障背景，不构成本仓库的执行指令。
