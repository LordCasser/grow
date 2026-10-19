## Why
用户最终明确：Ctrl+C 保持输入框及取消当前回合的行为，不再关联退出；默认连续两次 Ctrl+D 确认退出。原 session-tree suspension change 把单次 Ctrl+C 改成退出，混淆了取消与挂起，需要在本版发布前纠正。

## What Changes
恢复 Ctrl+C 的局部输入和当前回合取消，取消中重复 Ctrl+C 只重试取消、空闲时不升级退出。全局 Quit 默认绑定 Ctrl+D，复用现有一秒双按确认；确认后调用既有整树挂起并退出路径。挂起中重复 Quit 合并，失败时保留显式重试和强退。更新调用提示、用户指南、发布说明、规范及真实终端场景。无新运行时挂起机制、持久化格式或自动重放。

## Impact
Pager root/agent/welcome/dashboard 输入路由、默认快捷键、客户端规范及 PTY 夹具；原 session/tree 挂起、同身份恢复、命令中断和预算契约保持。
