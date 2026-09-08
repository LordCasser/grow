## ADDED Requirements

### Requirement: Raw skill config comparisons expand environment references
技能管理 SHALL 在比较原始配置路径时应用运行时配置的环境变量展开规则，再解析路径；不得将该展开写回保留条目，也不得对普通请求路径额外展开。

#### Scenario: Manage environment based configured path
- **WHEN** paths 与 ignore 使用环境变量引用一个现有目录，用户添加该真实目录
- **THEN** 清除对应 ignore、不重复添加、保留原 paths 引用；按真实路径移除时删除对应项。

#### Scenario: Literal request path
- **WHEN** 普通请求的路径文本包含 ${HOME}
- **THEN** 路径解析保留该字面文本，不将配置展开规则应用到请求。
