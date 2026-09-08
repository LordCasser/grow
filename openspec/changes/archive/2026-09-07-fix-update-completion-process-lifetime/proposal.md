## Why
安装后的补全生成直接等待子进程，无超时和取消清理；补全故障可阻塞已经完成二进制替换的更新。该步骤只负责尽力刷新补全，不能让已完成的二进制安装停留在收尾阶段，也不能在调用方取消后留下持续运行的补全进程。
## What Changes
- 每个 shell 补全生成限制为 10 秒，取消和超时终止直接子进程。
- 失败、空输出、超时均保留已有补全，继续后续 shell。
## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `client-surfaces`: 更新后补全的有界尽力执行。
## Impact
仅 update 的补全执行路径、测试和开发者说明，不改变后台 updater 的脱离父进程行为。
