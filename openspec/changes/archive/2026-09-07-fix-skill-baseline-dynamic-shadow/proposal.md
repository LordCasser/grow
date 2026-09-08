## Why
动态技能在所有投影中优先于 baseline；同一文件已动态发现后，新 baseline 的描述/启用/条件变化被旧动态副本遮蔽。

## What Changes
新 baseline 接管其实际包含的规范路径，移除该路径的旧动态副本；其余动态发现保留。先复现再实现。

## Capabilities
### Modified Capabilities
- configuration-rules: baseline 对同一路径的刷新权威。

## Impact
SkillManager baseline 更新及动态去重集合；不改变不同路径同名技能的发现优先级。
