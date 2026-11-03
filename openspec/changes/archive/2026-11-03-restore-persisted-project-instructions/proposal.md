## Why
恢复的模型 Surface 保存旧 ProjectInstructions，但 source 与 Auto classifier 读取当前磁盘规则，正常编辑 AGENTS.md 后挂起任务无法继续。

## What Changes
恢复时以已验证当前分支 typed ProjectInstructions 为唯一有效规则来源；无历史规则保持无规则。模型、source、root classifier 同源，child 不覆盖父 classifier。新任务仍使用当前规则。

## Impact
Shell actor spawn、source 和项目指令投影；不修改 checkpoint、不跳过恢复校验。
