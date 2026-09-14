## MODIFIED Requirements

### Requirement: Portable history preserves complete local tool exchanges

Portable 请求投影 SHALL 保留完整、无歧义的本地工具调用和匹配结果，并通过目标 backend 的结构化工具协议表达名称、合法 JSON 对象参数、关联 ID、结果正文及预算允许的图片。默认投影 SHALL 移除旧 provider reasoning、签名、加密数据、输出 item identity/status 和模型诊断；若当前 Responses 路由以明确的协议错误要求回传 `reasoning_text`，则该路由 MAY 仅回放 Surface 中既有的可见 reasoning 文本，仍 SHALL 移除 opaque identity、签名、加密内容及状态。投影 SHALL NOT 将历史调用作为新的执行请求。原始 Timeline 和既有隔离事实保持不变。

#### Scenario: Tool attachments include eviction text
- **WHEN** 工具结果附件同时含图片与图片预算产生的文本，或只剩替换文本
- **THEN** 三协议在 live 和 portable 请求中都保留附件顺序及文本，不因附件位于 images 字段而丢弃非图片内容。

#### Scenario: Restore or switch provider
- **WHEN** 会话恢复或切换模型/backend 后中性历史包含完整工具往返
- **THEN** Chat Completions、Responses、Messages 请求分别保留配对的工具协议和内容，默认不包含被撤销的 native reasoning，切回原模型也不复活 native；仅当前 Responses 路由明确要求时可回放无 opaque 字段的可见 reasoning。

#### Scenario: Ambiguous or incomplete history
- **WHEN** portable 区域包含未配对、重复或无效工具记录
- **THEN** 不输出悬空调用、孤立结果或歧义配对，不伪造工具结果或修补非法 JSON；原始持久化证据不变。

#### Scenario: Valid same-route native continuation
- **WHEN** 请求仍有当前 epoch 的有效 native span
- **THEN** 该 span 继续使用完整原生内容，portable 处理不删掉其 thinking 或重复生成工具调用。

#### Scenario: Target route requires reasoning text replay
- **WHEN** Responses 路由对含 portable 工具历史的请求返回明确 400，声明 thinking mode 必须回传 `reasoning_text`
- **THEN** 系统在当前路由内启用可见 reasoning 回放，确认投影状态后按同一 logical sampling 余额重建请求，reasoning、function call 与结果各保留一次。

#### Scenario: Replayed portable reasoning remains narrow
- **WHEN** 兼容回放已为当前 Responses 路由启用，随后发生 native reset、相同路由参数更新或真正的 route 替换
- **THEN** 前两者保留当前路由兼容状态，真正 route 替换清除它；Chat Completions、Messages 和未学习的 Responses 路由不接收该 portable reasoning。

## ADDED Requirements

### Requirement: Provider-required portable reasoning recovery is bounded

系统 SHALL 将明确的 Responses reasoning 回传拒绝表示为类型化 attempt 事实。兼容状态只有在当前 Surface 含可回放的非空 reasoning 且当前路由尚未启用时才可改变；自动重提交 SHALL 使用同一 logical sampling 的剩余 attempt 上限与绝对期限。

#### Scenario: First explicit rejection enables replay
- **WHEN** 当前请求因缺少要求的 `reasoning_text` 首次被明确拒绝，且存在可回放 reasoning 与恢复余额
- **THEN** ChatState 确认启用当前路由投影模式后静默重提交，不把失败候选加入 Surface。

#### Scenario: Repeated rejection does not loop
- **WHEN** 回放模式已经启用后端点仍返回相同拒绝，或 Surface 没有可回放 reasoning
- **THEN** 系统不再次声明状态已改变，不重置 logical sampling 预算，并按既有其他恢复或终态路径处理。
