## Why
本次修复在diagnostics/pager-render/tty-utils/test-support中引入或依赖回归；原Core门禁只编译这些依赖、不执行其lib测试。正式发布必须覆盖新修复的验收。

## What Changes
现有immutable-SHA Core门禁的包清单加入四个Runtime测试包，沿用既有线程数、预算与失败阻断。

## Impact
官方Core可复用workflow和release验收覆盖范围，不改变平台矩阵或绕过任何检查。
