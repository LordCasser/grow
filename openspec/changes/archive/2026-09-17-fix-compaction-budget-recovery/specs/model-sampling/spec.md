## ADDED Requirements

### Requirement: Recap retains completed tool evidence
Recap SHALL 保留冻结输入中可合法配对的已完成工具调用、结果、附件与 Assistant 正文，不因它们位于尾部而删除。未完成调用 SHALL 不输出悬空协议；预算裁剪后的请求仍保持配对。

#### Scenario: Complete and partial tool batches at recap boundary
- **WHEN** recap 在完整或部分完成的工具批次后生成
- **THEN** 请求保留已完成调用及结果与普通正文，排除未完成调用，主 Surface 不变。

#### Scenario: Recap requires budget trimming
- **WHEN** recap 输入超预算而需要去掉较早历史
- **THEN** 最新可保留工具证据进入请求且不存在孤立工具协议。
