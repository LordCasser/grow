## Evidence
collect_config_skills 使用 expanded.starts_with(root)；list_skills_with_plugins 按 scope 排序，再执行同名合并。collect_discovered_paths 则已用 canonicalize 去重。

## Decision
仓库根在扫描循环前解析一次，配置根仅为 scope 比较解析；保留 expanded 用于实际发现，避免改变直接 SKILL.md 路径检查和原始路径标签。canonicalize 失败保留既有回退。

## Limits
只判断配置根身份；递归扫描中跨目录链接的逐文件 scope 策略与自动发现 scope_for_config_dir 单独审计。
