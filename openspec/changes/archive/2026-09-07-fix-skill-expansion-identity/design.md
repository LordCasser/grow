## Decision
利用 ParsedSkillRef 已有 skill_path、qualified_name、plugin_name 与 SkillInfo 比较，不增加新身份字段。路径、format_skill_name 及插件名必须全部相同，未找到则保持失败结果。插件名的独立比较避免插件名恰好等于 scope 标签时混淆来源。

## Evidence and Verification
agent merge_skills_with_plugins 保留原生与插件项，插件间按 dedup_key 去重，不以文件路径去重。回归使用同一路径的原生/插件条目及不同正文快照、插件变量，验证选择插件只展开插件正文；从 catalogue 删除该插件后不得回退到原生项。
