## Why

用户报告会话 `01a19a05-5281-7201-bae3-89f599512353` 连续两次 Ctrl+D 退出后，resume 将五个运行中 child 显示为 cancelled。需要核对实际取消时间、退出来源和当前程序，避免把旧进程退出造成的终态误当作恢复时取消。

## What Changes

- 保存脱敏的父子 Timeline、日志和进程起始时间证据，明确已证实事实与归因限制。
- 使用当前已安装程序运行现有 Ctrl+D、递归 child 冷恢复 PTY 回归。
- 补充开发者排查说明：核对退出时刻的持久挂起事实及运行进程，而非只看恢复时 UI 或磁盘上的新程序。

## Capabilities

无契约变化。现有 `session-timeline` 和 `client-surfaces` 已要求确认整树挂起后退出、恢复原 child 身份；本次只调查事故、验证已有实现并补文档，设置 `skip_specs: true`。

## Impact

仅本 change 的调查/验证记录及 `docs/development.md`。不修改运行时代码、用户会话、child 终态或项目文件。
