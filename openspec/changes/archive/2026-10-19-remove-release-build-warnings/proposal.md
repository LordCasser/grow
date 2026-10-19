## Why

发布前编译仍报告 Pager 查询辅助方法的 dead_code warning。两个只接受 prompt ID 的便捷方法目前仅供单元测试调用；生产 watchdog 已统一使用接受 `Option<&str>` 的方法，同时覆盖 prompt 和 lifecycle 查询。保留无条件编译的测试包装会让正常 CLI 构建持续报警。

## What Changes

- 核对默认 CLI、工作区各 target 及发布 feature 的实际编译诊断，按调用关系修复本地确认的 warnings。
- 把仅测试使用的查询包装限制在 test cfg，保留生产统一查询入口及现有回归断言。
- 记录固定工具链、编译组合、warning 前后对照和平台验证边界；不增加全局 lint 屏蔽或修改 MCP 功能。

## Impact

这是编译维护，不改变运行时行为、公共接口、状态、持久化或发布门禁契约，故设置 `skip_specs: true`，不虚构行为 delta。相关不变量仍以 client-surfaces 的 authoritative prompt-state reconciliation 为准。
