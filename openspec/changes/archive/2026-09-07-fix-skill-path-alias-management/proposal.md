## Why
技能发现已支持 tilde 和文件系统别名，但管理入口仍用配置原文比较已解析的请求，导致同一位置重复添加、无法取消忽略或移除，并让来源计数错误。

## What Changes
添加、移除和来源统计按解析后的路径比较，保留未删除配置条目的原始写法。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能管理路径别名一致性。

## Impact
shell 技能扩展。保留组件边界；不改变发现优先级或 reset。
