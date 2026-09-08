## Why
配置编辑保留原始环境变量引用，而运行时加载会展开这些引用。技能添加和移除用已解析请求与原始配置比较，导致可以加载的变量路径无法正常管理，或重复添加同一位置。

## What Changes
仅在比较原始 skills.paths 和 ignore 时应用现有环境展开，再执行现有路径解析。保留未删除条目的原文。

## Capabilities
### Modified Capabilities
- configuration-rules: 原始技能配置路径比较。

## Impact
shell 技能管理，不改变全局配置持久化和请求路径语义。
