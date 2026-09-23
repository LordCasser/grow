## MODIFIED Requirements

### Requirement: Agent communication tools expose their intent and result

通信工具 SHALL 在同一发送工具行显示实际工具身份、参与方、简短状态和有界正文。固定 UI 文案 SHALL 使用英文，消息正文 SHALL 保持原语言。常规列表、展开与正文详情 SHALL NOT 增加 Sideband、主上下文去向或投递模式的解释行；完整身份、原始参数和回执数据 SHALL 在按需详情中可检查。状态 SHALL 来自结构化事实，不通过错误文案猜测投递结果。

#### Scenario: Parent sends queued guidance
- **WHEN** agent 发送消息并收到 durable receipt
- **THEN** 原工具行由 Sending 更新为 Received，显示目标和正文；不另加 ACK 行，不显示已读、已生效或任务完成，也不追加投递机制解释。

#### Scenario: Immediate guidance and uncertain acknowledgement
- **WHEN** 父消息请求安全中断，或已派发的消息因超时、断连或取消无法确认接收
- **THEN** 安全中断参数在按需数据中保留且不推断不可中断工具已停止；无法确认时原行显示 Unconfirmed 和简短原因，不将其当作未送达，不自动重发。

#### Scenario: Parent child or peer inquiry
- **WHEN** ask 得到 Sideband 答案
- **THEN** 原行显示 Answered、问题摘要和答案摘要，接收侧身份准确，完整问答可展开。

#### Scenario: Inquiry state lookup
- **WHEN** get_inquiry 成功查询到失败的 inquiry
- **THEN** 查询成功与 inquiry 失败分别表达，不将两者状态合并。

### Requirement: Parent message receipt appears in its child view

子 agent SHALL 在父消息持久接收后显示一条有稳定 receipt 身份的接收记录，保留来源和原文。常规 UI SHALL 使用英文简洁标题，不强制展示投递模式；原始参数可在按需数据中查看。接收记录 SHALL NOT 冒充人类输入、接收方主动发送工具或额外模型消费。

#### Scenario: Receipt while the child is busy
- **WHEN** child 持久接收消息但尚未消费
- **THEN** 所属视图显示 Message from parent 及正文，不抢焦点，不声称已经执行。

#### Scenario: Duplicate or replayed receipt
- **WHEN** 同一 receipt 在重试、重连或消费后冷恢复中再次投影
- **THEN** UI 保持一条记录，不重复消费或触发副作用。

#### Scenario: Receipt projection fails after commit
- **WHEN** durable commit 成功但 UI 发布失败
- **THEN** 回执仍有效，恢复从持久事实补显示，不重复发送。

#### Scenario: Normal and minimal history
- **WHEN** 在 normal/minimal 或未选中的子视图接收消息
- **THEN** 记录属于正确 Session，Minimal 只打印一次不可变收件事实，主 turn 完成不终止独立 inquiry 行。

## ADDED Requirements

### Requirement: Communication bodies support Markdown and lossless source access

通信记录的消息、问题与回答 SHALL 支持正文 Markdown 展示，并与方向、身份、状态和错误等界面字段分开渲染。折叠记录 SHALL 按状态提供以显示宽度计算的有界预览：等待询问最多两行问题，已回答最多一行问题加两行答案，询问失败最多一行问题加两行原因，父消息最多两行消息；投递未知 SHALL 保留原因预览。展开正文和详情 SHALL 保留全文浏览、选择及原始 Markdown 复制入口，原始协议数据 SHALL 可单独查看，默认正文 SHALL NOT 重复附上整份 JSON。查看模式 SHALL NOT 改写存储原文、模型输入或原始工具结果。

#### Scenario: Answer becomes visible without opening details
- **WHEN** 等待中的询问完成并返回答案
- **THEN** 同一默认行保留问题摘要并展示答案摘要；预览直接派生自已有正文，不启动额外模型总结，失败时在同一位置显示实际原因。

#### Scenario: Markdown question and answer
- **WHEN** 询问的问题或回答含标题、列表、引用、行内代码、代码块、链接或表格
- **THEN** 详情将问题与回答分为独立正文区域并使用现有 Markdown 能力展示，原文视图和复制保留 Markdown 源文本及代码缩进。

#### Scenario: Exact source differs from rendered Markdown
- **WHEN** 问题、答案或消息含 tab、CRLF、soft break、围栏或复杂链接语法
- **THEN** 原文复制保留实际接收的正文字符串，不从展开 tab、合并 soft break 或添加元信息的渲染结果重建。

#### Scenario: Parent message and incoming inquiry use the same body behavior
- **WHEN** 同样的 Markdown 出现在父消息接收通知、询问发送行或询问接收行
- **THEN** 三个入口提供一致的正文展示、原文访问和复制行为，图片路径或 Markdown 图片引用不替代整条文字记录。

#### Scenario: Body cannot override communication metadata
- **WHEN** 正文含看似“已接收”“来自父 Agent”的标题，或任务名含 Markdown 控制字符
- **THEN** 界面元信息仍由结构化事实产生并与正文分区，正文不改变真实参与方、交互模式和状态。

#### Scenario: Full source after clipping and resize
- **WHEN** 中文、emoji、长代码行或表格在小宽度下被预览截断并随后调整终端宽度
- **THEN** 正文重新排版，截断有明确标记，全文和复制不丢失；两个不同正文区之间不发生代码围栏或表格结构串扰。

#### Scenario: Replay and minimal rendering
- **WHEN** 通信记录通过 normal、minimal 或重连回放展示
- **THEN** 各入口保持相同语义及原文，Minimal 仍只在询问自身终态后追加接收行，父消息不可变收件通知仍仅追加一次，显示切换不触发新的消费或 Hook。

#### Scenario: Keyboard reading preserves established navigation
- **WHEN** 用户从选中的通信记录进入详情，切换正文/原文/数据并返回
- **THEN** 现有展开、详情、搜索、选择、换行和关闭操作保持可用，输入状态不误触查看动作，Tab 不被改成详情分页；退出恢复原记录的焦点和位置。

#### Scenario: Result arrives during reading
- **WHEN** 新的终态内容到达而用户已滚动离开底部或正在选择正文
- **THEN** 状态更新仍关联原记录，不抢焦点或跳到其他 Session，不以新内容替换正在复制的选区；显示更新后仍可查看最新完整结果。
