## ADDED Requirements

### Requirement: Delivery review bindings are actionable in model projections

模型可见的未闭合必需交付摘要 SHALL 提供与 review 输入同名的准确 linkId、resultId、reviewedSourceRevision，保留 resultId 的完整字符串或 null；request-only 规划 link 投影 SHALL 使用相同绑定。提示 SHALL 说明复制绑定后仍需明确选择验收决定和依据，不把 task id 或单独 seq 当成 resultId。既有有界摘要可分批显示，不能因展示截断免除剩余义务。

#### Scenario: Review a delivered result from the Todo summary
- **WHEN** 必需 child 已交付，模型从 todo_write 摘要复制绑定并附上合法 accepted 决定及验证依据
- **THEN** 原 actor 可持久接受该 review，关闭对应交付阻塞；恢复后仍保留该准确验收。

#### Scenario: Release a terminal delivery with an incorrect result identity
- **WHEN** released 携带 null 或单独 seq，但 link 已绑定不同的完整 resultId
- **THEN** actor 在持久提交前拒绝并返回该 link 可复制的准确绑定，原计划保持不变；用准确绑定和理由重试可以正常释放。

#### Scenario: Release a task without a delivered result
- **WHEN** task 尚无结果，模型复制含 null resultId 的绑定并提供 released 理由
- **THEN** 不要求虚构结果，沿原执行和等待处置规则提交；活跃执行仍可阻止完成。

#### Scenario: Locate a review beyond the bounded summary
- **WHEN** 有多于摘要上限的交付，模型对未展示 link 提交错误绑定
- **THEN** 错误回执返回请求指定 link 的准确绑定，不能返回另一项或让模型继续猜测身份。
