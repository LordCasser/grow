## MODIFIED Requirements

### Requirement: Portable history preserves complete local tool exchanges

Portable 请求投影 SHALL 保留完整、无歧义的本地工具调用和匹配结果，并通过目标 backend 的结构化工具协议表达名称、合法 JSON 对象参数、关联 ID、结果正文及预算允许的图片。默认投影 SHALL 移除旧 provider reasoning、签名、加密数据、输出 item identity/status 和模型诊断；若当前路由以明确的协议错误要求回传 Responses `reasoning_text` 或 Chat Completions `reasoning_content`，则该路由 SHALL 仅回放 Surface 中既有的可见 reasoning 文本；Chat 历史 assistant 没有该文本时 SHALL 编码空字符串，仍 SHALL 移除 opaque identity、签名、加密内容及状态。投影 SHALL NOT 将历史调用作为新的执行请求。原始 Timeline 和既有隔离事实保持不变。

#### Scenario: Tool attachments include eviction text
- **WHEN** 工具结果附件同时含图片与图片预算产生的文本，或只剩替换文本
- **THEN** 三协议在 live 和 portable 请求中都保留附件顺序及文本，不因附件位于 images 字段而丢弃非图片内容。

#### Scenario: Restore or switch provider
- **WHEN** 会话恢复或切换模型/backend 后中性历史包含完整工具往返
- **THEN** Chat Completions、Responses、Messages 请求分别保留配对的工具协议和内容，默认不包含被撤销的 native reasoning，切回原模型也不复活 native；仅当前 Responses 或 Chat Completions 路由明确要求时可回放无 opaque 字段的可见 reasoning。

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
- **WHEN** 兼容回放已为当前 Responses 或 Chat Completions 路由启用，随后发生 native reset、相同路由参数更新或真正的 route 替换
- **THEN** 前两者保留当前路由兼容状态，真正 route 替换清除它；Messages、未学习路由以及与已学习 backend 不匹配的 wire 转换不接收该 portable reasoning。

#### Scenario: Chat route requires reasoning content after a switch
- **WHEN** 切换后的 Chat Completions 路由明确要求历史 assistant 回传 `reasoning_content`
- **THEN** 确认启用后，每条 assistant 使用紧邻它的已有可见 reasoning 按顺序编码（包括普通正文），无文本时使用空字符串；保留完整工具调用、结果和附件，不重放工具执行。

#### Scenario: Chat reasoning stays within its assistant boundary
- **WHEN** 历史含多段 reasoning、不同 assistant、User/System/ToolResult 边界或有效 native span
- **THEN** 可见 reasoning 不跨越无关边界绑定，native 原文保持且不重复，缺 reasoning 字段时仅补空字符串；Timeline 不改写。


### Requirement: Provider-required portable reasoning recovery is bounded

系统 SHALL 将明确的 Responses `reasoning_text` 或 Chat Completions `reasoning_content` 回传拒绝表示为类型化 attempt 事实。兼容状态只有在拒绝指向当前 backend、存在会改变 wire 的有效历史且当前路由尚未启用时才可改变；自动重提交 SHALL 使用同一 logical sampling 的剩余 attempt 上限与绝对期限。

#### Scenario: First explicit rejection enables replay
- **WHEN** 当前请求因缺少要求的 reasoning 字段首次被明确拒绝，且存在可改变的有效历史与恢复余额
- **THEN** ChatState 确认启用当前路由投影模式后静默重提交，不把失败候选加入 Surface。

#### Scenario: Repeated rejection does not loop
- **WHEN** 回放模式已经启用后端点仍返回相同拒绝，或不存在可改变的有效历史（Responses 仍要求完整工具往返前的非空 reasoning）
- **THEN** 系统不再次声明状态已改变，不重置 logical sampling 预算，并按既有其他恢复或终态路径处理。

#### Scenario: Chat history has no visible reasoning
- **WHEN** Chat 历史 assistant 没有可见 reasoning，端点明确拒绝缺失 `reasoning_content`
- **THEN** 当前 Chat 路由可确认一次空字段编码恢复，不能为满足字段要求虚构 reasoning 正文。

#### Scenario: Rejection identifies a different backend
- **WHEN** 错误要求的 reasoning 字段属于另一 backend，或错误不同时满足 400、thinking mode 与明确回传要求
- **THEN** 不启用当前路由的 reasoning 兼容状态。

验证入口：`sampling-types/src/conversation.rs` 的 wire 投影、`sampling-types/src/error.rs` 的分类、`chat-state/src/actor/tests.rs` 的路由状态测试与 `shell/src/session/actor/turn/sampling.rs` 的恢复测试。
