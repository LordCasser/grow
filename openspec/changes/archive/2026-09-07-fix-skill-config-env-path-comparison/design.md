## Evidence
config::load_toml_file 调用 expand_env_vars_in_toml；update_config_at 使用 load_config_for_update 解析原始 TOML。add_skill_path/remove_skill_path 收到原始配置，却只处理 tilde 和 canonicalize。

## Decision
给原始配置比较增加一个局部解析函数，复用 crate::config::expand_env_vars_in_string，然后 resolve_skill_path。仅添加去重、ignore 清理和移除使用它。请求与计数保持原解析，避免再次展开已有值或改变用户传入的字面文件名。

## Verification
使用当前 HOME 的现有值，无全局环境改动或文件写入，验证变量形式配置可管理且保留原文。另外验证普通请求不会展开 ${HOME} 文本。
