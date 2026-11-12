## Decisions

以现有单目标 wait_one 和 ChannelBackend 为执行/授权所有者。聚合从 join_all 改为 FuturesUnordered：继续越过非终态分段和单项观察失败，首次确认任一终态后立即结束。每个等待 future 借用一个本次调用的结果槽，持续保存最近观察；释放其余等待 future 只取消观察，不取消 child。按输入规范化后的顺序返回全部身份。

结束原因从 all_finished 改为 any_finished；已有 completed/failed/cancelled/output_unavailable 均是执行终态。没有终态时仍按实际总期限或所有观察失败返回 deadline_elapsed/observation_failed。提前返回时无快照的目标复用 observation_interrupted，并说明观测因另一目标终态而停止；已有非终态观察原样保留。不新增目标状态推断或第二份持久状态。

等待结果只确认实际交付的终态 ID，其他 child 的通知继续由原 inbox/等待包装器负责。主 agent 指引要求按 ID 分析返回结果、处理失败和结果不可用，再决定剩余依赖；运行终态不代表成果验收。

## Verification

通过公共 Tool 入口验证慢/停滞 sibling 不拖延已完成结果、已有终态立即返回、失败/取消同样唤醒、观察错误不会冒充终态、非终态最后快照与全部身份保留、单一 deadline、只消费实际交付终态、等待结束不取消 child。复用真实 coordinator 的分段/终态测试，核对生产 prompt 渲染与重命名。对长历史、历史回退、结果回执和耗时的独立发现仅登记证据，不顺带改动。

## Receipt race audit

多个 child 同时终态而只返回其中一个时，未返回的结果不得被“oneshot 已发送”视为已被模型消费。Shell 的 admit_completion_receipt_before_result 在 coordinator 结果发送前确认 durable inbox；工具输出只确认实际返回的 terminal ID。普通等待 finish_wait 仅移除临时 handoff reservation，不确认未返回 child 的 durable receipt。任何拟议的收尾优化都必须保持这一边界。
