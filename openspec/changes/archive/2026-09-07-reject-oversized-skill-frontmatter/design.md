## Decision
在现有 total_bytes 超限分支按 found_opening 区分：已识别元数据则报错，否则维持普通正文回退。不提高读取预算，不从截断元数据推测限制。parse_skill_files 已在读取错误时跳过，因此无需新错误协议。

## Limits
本轮仅处理超过读取预算；未闭合但未超限的 header 和 YAML 错误回退策略单独审计。
