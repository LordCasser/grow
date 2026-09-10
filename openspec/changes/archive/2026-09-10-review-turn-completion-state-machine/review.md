# Turn 完成与跨端点状态机审查

审查基线：`afc9a481`；完成协议引入于 `dad7f053`。日期：2026-09-10。

当前方案消除了“无工具响应默认完成”这一条路径，但没有闭合响应终止、完成声明、等待依赖与调度之间的关系。不能据此认定提前停止问题已完整解决。以下结论区分动态探针、源码推演和模型能力风险；不是对所有端点的线上复现声明。

## 1. P1：工具存在性覆盖 provider 终止语义，FinishTurn 可绕过拒绝或截断

三个 adapter 都在存在完整工具调用时将中性的 stop_reason 覆写为 ToolCalls：

- `crates/codegen/sampler/src/stream/chat_completions.rs:391`
- `crates/codegen/sampler/src/stream/messages.rs:657`
- `crates/codegen/sampler/src/stream/responses.rs:668`

原始原因仍在 raw_stop_reason，事实并未完全丢失；但宿主在 `shell/src/session/actor/turn/mod.rs:1579` 只从中性 stop_reason 判断 refusal。`completion.rs:68` 校验 FinishTurn 的参数和可见文本，不检查 provider 终止事实。一个带非空文本和完整 FinishTurn 的拒绝/截断响应由此进入普通完成分支。缺少可见文本时则可能进入协议恢复，仍未按拒绝停止。

`probe.rs` 直接运行真实 adapter，六种输入全部确认同一结果：

| Backend | 原始终止 | 中性终止 | FinishTurn |
| --- | --- | --- | --- |
| Chat | content_filter / length | ToolCalls | 保留 |
| Messages | refusal / max_tokens | ToolCalls | 保留 |
| Responses | incomplete:content_filter / incomplete:max_output_tokens | ToolCalls | 保留 |

探针输入都带“先检查：”和格式合法的 completed 声明。这证明了到宿主入口的数据形态；进入 Completed 的结论来自上述源码组合，本轮没有编译新的 Shell 端到端反例。

这不是传输 EOF 被伪造为成功，而是把“调用完整”与“响应如何终止”放进了同一个互斥字段。旧测试 `refusal_after_tool_use_blocks_keeps_tool_calls_stop_reason` 和 `complete_tool_call_overrides_typed_length_but_preserves_raw_length` 还明确固定了覆盖行为，与新增“拒绝保留独立终止权威”的要求产生交叉冲突。

原始 session 已定位的三份停顿响应是正常 end_turn，不属于这里构造的拒绝/截断反例。这里确认的是整体审查发现的另一条错误路径，不能替代原始会话的取证结论。

修复边界：保留 provider 终止原因与调用完整性两个独立事实。完整业务调用如何处理可继续遵守其契约；控制性质的 FinishTurn 不能因此获得正常完成权。不要让 Shell 通过 raw_stop_reason 字符串反解析另一套 provider 状态机。

## 2. P1：等待声明未进入调度准入，Active Goal 可以继续空转

`CompletionIntent` 的两个 waiting 变体只经 `terminal_kind()` 写入 Timeline 字符串。`turn/admission.rs:1447` 把 completed、waiting_for_user 和 waiting_for_background 全部降为 `PromptCompletionKind::Completed`。`turn/settlement.rs:454` 的 Goal 收尾看不到等待原因；`turn/mod.rs:284` 唤醒 idle arbiter，`goal.rs:1530` 根据 Active/空闲/预算再次接纳 Goal turn。

具体条件：Active Goal 返回 waiting_for_user，且用户尚未提供信息；或者 waiting_for_background，结果仍未到达。只要模型未另行改变 Goal 状态且预算允许，下一 Goal turn 仍可立即开始。每一 turn 都有合法 FinishTurn，所以“三次缺少声明”的上限不约束这种循环。Stop hook 也不会补救所有情形，GoalContinuation 在 `turn/admission.rs:888` 跳过该 gate。

同时，FinishTurn 参数没有 dependency/task/interaction 身份。`explicit_completion_rejects_mixed_calls_and_accepts_waiting` 的测试没有创建后台构建，却能接受“等待外部构建结果”。在普通模式下，这只能结束前台，不能凭声明建立可靠唤醒；已有真实任务的通知机制仍然有效，不能泛称所有后台结果都会丢失。

