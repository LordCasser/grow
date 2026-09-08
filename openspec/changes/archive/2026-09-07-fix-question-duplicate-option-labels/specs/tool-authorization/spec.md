## ADDED Requirements

### Requirement: Question option labels are unambiguous
ask_user_question SHALL 在发送请求前拒绝同一道题内完全相同的选项 label，并返回参数错误；不同题之间相同 label SHALL 允许。选项 ID 或描述差异 SHALL 不使重复 label 合法。

#### Scenario: 同题重复选项
- **WHEN** 同一问题的两个选项 label 相同
- **THEN** 工具返回参数错误，不向 coordinator 发送请求。

#### Scenario: 不同题共用选项文本
- **WHEN** 不同问题均有同名选项，但各题内部 label 唯一
- **THEN** 不因跨题重复拒绝请求。
