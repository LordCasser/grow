## Reproduction
旧实现错误 paths 类型仍产生技能，实际文件发现回归失败。

## Validation
低磁盘配置 tools implementations::skills 102 passed，agent prompt::skills::tests 96 passed。新增测试拒绝数字、布尔、映射、混合列表与嵌套列表；接受 null、空列表、空字符串、** 和正常字符串列表，已有空格与花括号分隔回归保持通过。

## Limits
错误通过现有 SkillParseError::YamlError 传播，不新增错误协议；尚未改变其他字段的宽松转换或 glob 语法处理。CLI 未重新链接。磁盘可用约 68 GiB，target 9.8 GiB。
