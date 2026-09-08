## Why
技能正文块的 name/args 与引用索引的 name/path 直接插入双引号 XML 属性。普通引号参数或含特殊字符的文件路径会破坏提示结构。

## What Changes
复用已有测试 helper escape_xml 处理正文块 name/args、引用索引 name/path 及预加载消息 name/description/path，正文保持原样。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能提示属性转义。

## Impact
tools 的共享技能块与索引包装。不改变参数替换、正文或引用去重身份。
