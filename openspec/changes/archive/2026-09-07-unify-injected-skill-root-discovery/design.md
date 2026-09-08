## Decision
使用配置/插件已有的 find_skill_md_paths；不新写扫描器。目录本身仍需存在且为目录，不扩大为直接文件入口。根技能优先于子目录，沿用共享函数语义，重复路径仍经 collect_discovered_paths 去重。

## Evidence limits
字段可从配置反序列化进入发现，未发现仓内 launcher 写入者。本轮是明确扩展一致性契约，不将过去未定义的根支持描述为已存在保证。
