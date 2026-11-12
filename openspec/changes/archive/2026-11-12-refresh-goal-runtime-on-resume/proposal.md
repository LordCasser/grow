## Why
冷恢复入口延后 Goal 工具资格检查，执行解封时漏掉该检查。恢复后的 Goal 即便成功切换为 Active，idle 调度仍可能使用启动时的不可用状态。

## What Changes
在恢复解封时按当前注册工具重新检查 Goal 资格，再释放等待中的执行和 idle 调度。
