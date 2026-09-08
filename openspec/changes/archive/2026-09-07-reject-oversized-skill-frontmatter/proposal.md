## Why
读取到 opening fence 后超限会返回未闭合的部分 frontmatter，解析器按 NoFrontmatter 回退成普通技能，可能遗失 paths 或调用限制。应区分普通正文与已开始但超限的元数据。

## What Changes
已开始 frontmatter 且超过读取上限时返回 InvalidData，发现器跳过该技能；普通无 frontmatter 的长正文保持可发现。

## Capabilities
### Modified Capabilities
- configuration-rules: 超限 frontmatter 不降级为无限制技能。

## Impact
tools frontmatter reader 与发现器；保留现有读取预算和精确上限接受规则。
