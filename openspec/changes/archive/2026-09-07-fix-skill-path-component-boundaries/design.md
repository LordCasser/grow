## Evidence
agent filter_skills 已使用 Path::starts_with；shell 添加 ignore 清理、来源计数和新增计数却使用 str::starts_with。

## Decision
提取现有添加配置变换供生产和回归共用，使用 Path 的组件包含关系；新增计数和配置展示中的自定义路径计数复用来源计数函数。保留原有忽略祖先及后代清理政策与 paths 去重。测试使用局部 SkillsConfig 和 SkillInfo，不改 HOME 或真实配置。

## Limits
本次不处理 tilde、符号链接或不存在路径的额外归一化。reset 尚未找到仓内 launcher 注入来源，原债务保留。
