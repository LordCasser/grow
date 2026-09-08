## Why
调试功能同时存在 slash 命令、注册动作 ID 和直接动作入口，需要分清隐藏但可执行的功能与真正闲置的声明。否则可能误删现场诊断能力，或把未注册枚举误认为可配置快捷键。

## What Changes
记录 debug 命令可达性，增加 R14 未注册 input dump ActionId 删除候选；不删除代码，不改变行为契约，跳过 delta specs。

## Impact
仅审计及临时候选清单。真实输入日志动作和所有 debug slash 子命令保留。
