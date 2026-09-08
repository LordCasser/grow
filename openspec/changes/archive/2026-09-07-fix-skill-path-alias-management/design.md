## Evidence
配置路径在发现器中相对进程 cwd 解析；请求路径相对请求 cwd 解析。管理入口需保持这个区别，不能将已有配置重新锚定到请求目录。

## Decision
使用已有 resolve_skill_path 进行比较时展开和 canonicalize。添加对 ignore 的祖先/后代比较和 paths 去重解析配置路径；移除同样比较解析值；计数解析目录和技能路径。不将比较结果批量写回配置。

## Limits
canonicalize 失败继续沿用已有回退策略，不承诺推断已删除 symlink 的历史目标；不存在路径的词法归一化另行处理。
