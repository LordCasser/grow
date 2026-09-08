## Decision
描述读取使用独立 Read helper，预算为 MAX_FRONTMATTER_BYTES + MAX_BODY_PEEK_BYTES，额外一字节辨别截断。截断导致末尾 UTF-8 不完整时丢弃不完整字符；预览内部非法 UTF-8 或文件本身残缺仍返回错误，调用者使用技能名。之后沿用 extract_skill_body 和既有正文 2 KiB 截取/段落提取。

## Limits
这是有界描述预览，可能不再越过很长的前导空白寻找正文。显式正文加载无变化。frontmatter 独立读取预算仍单独计算，不把本轮预算表述成所有 metadata I/O 的总量。
