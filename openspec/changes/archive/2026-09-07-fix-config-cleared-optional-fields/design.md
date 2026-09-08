## Evidence
set_cancel_subagents_on_turn_cancel(ask)、set_screen_mode(空) 和 persist_models_default(None) 都将对应字段设为 None。merge_toml_tables 只迭代新键，原键不被删除。

## Decision
保留修改前 Config 克隆。对实际写回区域比较前后序列化树，递归删除 before 有而 after 没有的键；不遍历未知键来推断删除。沿用后续原始文件读取、合并与原子写入。抽出接受已读 TOML 的保存实现供 update 使用，公开入口继续原样委托。

## Limits
不改变显式 save_config 的整份写入契约；版本覆盖等更高优先级仍可能影响运行时解析，不在此改配置优先级。
