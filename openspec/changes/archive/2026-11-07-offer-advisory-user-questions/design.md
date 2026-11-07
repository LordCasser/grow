# Design

## 两种模式与参数

沿用一个 `ask_user_question` 工具，不增加第二个工具。输入增加 `blocking: bool`（缺省 true）；Agent 可以明确 false 选择非阻塞，有必须用户决定的前置条件时选择 true。每题增加可选 `recommended_option: String`，值精确匹配本题某个显式 option.label；有效非阻塞提问必须每题都提供一个合法推荐项，不从中文/英文 Recommended 字样或排列猜测默认值。多选题可以有一个推荐初始选项，用户之后仍可多选。其他重名/问题文本验证保持。

用户偏好存 `[toolset.ask_user_question].allow_non_blocking`，默认 true。Settings → Agent 显示 “Allow non-blocking questions”，开时由 Agent 逐次决定，关时即使请求 blocking=false 也走现有阻塞形态。沿既有 config 类型、env > effective TOML > default resolver 和 raw update_config/merge 持久化，不新建偏好文件。Shell Factory 注入 `UserQuestionPolicyResolver`，Tools 每次调用通过它取得本次参数快照；只刷新新增的非阻塞偏好，现有 timeout 参数继续使用当前 Agent 构建时的有效值。设置保存后影响下一次 ask 准入，已开始的问题保持其已承诺模式。非阻塞不做成短用户回答超时。

同一次准入选择的 effective blocking 值同时决定工具实际等待形式和ACP客户端显示。不能只改变Pager而让工具继续等。Settings说明实际配置优先级；环境强制覆盖本地设置时不能暗示本地保存改变了最终策略。这里不预建新的 requirements/remote 参数层或设置广播框架。

## 非阻塞不持有旧工具回答者

有效 nonblocking 调用的链路是：验证明确推荐项 → Shell 将结构化问题、原 tool_call_id/来源和建议投影交给客户端 → 客户端展示成功后工具返回“建议已展示，正按以下临时假设继续；用户尚未回答”。它不是 Accepted 用户答案，不产生 PermissionEvidence，也不能代替部署、权限、Plan approval 等必须用户决定的门槛。模型仍必须遵循普通权限与当前任务授权。

模型结果必须保留题目、推荐标签与解释，并明确 provenance=Agent recommendation。现有 `AskUserQuestionOutput.message` 可以承载模型文字；有效 mode 与推荐项通过同次展示的结构化元数据给 Pager。展示失败要明确失败，不能谎称用户已看见。工具只等展示准入的既有有界回执，不等用户答案；不能把 old oneshot receiver drop 后还要向它送答案当作实现。

Pager 显示可收起的非阻塞问答卡片，标明“Agent 正按 A 继续，可修改”。它不设置 session.question_pending，也不累计等待用户导致的 turn 暂停时间，不抢走正常 composer。用户点“修改”时复用 QuestionView 的本地选择/preview/Other 编辑能力；关闭只关本地编辑，不取消正在运行的模型。

用户提交修改后，Pager把题目、原建议、实际选择/自由文本和来源 id 组织为**本次真实用户输入**，走既有 session/prompt 的 HumanIntent/Hook/immutable payload/durable ACK/FIFO 接纳。仅在接纳成功后显示“已提交”；失败保留输入以便重试。相同回复重试使用稳定 prompt/input id，不多次入队。不把它 append 为旧工具的第二个 result，不直接改旧 Turn、清空 pending guard 或伪造旧 request ACK。无论原任务仍工作、已结束或 Goal 已停止，回复都遵从正常用户输入仲裁；不自动 restart 一个已完成/暂停 Goal。

卡片以已有工具参数/结果和 Grow 展示记录恢复，不另建无限 pending-question 表。来源子会话已经终结时，选择界面明确将修改发给 owning primary 的当前输入入口并附原问题来源，不向封闭 child Timeline 写事实；如果 primary 也已不可用则保留可复制内容并明确失败，不暗中创建新会话。

真实鼠标迟答验收发现，旧卡片实测高度被布局重建替换为估计后，新 prompt 的 virtual_y 改变，而 page-flip preserve 仍保留旧绝对 offset，意外激活 sticky prompt 并排除首行回复。修复归现 Scrollback layout/follow 所有者：固定位置跟随同一内容在当前布局中的坐标；新回复仍能放在 prompt 下方时保持顶部位置，真正溢出继续消费 preserve 并跟随底部。复用现有锚定边界，不加问答专用导航或强制全历史测量，不改变双击折叠、输入与结算语义。证据和交接见 [U7 审查](review-follow-pin.md)。

## 阻塞路径

blocking 继续用现有 UserQuestionRequest → worker → ACP ext_method → QuestionView → durable InteractionResolved → tool result。先验证迟答/timeout/cancel旧实现是否存在实证缺陷，不能沿“缺少取消通知”的错误假设另建协议；现有 pending guard resolve/drop 已发 InteractionResolved，Pager 已有按 session/tool_call_id 关闭投影。新字段不能改变这条原有归属，也不能凭UI已关闭认定 durable ACK 成功。

如果用户关闭非阻塞偏好，模型请求 false 被强制为 true：工具等待真实答复，界面是阻塞问答而非建议卡片，推荐项只作为参考。原 timeout_enabled=false 仍可无限等待用户，这是用户当前策略；不能与工具展示或结算 I/O 的等待混同。

## 共享接口与增量交接

Tools 参数的冻结方向：`AskUserQuestionInput.blocking`，`Question.recommended_option: Option<String>`，`AskUserQuestionParams.allow_non_blocking: bool`。沿现有 in-process request 与 ACP request/notification增加有效 mode，区分 Blocking 与 Advisory，Plan/Default 的原行为模式保持独立。禁止用 Plan flag 同时表示是否阻塞。

首先实现 Tools 参数验证与结果来源，随后 Shell 配置/准入及既有问答 producer，最后 Pager Settings/卡片/本地回答消费方。跨 crate exact wire struct 在派发前由父结合迟答判别结果冻结；本设计先固定行为，不让两侧自行猜 ack、来源或目标 session。不同模块可在隔离范围准备，同一次集成编译前必须全部适配。

验收采用真实 mock-provider 链：Agent 发 advisory ask 后在无人回答时继续下一次模型请求；blocking 情况不继续；Settings 强制阻塞生效并 cold restart 保留；之后用户选择通过普通输入接纳且原工具 result 仍只有一个。补 late reply 在原任务结束/Goal停后、新问已出现、重复提交/客户端重连、write/ACK失败场景。现有 Plan interview、Other notes、普通问答 timeout/cancel 及设置 raw field preservation 回归必须保留。模型池的实现审批不阻塞此项，但同文件写入与 Cargo 窗口仍按 owner 串行。
