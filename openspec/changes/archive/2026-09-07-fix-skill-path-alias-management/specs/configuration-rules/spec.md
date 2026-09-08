## ADDED Requirements

### Requirement: Skill management compares resolved path aliases
技能管理 SHALL 在添加去重、取消 ignore、移除和来源计数时比较解析后的文件系统路径。比较不得重写保留条目的配置原文。配置相对路径沿用发现器的进程工作目录基准，请求相对路径使用请求工作目录。

#### Scenario: Add already configured symlink target
- **WHEN** 配置 paths 和 ignore 使用指向现有技能目录的符号链接，用户添加其真实路径
- **THEN** 清除对应 ignore，不新增重复路径，已有 paths 写法保留。

#### Scenario: Remove or count a symlink source
- **WHEN** 配置使用现有符号链接，用户按真实路径移除，或统计该来源
- **THEN** 移除对应配置项，来源计数包含真实路径下的技能且不包含相邻目录。
