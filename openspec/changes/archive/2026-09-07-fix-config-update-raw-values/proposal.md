## Why
update_config 使用 load_from_disk，后者展开环境变量并应用版本覆盖。修改一个设置时，typed Config 的其他字段因此可能把运行时值写回基础配置，丢失原始环境变量引用或改变基础版本行为。

## What Changes
配置编辑使用原始 TOML 读取，与 save 路径共享现有读取错误保护。运行时加载保持环境展开及版本覆盖。

## Capabilities
### Modified Capabilities
- configuration-rules: 设置读改写保留原始值。

## Impact
仅 Shell update_config；不改变 save_config 的显式 Config 语义，不引入新配置层，不修改运行时加载器。
