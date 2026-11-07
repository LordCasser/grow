# Pager 第一轮部分实现审查

审查版本：`01a1b03a-ba65-71b2-b053-b65440bb3ecf` 的12处局部编辑，未编译、未运行测试。Task3未完成，不接受其“核心交互闭环”说法；目前没有卡片、打开编辑入口或replay验证。

## 修复任务 A：展示事实早于 Presented

触发：`handle_ask_user_question` 收到effective blocking=false。当前分支在 `app/acp_handler/interactions.rs` 直接回Presented，只写日志，不保留/更新任何可见卡片。成功tool结果要等这个ACK才产生，不能用未来结果渲染证明ACK时已展示。破坏的契约是“有界展示准入后再继续”，导致模型以为用户已看见建议而实际UI无题目。

修正归Pager问答展示owner：同一次ACP处理先更新既有对应tool row的结构化问题与来源身份，再回Presented；成功结果只确认/恢复这张卡。失败状态明确未确认/失败；重复ext/result/replay不增加第二张卡。覆盖真实handler返回ACK前已有可渲染题目、Agent推荐/Reply入口，composer和blocking overlay保持；失败/重复控制不能只检查日志。该任务包含剩余卡片渲染、编辑打开和等待投影的一致性，不改输入接纳规则。

## 修复任务 B：答复身份服从不可变输入生命周期

触发：`dispatch_submit_advisory_reply` 发现同稳定id `in_flight_prompt.admission_resolved=Some(false)`。当前注释称nothing admitted并使用相同id重新入队，可能配新text。现有Shell稳定输入把Blocked也作为durable terminal；相同id重放只能观察原Block，不重新运行Hook或接纳新内容。`in_flight`/本地pending队列消失后也没有历史接纳检查，无法兑现cold后只读已答。

修正归Pager输入接纳owner：未知ACK沿既有reconcile/固定payload+id继续，不允许改文本复用；canonical已接纳或已完成时卡片只读，不再次入队；明确终态拒绝保留可复制文本并显示拒绝，不能假装同id是新意图。现阶段每卡片使用一个稳定提交身份；用户在终态拒绝后主动另发普通prompt是既有普通输入新意图，不通过卡片重用terminal id。cold/replay利用已有UserPromptBlock.message_id、shared queue、uncertain input事实，不另建持久答复registry。若尚无足够canonical事实，先报告具体缺口，不凭volatilebool或optimisticecho标已答。

通过标准：已答历史与replay不重复；transportunknown保持同payload/id；terminalBlock不再次提交同id，也不显示成功；后续修改用普通prompt。输入Hook与权限门槛保持，StoppedGoal不被card直接修改。

## 修复任务 C：卡片恢复消费实际结构化输出

触发：成功ask的ToolCallUpdate或cold replay到达 `apply_advisory_result`。父核Shell实际 `session/acp_conversion.rs:512` producer：content只有 `ask.message` 人可读文字，完整带type的ToolOutput位于raw_output。当前在途解析 `from_str::<AskUserQuestionOutput>(&content_text(tc))`用错数据通道，正常成功事件不会确认live卡，cold时也无法重建卡。

修正归同一卡片恢复owner：消费raw_output里的typed `ToolOutput::AskUserQuestion`，沿既有Read等typed分支约定，不解析展示文字或给Shell另造wire。验证夹具必须从实际producer对应形状生成：content=message，raw_output=serde_json::to_value(ToolOutput::AskUserQuestion(...))；旧解析不能cold恢复，新解析确认并恢复同一卡，重复事件保reply终态。该修正由TaskA在最终源码冻结前实施与运行反例，未跑前不接受恢复链。

## 集成前追加反例

同一答复的本地uncertain in-flight如果尚未送达服务端，canonical Absent必须能在用户明确重试下按原payload/id继续；不能以本地inflight存在永远拒绝重送，也不能变成自动无限重试。已有Accepted/Rejected投影不能被晚到旧Absent/Unknown覆盖；setter对这种回退返回false，callback在改队列/在途快照或消费打开意图前检查。这两例分别由输入owner与卡片投影owner按冻结接口覆盖。

两个任务分别交付和审查。TaskA的卡片可以先完成展示和编辑入口；直到TaskB及跨模块集成通过前，整项仍未验收。父接管原提交函数审查，不将其当可复用的正确准入实现。
