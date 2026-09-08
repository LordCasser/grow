## ADDED Requirements

### Requirement: Skill substitutions do not reinterpret inserted values
技能模板替换 SHALL 只解释原始正文中的 token，插入的参数或上下文值 SHALL 按字面保留。显式 $ARGUMENTS[N] 缺失索引 SHALL 替换为空。

#### Scenario: 参数包含占位符文本
- **WHEN** 参数本身含有 $0 或 ${SESSION_ID} 等文本
- **THEN** 插入后不再展开这些文本。

#### Scenario: 上下文值含占位符
- **WHEN** skill_dir 等上下文值含其他 token 文本
- **THEN** 值作为原文插入，不级联展开。

#### Scenario: 显式大索引
- **WHEN** 正文引用不存在的 $ARGUMENTS[1000000]
- **THEN** 该 token 变为空，不产生部分替换残留。
