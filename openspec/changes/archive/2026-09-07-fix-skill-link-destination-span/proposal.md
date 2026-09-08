## Why
resolve_skill_internal_links 根据 parser 识别 URL 后，在原始链接范围内 rfind(url)；标题出现同名文件时会定位到标题并错误改写。必须准确定位目的地址，不能把标签、标题或代码中的同名文本当作目标。

## What Changes
先复现目标/标题同名冲突，再选择基于 Markdown 结构的目标范围定位。保留目录内文件检查及源码局部编辑策略。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能内部链接只改写目标。

## Impact
resolve_skill_internal_links 及正文加载，不顺带修改 frontmatter 或参数替换。
