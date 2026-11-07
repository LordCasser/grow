# Ask 两模式集成验收计划

本计划验收首次完整链路。Tools的72项ask/56项输出与机械调用方check只作为模块证据；Shell、Pager、Settings局部通过也不自动满足本里程碑。父负责确认最终源码及产物身份，执行由一个独占Cargo窗口的实现/验证者完成。禁止访问或修改真实用户session、Goal、配置。

## 可复用入口与隔离

优先沿现有 `pager/tests/pty_e2e/common.rs` 的 `ContentController`、`expect_tool_turn`/明确call-id的ChatCompletions SSE、`PtyHarness` 与同构建默认CLI产物。`auto_wake_cancel_preserves_queued_user_prompt.rs`已有脚本化工具调用和request_bodies控制；`ctrl_c_cancel_during_stream_recovers_cleanly.rs`已用真实PTY验证后续普通prompt可继续。不是给工具伪造Accepted结果来绕过Pager。

每个场景临时HOME/GROW_HOME/project，显式设定mockprovider配置，已有测试sandbox承担外部发现隔离，源配置不从用户家目录继承。记录产物路径及mtime或哈希；实际build后的产物才可作最终证明。长任务background运行，单一Cargo owner，不用fixedsleep假定UI已经接纳，使用provider请求、屏幕标记和canonical事件作barrier。结束沿现有harness回收本次进程，不按全局名字杀进程。

## 场景与判别

1. **默认允许建议，未答即可继续**：脚本模型第一次返回 `ask_user_question(blocking=false, questions=[合法recommended_option])`。不输入人答，等待真实Pager展示并确认，观察mockprovider收到下一模型请求，结果明确Agent推荐、用户未答，且后续正文完成。composer可接受独立输入，状态不标NeedsInput。canonical同call只有一个终态结果。
2. **迟答成为新输入**：在上述工具和原Turn结束后，经可见卡片入口修改选项并提交；新的provider请求中出现题目、原推荐和真实人答归属。原call仍只有一个终态结果，答复按 `question-reply:{source_session_id}:{tool_call_id}` 稳定身份durable接纳；再次点击、同身份重试或重连replay不重复接纳、不重新回答旧tool。把模型/标签中含slash样式的文本作为数据，不能触发slash、退出或其他UI命令。
3. **必要决定仍等人答**：缺省blocking或明确true的工具，在真实QuestionView出现后无人答，provider没有收到后续请求；真实选答后才继续。以服务端终态barrier和限定观察窗证明仍在等待，不把“尚未发到工具”当作通过。保持Plan/Other基本路径。
4. **用户关闭偏好**：通过实际Settings action与Effect持久保存false；随后同一个活跃会话中的模型仍请求blocking=false，应显示原阻塞界面并保持等待，选答后继续。重新启动/恢复隔离会话后再请求，仍为blocking。文件中timeout字段及无关raw字段保持。恢复true后下一次准入为advisory；已准入blocking问题不因切回true自动变形。
5. **失败与未知ACK**：沿Pager实际输入接纳失败/reconcile测试入口验证卡片答复仍可恢复、重试payload+id不变，成功事实到达前不标已答；与真实Shell canonical输入接纳查询配对验证。若现有PTY无法稳定故障注入，该项可用实际ACP connection/公开输入API的定向模块集成补充，不能用手改volatile bool代替。
6. **child与StoppedGoal边界**：原建议来自已终结child时，经现有parent关系提交到其primary；other tab不可接收。原Goal已Stopped时接收普通人答不能触发GoalContinuation/active状态写入。允许由真实普通用户输入产生其正常Turn，不把它误报为Goal自动恢复。原ordinary tool的迟答调查仍按独立change记录，未复现不得称已修。

## 完成门禁

- 对最终源码运行变更相关模块/集成，记录实际命令、退出、计数与日志。失败先区分产品缺陷、夹具前提和旧产物；不得删失败场景或以放宽断言代替修复。
- 因本特性涉及人答归属、原工具单一结算、重复接纳和持久偏好，按设计安排一次独立review；review真实diff及cross-boundary证据，重点是模型推荐不会成为用户授权、迟答不会投递错误session。
- 父验收后同步开发说明、strict全量和archive校验，归档本change；仅完成模块时保持任务未勾。模型池方案仍单独待用户同意，不能随该里程碑实施。
