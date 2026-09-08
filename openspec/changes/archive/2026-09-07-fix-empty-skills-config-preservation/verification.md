## Reproduction
旧实现 empty_skill_settings_preserve_unknown_fields 失败：仅包含未知子表的 skills 在无关 UI 设置保存后消失。

## Validation
shell util::config::persist：60 passed。新回归覆盖未知子表保留、最后已知项清空、真正空段删除；既有原始值、类型错误与读取错误保护回归同时通过。macOS linker 既有 __eh_frame 警告未影响结果。磁盘剩余约 73 GiB。

## Scope
真实临时文件测试，不修改用户配置；保持已知字段清空和真正空段删除的行为。字段集合来自当前 SkillsConfig 默认值序列化（五个字段均包含空数组），未引入第二份硬编码名单。
