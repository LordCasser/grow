# 审计与修复边界

## 真实会话证据
只读检查用户指定 session 的 Timeline，未修改历史或重放工具。手动压缩前压力为 714,505，raw Surface 净减量为 901,979；旧实现直接相减并饱和为 0，丢掉 187,474 的差额。压缩实际已提交，1057 个条目替换为摘要，不能将界面的 0 解读为没有执行指令或清空历史。后续请求再用另一套 raw/request 差分调整，形成假增长。

后续自动压缩也提交了 replacement，Timeline 为 Completed，但收敛检查返回错误时，展示只有 Started。Pager 的 tracker 与 compaction live_feedback 独立，结束回合仅清 tracker，导致错误下方仍显示压缩中。完整证据在前序 [审计](../2026-10-29-audit-compaction-pressure-transitions/analysis.md)。

## 不变量及实现核对
- ChatState：当前有效输入估值 + 当前路由 provider 校准开销。Surface 只有在 durable/buffered 写入确认且 revision 改变后重估；不能将遗漏 reasoning 的 raw 字节差分扣到 provider 压力上。工具/schema envelope 保留估值，不增加副本账本。
- Goal、图片选取、native/portable reasoning：复用请求投影规则及已有选择身份。配置使图片选择失效、native reset 或 learned reasoning 变化，压力与通知同步更新。完整请求覆盖 envelope 并保持幂等。
- route/rewind：沿前序修复清理旧路由校准；普通 replacement/prune 保留同路由校准。billing ledger 不变。
- Shell：普通请求完整物化后、provider 派发前检查硬窗口，独立于重试抑制及子任务输出预算。model-switch 错误向上传播。
- 压缩事务：replacement 已提交则展示 Completed，同时保留 enclosing turn 的错误；未提交的失败/取消发对应终态。删去生成内层取消通知，避免与外层终态重复。新的 durable replacement 立即使旧 pending async notice 失效。
- pre-prune：写入前计划拒绝仍可进入摘要；持久化确认丢失或 owner 失败必须停止。prune 不增加额外 ChatState UI 事件，保留既有接口行为。
- Pager：finish_turn 清理 compaction 域反馈，不清除其它业务反馈。

## 对抗性复核
复核了大比例 omitted reasoning、重复 request build、partial replacement、裁剪、图片选择失效、native reset、路由切换、取消、持久化确认失败和 committed+error 的交叉边界。首次全库回归暴露的旧工具结果夹具只含孤立 result，而实际 portable replay 会省略它；将夹具修正为完整工具调用/结果对，保留增长、持久化、失败及无额外事件断言。

真正超窗时仍受现有一次有界摘要契约约束：摘要来源必须适配目标模型，且只替换实际覆盖的范围。不能压缩或压缩后仍超窗会保留历史并终止本轮。本次没有新增多轮分段压缩框架，也没有更改各协议普通请求的输出额度预留策略。Plan live handoff 已消费后的归属核对是独立缺陷，仍按 backlog 单独处理。

## 提交、展示与准入

| replacement | 本次结果 | 展示终态 | 普通请求 |
| --- | --- | --- | --- |
| 未提交 | 摘要失败 | Failed | 本轮停止 |
| 未提交 | 用户取消 | Cancelled（一次） | 停止 |
| 已提交 | 有效输入仍超窗 | Completed + 保留回合错误 | 禁止派发 |
| 已提交 | 有效输入适配窗口 | Completed；后台沿原延后通知规则 | 重新物化并通过准入后可发送 |
| pre-prune ACK 失败 | 持久化错误 | Failed | 不回退到摘要请求 |

同 slug 只降低 window 时，无需依赖模型切换专用入口：完整请求后的普通压力检查仍尝试恢复。SUPPRESS_TURN 在新回合清除是既有契约；持续的账号抑制与子任务自动压缩禁用则由最终硬窗口检查兜底。
