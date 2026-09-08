## Findings
未闭合 header 的旧发现回归失败。首次仅改外层 YamlError 处理后，损坏 YAML 仍加载；进一步核对发现解析器内部存在全字段重新引号化与标量恢复，外层错误并未触发。最终直接传播 YAML 解析错误，停止生产自动修复。

## Validation
低磁盘配置 tools implementations::skills 101 passed，agent prompt::skills::tests 96 passed。四个旧损坏恢复场景保留输入并改为实际发现拒绝回归。tools 字段转换与 agent 冒号测试同时验证未引号化冒号拒绝、合法引号化版本的字段和值保持有效。普通无 header Markdown 仍加载并提取正文描述。

## Reviewable cleanup
quote_problematic_values、recover_scalar_fields、RECOVERABLE_KEYS 已无生产调用，列入临时清单 R9，保留定义并注明待确认删除。没有删除候选实体。

## Limits
字段级宽松转换未改；独立正文提取语义未改。不代表所有元数据类型校验完成。CLI 未重新链接，target 9.8 GiB，可用约 69 GiB。
