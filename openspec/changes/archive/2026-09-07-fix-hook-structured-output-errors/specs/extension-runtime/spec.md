## ADDED Requirements

### Requirement: Structured hook output errors are failures
命令与 HTTP Hook 决策解析 SHALL 将对象/数组前缀的 JSON 语法错误以及 JSON 数据/schema 错误分类为 Failed，不当作普通日志成功允许。空输出和普通非 JSON 文本保留既有规则。Prompt/Tool 命令 exit 2 SHALL 不因解析错误或未知决策而失去拒绝优先。

#### Scenario: 错字段或截断决策
- **WHEN** 成功执行输出决策对象但存在未知字段、字段类型错误或截断对象
- **THEN** 记录 Failed 并应用现有失败策略。

#### Scenario: 退出码拒绝与错误正文并存
- **WHEN** Prompt/Tool 命令以 2 退出且正文协议错误
- **THEN** 仍拒绝操作。

#### Scenario: 数组冒充决策对象
- **WHEN** 输出是 JSON 数组，包括空数组
- **THEN** 返回 Failed，不使用 Serde 的位置字段映射生成默认决策。
