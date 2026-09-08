## Evidence
config::load_from_disk -> load_config_file -> load_toml_file 展开环境变量，再应用版本覆盖；update_config 将所得 typed 值合并进原始文件。save 虽重新读取原文，merge_section 仍用展开后的 typed 字段替换。

## Decision
提取内部 read_config_for_save(path) 供保存与读改写共享，返回原始 TOML 且仅 NotFound 创建空表。update_config 继续持有原锁，委托显式路径 update_config_at 方便真实临时文件验证；不使用全局 GROW_HOME。显式 save_config 的调用者传入什么值仍保存什么值，不在此追溯来源。

## Limits
本次不处理 typed section 容错回落、跨进程修改或已有版本覆盖阻止用户当前设置生效的产品语义。
