## Why
未闭合 header 和 YAML 语法错误目前会被当作缺少元数据，生成默认调用限制的技能。文件明确表达了元数据意图，解析失败时不能推断限制不存在。

## What Changes
已开始但未闭合 frontmatter 返回读取错误；YamlError 跳过技能。合法无 frontmatter Markdown 保留目录名及正文描述回退。

## Capabilities
### Modified Capabilities
- configuration-rules: 损坏技能元数据拒绝加载。

## Impact
tools 技能发现。以 --- 开始但无闭合的文档按损坏 header 处理；不修改正文提取工具的独立语义。
