## Evidence

现场 Timeline 位于 `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fsolaris-agent/01a1a0fc-f83a-70e0-aefa-339ca4f054b4/timeline.jsonl`。以下 seq 指外层 journal 物理 seq：

- 2515 已出现五项 required delivery 阻塞；2559、2581、2603、2625 的 todo_write result 报 `review requires the exact delivered terminal result`。
- 2920 提交 265278 字节的 followup payload；2932 的 Input::Consumed 中 permission_evidence 保留全文，但模型可见 item.content 仅 25090 字节并以 `[truncated]` 结尾。长输入截断另见 `preserve-long-steering-input`；本项验收错误早于长粘贴。
- 3061 继续因错误 resultId 拒绝；3115、3137、3159 的 released 操作因 null 或单独 seq 与真实 resultId 不符，落到 causal-fold 拒绝。
- 最后 plan revision 为 6，五项 link 均 terminal。实际 resultId 是序列化 TimelineRangeRef 字符串，不能以 child id 或 seq 代替。

现有契约要求精确验收、保留旧义务并停止重复阻塞；问题是执行这一契约的模型投影缺少与输入字段同名的绑定。修复投影和参数拒绝，不放宽验收。

## Design

复用 LocalPlan/PlanLink，提供一个共享的 JSON 绑定投影，字段为 linkId、resultId、reviewedSourceRevision。Todo 摘要中的未闭合必需交付及 request-only local planning context 共用此投影，模型仍需选择 decision、提供理由及验证依据。摘要保留原有八项上限；验收前八项后后续项自然进入摘要。错误回执始终针对请求指定的 link，不受摘要截断影响。

actor 在所有 review decision 的 resultId 不匹配时返回准确绑定，不进入 durable commit；非 released 仍要求 terminal/result，future revision 仍拒绝。旧 revision 不冒充最新要求已验收，原完成门禁不变。

## Verification

用生产 PlanningCommand::Write 路径核对：从模型可见摘要复制绑定后 accepted/released 可持久提交并消除对应阻塞；猜测结果身份被拒、原 plan 不变、错误携带可用绑定；冷 replay 保持验收结果。覆盖有结果与无结果的 release、旧 revision 和超过摘要上限的定位。保留现有重复阻塞停止回归。
