## Why
滚动记录器 I/O 失败会置 Sink::Disabled，但外层仍用 Option::is_some 判断启用。界面误报开启，用户下一次切换只是删除已停写对象，必须再切一次才能尝试恢复。

## What Changes
从 recorder 实际 sink 判断活动状态；失败后下一次切换直接创建新记录器。

## Impact
只修复状态报告和运行时切换，不修改滚动算法、日志字段或错误时停止记录的策略。