这是设计缺口，不是要求模型工具直接 pause Goal。Goal 的 Active 生命周期和“此刻是否有条件继续采样”需要分开。等待应引用现有可验证依赖，参与现有准入与通知消费；依赖就绪、用户输入或显式控制解除等待。没有可关联依赖的声明应明确处理，不能伪装成已经建立的等待状态。

证据等级：当前源码完整调用链；已有等待标签回归通过。未运行 Active Goal 的上述新组合测试。

## 3. P2：有效声明遇到插话后，连续违约计数仍可能包含声明之前的失败

`turn/mod.rs:896` 初始化 completion_violations；`completion.rs:129` 递增；`turn/mod.rs:1886` 只在剩余业务调用进入执行路径时清零。接受合法 FinishTurn 后，如果 `turn/mod.rs:1797` 或 `1825` 消费了插话/延迟结果，就直接继续循环，计数未清零。

反例顺序：两次无声明 → 第三次合法 FinishTurn，同时接纳插话 → 下一响应无声明。实际会达到 3 次并报“three consecutive responses”，但中间已有一份合法声明。若同一插话稍晚到达，走 `turn/mod.rs:428` 的外层结算竞争处理，则重新进入 process_conversation_turn，计数反而清零。恢复额度因此依赖输入到达了哪个 await 窗口。

修复应把“收到有效声明后结束连续违约段”和“该声明能否收尾当前输入”分别处理。消费新输入使旧声明不能授权完成，不应把旧声明变成一次未发生的协议违约。端点切换本身仍不应无条件刷新恢复预算。

证据等级：源码分支推演。既有插话测试从计数为零开始，未覆盖上述序列。

## 4. P2：切到 Messages 时，中性工具 ID 会发生多对一编码

`sampling-types/src/conversation.rs:4006` 将部分字符替换成 `_`，在 `4139` 和 `4159` 分别用于调用及结果。有效中性历史中的 `a.b` 与 `a/b` 都成为 `a_b`。探针通过真实 build_messages_request 确认调用和结果均出现重复 ID。

因此，历史配对在 Timeline 和 portable projection 内正确，不代表跨 backend 编码后仍然唯一。服务端可能拒绝请求或得到歧义关联。FinishTurn 也使用相同历史通道。原始两份 session 尚无该碰撞证据，不能把它说成原始冒号停顿的已证实根因。

这是 backlog 已有独立债务，本轮补足动态证据。后续应在目标编码边界建立单射身份映射，并统一 live/portable/native 的边界行为，不改写历史事实或重放工具。

## 5. 完成声明的兼容性与语义风险

这些是已证实的宿主限制，不能冒充已复现的真实端点故障：

- 所有 `json_schema.is_none()` 的普通请求都添加 FinishTurn（`turn/mod.rs:1208`）。该门槛不检查当前模型能否可靠执行工具，request_builder 的 tool_choice 仍为 None。端点支持某种 API 格式，不等于模型遵守该完成协议。
- 已经给出完整答案但没有调用 FinishTurn，也会被要求继续，两次纠正后第三次报错。只输出工具调用，或把正文与声明分在不同响应中的端点，也会违反“同一响应必须带正文”的条件。这包含纯问答 Agent，并不限于代码执行场景。
- 校验“有文本、有理由、有合法 status”不验证业务是否完成。“先检查：”加 completed/done 同样可被接受；waiting 也不验证所述依赖是否真实存在。FinishTurn 是主张，不是业务正确性的证明。
- Responses 的 phase 和消息边界没有完整进入中性 Assistant；切换、恢复或压缩撤下 native 后，阶段语义无法完整还原。现协议没有使用 phase，所以不能将新增 FinishTurn 当作修复了这一投影债务。

应先定义当前 route/Agent 实际可满足的完成协议，再进行准入和有界纠正。若协议不受支持，应明确暴露能力限制；不建议用冒号规则、任意旧正文或再次默认“无工具即完成”来掩盖它。任意任务是否真的完成仍需模型判断；宿主可以且应当核对已有的执行、等待、输入和权限事实，但不能声称通用语义正确性已被形式化证明。

