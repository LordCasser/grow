## ADDED Requirements

### Requirement: Skill descriptive frontmatter preserves declared string types
技能 `name`、`description`、`when-to-use`（及其 `when_to_use` 别名）、`license`、`compatibility` 和 `argument-hint` SHALL 仅把 YAML 字符串作为文本。非字符串 `name` SHALL 使用既有目录名回退；非字符串 `description` SHALL 使用既有正文描述回退；其他可选字段 SHALL 视为未提供。数值或布尔 SHALL NOT 被字符串化为技能身份、模型选择文本或展示文本。

#### Scenario: Wrong scalar type does not become skill identity
- **WHEN** `name` 为 YAML 布尔或数字，且存在有效目录名
- **THEN** 技能使用目录名，而不是字符串化后的布尔或数字。

#### Scenario: Wrong scalar type does not become selection text
- **WHEN** `description` 或 `when-to-use` 为 YAML 布尔或数字
- **THEN** description 使用现有正文回退且不标记为用户声明；when-to-use 为空，不作为模型选择短语或插件可见性条件。

#### Scenario: Optional display scalars require strings
- **WHEN** `license`、`compatibility` 或 `argument-hint` 为非字符串 YAML 标量
- **THEN** 对应字段未提供，其他合法技能元数据继续解析。

### Requirement: Skill allowed-tools declarations are displayed atomically
技能 `allowed-tools` SHALL 接受现有的分隔字符串或纯字符串列表。错误顶层类型或含非字符串元素的列表 SHALL 使整个展示字段缺省，不得静默显示部分列表；该字段 SHALL 继续只作为元数据，不改变工具授权。

#### Scenario: Mixed allowed-tools list
- **WHEN** `allowed-tools` 列表至少包含一个非字符串元素
- **THEN** 不展示部分工具声明，技能其他元数据仍正常解析。

#### Scenario: Valid allowed-tools forms
- **WHEN** `allowed-tools` 是现有分隔字符串或全字符串列表
- **THEN** 保持既有拆分结果与展示值。
