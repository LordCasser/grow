## Why
ManagedConfig 语法检查器仅在超时或 wait 错误时清理进程组；正常与非零退出会留下 Unix 后代进程。

## What Changes
所有观察到的退出均完成现有有界进程清理，再返回校验结果；清理错误不得伪装为校验成功。

## Impact
config managed_text validator、回归测试与开发入口。外部编辑后的回滚覆盖单独记录。
