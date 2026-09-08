## Why
Turn admission 在正文加载前无条件上报 PluginUsed.success=true。文件消失或读取失败后，统计仍称插件技能使用成功。

## What Changes
共享展开结果携带逐引用加载结果；admission 在展开后据此上报插件使用结果。

## Capabilities
### Modified Capabilities
- configuration-rules: 插件技能使用诊断与展开结果一致。

## Impact
shell 技能展开返回值及两个内部调用点。不改变技能派发、turn 归属或 interjection 的诊断范围。
