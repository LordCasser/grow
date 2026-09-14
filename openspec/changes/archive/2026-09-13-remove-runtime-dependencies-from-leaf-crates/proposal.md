## Why
纯采样类型、HTTP、渲染和更新模块因少数值类型或配置入口依赖 tools/workspace/sampler/shell 运行时，扩大编译传播并模糊状态所有权。

## What Changes
- 共享工具定义与模型图片输入 key 移到已有 tool-types。
- 客户端身份值类型移到已有 config-types，HTTP 不再依赖 workspace/sampler。
- 权限 cursor 由 Pager 传入特殊行语义，renderer 不再依赖 workspace。
- 版本策略下沉 config，更新配置由 CLI 组合层传入并继续复用现有读写所有者，update 不再依赖 shell。
- 不新增 crate，不修改 wire 格式、配置优先级、更新决策或权限行为。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
无。纯依赖方向重构，skip_specs: true；不虚构行为 delta。

## Impact
Cargo manifest/lock、对应类型及调用方、开发说明。保留测试的行为预期，不以重写测试掩盖迁移差异。

