## Why
/doctor 在 dispatcher 同步执行 tmux 查询和定义扫描，后续 fix 规划进入后台之前也先支付这段等待。诊断挂起时会阻塞交互。

## What Changes
把 TUI 报告采集移入受控后台执行，按请求时的会话/绑定身份提交结果，保留独立 CLI 同步路径和修复确认。

## Impact
Doctor 的 TUI dispatch、effect/result 及相关测试；不修改真实修复写文件内容或绕过确认。
