## Decision

复用 `CanonicalToolMeta::new` 和单一 `TOOL_META_VERSION`，不在各工具或调用方分别打版本。现有 `ToolKind` 序列化保持闭合；新 list/status/wait 词汇统一进入版本 6。Typed 消费者按新 schema 更新，不能将 v6 内容标为 v5 以掩盖接口变化。

这里只修复既有词汇版本规则的遗漏。不增加 parser fallback、旧词汇转换、持久化迁移或新的元数据包装。

## Verification

通过真实 `CanonicalToolMeta` 构造/序列化检查三类新 kind 的版本、名称和 access；执行既有 schema 生成及无更新变量的精确检查。重新构建最终 CLI，避免本地候选仍嵌入旧常量。其他发布回归失败仍由发布准备记录保留。
