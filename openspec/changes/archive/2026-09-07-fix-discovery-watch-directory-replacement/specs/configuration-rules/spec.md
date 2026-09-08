## ADDED Requirements

### Requirement: Discovery watches follow replacement directories
项目 discovery watcher SHALL 在已存在的 .grow 及其 seed 目录被替换后继续接收更新；曾注册路径 SHALL 不等于当前目录已被监听。普通文件更新 SHALL 不导致无条件重建全部递归监听。

#### Scenario: 已存在的根删除后重建
- **WHEN** 启动时.grow存在，随后删除并重建.grow及workflows
- **THEN** 新workflows的后续修改仍可观察。

#### Scenario: 原子替换与普通修改
- **WHEN** 已监听 skills 或 .grow 被原子替换后执行刷新
- **THEN** 新实体得到注册；随后普通文件修改和重复刷新不重复注册。

#### Scenario: Skills watcher 根重建
- **WHEN** SkillsFileWatcher 的项目 .grow 删除后重建
- **THEN** 根目录及 skills 递归监听恢复，嵌套技能修改继续可观察。
