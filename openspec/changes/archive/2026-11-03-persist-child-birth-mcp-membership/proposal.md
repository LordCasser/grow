## Why
child live ceiling保存出生MCP成员与client identity，但Spawn只持久RWX，cold restore从当前更宽profile/pool重建会补入原来没有的服务。聚合source hash没有覆盖该事实，不能当作授权证明。

## What Changes
在Spawn保存已知出生MCP server集合，实际过滤后pool与持久值同源；冷恢复与当前父上限/许可求交并绑定新incarnation。缺出生事实在整树准备前明确拒绝。

## Impact
Timeline Spawn载荷、subagent初始化与tree恢复准入。普通tool仍按当前definition/preflight/permit重核对，不新增root配置snapshot或持久client ID。
