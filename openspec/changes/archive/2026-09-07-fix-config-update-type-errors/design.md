## Evidence
load_config_from_toml 的 section 在 try_into 失败时返回 Default。update_config_at 使用该结果，save_config_at 合并 typed 段，默认 SkillsConfig 还会直接删除 skills 段。

## Decision
新增仅用于编辑的严格解析入口，缺失段使用默认值，存在段必须符合其现有类型。只严格处理保存函数会写回的六个区域，其他字段沿用当前读取以保留 Config API。toolset 存在时必须为 table，否则不能安全合并 ask_user_question。错误在闭包之前返回，不尝试自动纠错。

## Verification
真实临时文件包含 skills.paths 整数并保留 disabled 列表，修改其他设置必须返回错误且原字节不变；补齐六个区域的类型错误以及未知字段正常保留。不修改真实用户配置。
