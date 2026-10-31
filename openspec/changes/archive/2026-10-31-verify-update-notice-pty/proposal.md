## Why

默认自动更新与提示已实现并通过编译，尚缺真实终端画面的验证。

## What Changes

增加仅测试使用的 PTY 回归，从既有 oneshot 更新结果通道注入目标版本，运行真实 Pager UI、嵌入式 Agent 和终端渲染，检查欢迎页、会话内 toast、minimal notice，以及无更新不提示。

## Impact

只新增测试与验证证据，不修改生产行为或契约，skip_specs: true。
