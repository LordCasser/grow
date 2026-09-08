## Why
command_uses_shell 只检测普通空格，tab 参数及 LF 多行命令会被误当作相对可执行路径。

## What Changes
识别 shell 分隔符空格/tab/LF，执行和去重共用该规则。

## Capabilities
### Modified Capabilities
- extension-runtime: shell 空白分隔命令路由。

## Impact
config 路由、runner 回归和 discovery 去重测试。
