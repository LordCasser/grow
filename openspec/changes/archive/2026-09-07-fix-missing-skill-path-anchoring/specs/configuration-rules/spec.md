## ADDED Requirements

### Requirement: Missing skill paths are anchored before canonicalization
在进程工作目录可读取时，技能路径解析 SHALL 先将相对请求 cwd 与路径锚定为绝对路径，不以目标是否存在为条件。

#### Scenario: Missing target with default cwd
- **WHEN** 目标不存在，技能请求使用默认点 cwd 和相对路径
- **THEN** 返回锚定到当前进程目录的绝对路径，添加保存该路径。

#### Scenario: Missing target with relative cwd
- **WHEN** 目标不存在，请求 cwd 本身为相对目录
- **THEN** 将 cwd 与目标一起锚定到当前进程目录，不返回相对配置路径。