## 6. 状态组合审查矩阵

| 状态转换 / 交错 | 当前结论 | 证据与限制 |
| --- | --- | --- |
| 无声明 → 工具 → 显式完成，三 backend | 既有基本路径有效 | 复跑 explicit_completion 组；脚本化端点，不是在线模型 |
| 采样期间 A → B，连续多次选择 | 有 Step 边界和 desired revision 隔离 | 复跑 model_switch 组；没有新增 FinishTurn × 全切换方向笛卡尔测试 |
| 切回 A / 同名模型换 endpoint 的 catalog reload | 旧 native 不会因名字相同复活 | ReplaceSamplingRoute 清 epoch；same_model_catalog_reload 等测试 |
| 只改变 reasoning effort / 重复选当前模型 | 保留同路由 native 是既有契约 | effort_only_update / identical_sampling_application 回归 |
| 路由切换与迟到凭据 | 旧凭据刷新不跨 revision 提交 | stale_credential_refresh / frozen_route_auth_axes 回归 |
| 切到小窗口 / 异步压缩与模型控制 | 在边界压缩；迟到结果核对 authority/model | 代码与选定 compaction 回归；没有新全生命周期故障注入 |
| unsigned thinking / native 被拒绝 / 恢复 | portable 保护可保留工具往返，不自动重放 | ChatState 回归；目标 ID 编码仍有第 4 项缺口 |
| 合法 FinishTurn 与新输入交错 | 旧声明不会直接授权新输入完成 | 既有 steering 回归；连续违约计数有第 3 项问题 |
| 切换 Agent / Behavior | Agent 工具契约换代；Behavior 变更控制结束旧 turn | 源码与 model_switch 回归；不能概括为所有新输入都会自动续跑 |
| 取消、owner 失效、预算 | 下一 Step 有准入保护，持久化确认独立 | 既有取消 fixture 为直接前台 stub；不声称真实 ACP 各取消窗口全覆盖 |
| FinishTurn 与拒绝/截断同响应 | **存在缺口** | 第 1 项六个 adapter 反例 |
| 等待用户/后台 + Active Goal | **存在设计缺口** | 第 2 项调度链；暂无新 Goal 组合动态回归 |
| Stop hook 拒绝结束 | 新 round 重新获得声明，最多 8 次 hook continuation | 源码；默认 hook 不证明完成，GoalContinuation 跳过 gate |
| 结构化输出 / Sideband | 有独立契约，普通 FinishTurn 不注入 | 既有 schema fixture；切换到 native schema 后已安装工具协议保持 sticky |
| 崩溃在调用、结果、Turn terminal 之间 | 没有完整的新交叉验证 | durable response/result 与恢复机制分别有测试，不等于三个崩溃窗口组合已验证 |

## 7. 规范偏差与修复顺序

`model-sampling/spec.md` 的 Ordinary turns require explicit completion intent 要求普通回复必须声明；`context-compaction/spec.md` 的 Ordinary final response after handoff 仍明确规定合法无工具最终回复按正常完成结束。当前实现采用前者，既有压缩 fixture 加入 FinishTurn 后通过，但没有同步修订后者。OpenSpec 格式校验不能识别这类跨规范矛盾。修复时应在同一行为 change 明确更新对应 delta 和开发者说明，不能静默删掉旧场景。

建议按独立闭环处理：

1. 先分离 provider 终止事实和工具存在性，补“FinishTurn × 拒绝/截断”的三协议回归。
2. 明确完成与等待的宿主状态，复用现有 input/task/notification 身份及 Goal 准入；补等待、唤醒、取消、重启和无依赖反例。
3. 修正有效声明与恢复计数的状态归属，补早/晚插话以及端点切换后的同一序列，保持恢复总额度。
4. 单独修目标 ID 映射；phase/消息边界保留单独定义，不混进第 1 项。
5. 定义可跨端点满足的完成编码与能力限制，同步相关规范；真实端点冒烟应验证模型实际输出，不能由 fixture 自动补声明来代替。

不需要再增加一个 LLM 裁判或文本启发式框架。核心是把响应结束、声明、可执行调用和调度就绪状态分清，并让一个宿主决策路径使用这些事实。
