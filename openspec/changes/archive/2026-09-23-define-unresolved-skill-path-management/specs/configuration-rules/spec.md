## MODIFIED Requirements

### Requirement: Skill management compares resolved path aliases
技能管理 SHALL 在添加去重、取消 ignore、移除和来源计数时比较解析后的文件系统路径。比较不得重写保留条目的配置原文。配置相对路径沿用发现器的进程工作目录基准，请求相对路径使用请求工作目录。目标不存在时，保存或比较的是当次锚定路径表达式；后续请求按当前文件系统重新解析，不从已删除符号链接推断历史目标。移除无匹配项 SHALL 明确报告未匹配，不声称已经移除。

#### Scenario: Add already configured symlink target
- **WHEN** 配置 paths 和 ignore 使用指向现有技能目录的符号链接，用户添加其真实路径
- **THEN** 清除对应 ignore，不新增重复路径，已有 paths 写法保留。

#### Scenario: Remove or count a symlink source
- **WHEN** 配置使用现有符号链接，用户按真实路径移除，或统计该来源
- **THEN** 移除对应配置项，来源计数包含真实路径下的技能且不包含相邻目录。

#### Scenario: Symlink removed after a canonical path was saved
- **WHEN** 添加时别名解析为现有目标并保存规范路径，随后符号链接被删除，用户按旧别名请求移除
- **THEN** 不凭旧别名删除无法证明的目标，响应明确报告未匹配；按保存的规范路径移除时仍能删除该项。

### Requirement: Missing skill paths are anchored before canonicalization
在进程工作目录可读取时，技能路径解析 SHALL 先将相对请求 cwd 与路径锚定为绝对路径，不以目标是否存在为条件。缺失目标的 `..` 组件 SHALL 保留，不能词法折叠跨过将来可能出现的符号链接。技能 add/remove 的请求 cwd 若不能规范化为可读取目录，SHALL 在配置更新前失败，而不保存相对或猜测路径。

#### Scenario: Missing target with default cwd
- **WHEN** 目标不存在，技能请求使用默认点 cwd 和相对路径
- **THEN** 返回锚定到当前进程目录的绝对路径，添加保存该路径。

#### Scenario: Missing target with relative cwd
- **WHEN** 目标不存在，请求 cwd 本身为现有的相对目录
- **THEN** 将 cwd 与目标一起锚定到当前进程目录，不返回相对配置路径。

#### Scenario: Missing target contains parent traversal
- **WHEN** 缺失路径的中间组件后带有 `..`
- **THEN** 目标仍缺失时，锚定表达式保留该组件，不把它词法折叠成另一个路径；目标实际存在后才按真实文件系统解析。

#### Scenario: Request cwd cannot be read
- **WHEN** 技能 add/remove 的 cwd 不存在、不是目录或无法读取
- **THEN** 返回参数错误，不更新配置，也不把相对路径保存到配置中。
