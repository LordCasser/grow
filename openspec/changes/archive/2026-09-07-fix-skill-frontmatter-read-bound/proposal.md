## Why
read_frontmatter_only 在 read_line 完成后才检查 4096 字节上限，超长单行会先被无界读入并分配。需要在底层读取阶段限制，而非仅限制最终保留内容。

## What Changes
底层读取最多 MAX_FRONTMATTER_BYTES+1 字节，用探测字节判断超限；超限行不加入 metadata，保留既有返回类型。使用字节行缓冲，先判断超限再解码，避免边界截断合法 UTF-8 误报。

## Capabilities
### Modified Capabilities
- configuration-rules: frontmatter 读取累积上限。

## Impact
仅流式 metadata helper；不改变显式正文加载或 YAML 字段恢复。
