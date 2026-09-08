## Evidence
read_frontmatter_only 使用 line.trim() == "---"；另两处使用 starts_with("---") + find("\n---")，会把非独立行作为分隔符。

## Decision
共享 split_skill_frontmatter 返回借用 YAML 和 body；按行包含换行符遍历，trim 后恰好等于 --- 才识别开闭边界。正文返回策略仍 trim_start；完整成对 --- 行仍按 frontmatter 处理，不试图推断其是 Markdown 水平线还是元数据。复用原 YAML 解析/恢复，不混入语义校验或大小限制改变。
