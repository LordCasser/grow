## Why
save_config_locked 将所有文件读取错误当成空配置，非法 UTF-8 或权限/IO 错误后仍可能覆盖原文件。update_config 也把初始加载失败降级为默认配置。设置写入应在读取失败时终止。

## What Changes
保存仅允许 NotFound 作为空配置，其他读取错误直接返回。update_config 传播初始配置加载错误。抽取接受显式路径的内部保存入口以验证真实临时文件，不修改全局用户路径。

## Capabilities
### Modified Capabilities
- configuration-rules: 配置保存读取失败保护。

## Impact
Shell 配置持久化；不改变正常合并、原子替换、文件模式或设置模型，不清理其他持久化债务。
