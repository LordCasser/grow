# 全压缩链路对抗审计（2026-10-29）

本轮在前序 pressure lifecycle 修复后的工作区审查全部主上下文压缩入口及相邻摘要 sideband。六个定向探针均违反预期不变量，确认五类缺陷；输出链继续确认 Responses EOF 被补成成功的相邻问题。测试采用真实纯函数、SessionActor 和本地 MockInferenceServer，不是对用户生产 session 的在线重放。修复在独立 change [fix-compaction-audit-counterexamples](../2026-10-29-fix-compaction-audit-counterexamples/verification.md) 中实施。

## 确认缺陷

| 优先级 | 缺陷与触发 | 实际反例 | 修复边界 |
| --- | --- | --- | --- |
| P1 | 未提升的后台摘要被普通 Stop/控制取消 | gate 尚无 background scope，现有 slot 的 token 没有被取消；释放已阻塞 provider 后真实 publication boundary 返回 `Ok(true)` 并提交摘要 | 取消原 generation token，保留 owner；不在普通 inbox 命令里 join |
| P1 | 手动压缩排队与自动压力恢复竞争 | `run_compact_only` 因 pending manual 跳过但返回 Ok；外层仍以相同压力继续；0.5 秒内 62 个 Step、0 个 provider 请求 | 当前 turn 在闭合边界让出前台，交给现有 idle arbitration；不是增设重试上限 |
| P1 | 非成功摘要响应有足量正文 | `max_tokens`、`model_context_window_exceeded`、`refusal`、`pause_turn` 四种终态全部返回成功并安装 partial summary | 在共享摘要生成器接纳正文之前验收终态，保留现有 Sideband settlement 和有界恢复 |
| P2 | 摘要来源预算使用原始 reasoning 体积 | 原范围 110002 tokens，实际 Messages 摘要输入 10003 tokens，12000 来源预算仍拒绝合法选区 | 原 Surface 决定完整边界/identity，实际摘要投影决定预算和缩减重试 |
| P2 | 非数字开头的摘要正文引用 analysis closer | `MUST_KEEP_EVIDENCE` 被清理删除，剩余 1701 字符仍通过摘要验收 | 仅移除明确前导 scratchpad；正文标签引用只中和，不删前文 |

### 证据与因果链

1. `actor/compaction.rs::cancel_background_compaction` 原来只调用 `CompactCancelGate::request_background_cancel`；generation 在 `background_compaction_boundary` 建立独立 token，直到 `finish_background_compaction` 才进入 gate。原取消测试没有把取消后释放的结果送入 publication boundary，不能证明迟到结果不可提交。新增两个探针分别验证 token 信号和真实迟到发布。
2. `idle_arbitration::admit_manual_compaction` 在普通 turn 活跃时排队，idle 才能取出；`check_auto_compact_needed` 不检查这项排队，而 `run_compact_only` 检查后返回 Ok。`turn/mod.rs` 把自动触发搬到下一个闭合 Step，形成循环。不是 provider 慢，也不是 UI 自己未刷新。
3. `generate_session_compact` 保留正文/stop/truncated，Shell adapter 原来只把正文交给共享 `generate_summary`；后者仅检查非空和最低长度。`truncated` 直到提交阶段才作为诊断使用。最终原 Timeline 仍在，但模型可见 Surface 可被不完整摘要替换。继续检查还发现 Responses 在没有 ResponseCompleted 时于 EOF 默认补造 stop；修复将成功标记绑定真实完成事件，并对未知/缺失终态明确失败。
4. `fit_compaction_range_to_budget` 原来在原 Surface 上累计预算，后续 `compaction_summary_source` 才剥离 Messages reasoning、portable history 和回忆派生。这与此前修复主请求 pressure 是两个不同计数边界。
5. `strip_leading_scratchpad` 原来以“不是数字开头”为条件，从最后一个 analysis closer 之前整体删除。自然语言摘要符合这一条件；最低长度检查无法发现局部关键事实丢失。

## 已有保护和本次结论的边界

- 大窗口切到小窗口（例如 600K/1M → 600K/500K）走模型切换恢复，普通请求还有独立硬窗口门禁。前序修复覆盖同 slug 降 window、child output budget、suppression 组合。本轮继续核对来源预算与输出验收；无法压到窗口内仍应明确失败并保留历史。
- pre-prune 的实际持久化失败 fail closed；写入前 Invalid/Empty 可回退摘要。replacement ACK 前不更新压力；提交后的修复失败仍以 Completed 记录已经发生的替换，同时 turn 返回错误。
- 原 Surface IDs、authority/model key、连续范围和 fresh skill 保护仍是提交条件。无关 tail 追加不应使冻结源失效；选区本身变化会拒绝发布。
- Provider 错误里的更小 context window 会在错误恢复路径应用。不能因常规 metadata 不接受降窗，就推断错误恢复忽略它。
- Pager Failed/Cancelled 与 turn finish 会清压缩反馈；异步 Completed 的提前返回是其无前台 Started 的既有语义。没有把这些合法分支列成新缺陷。
- 本轮没有证明所有真实模型 tokenizer 下计数精确，也没有运行线上付费请求。估算与 provider 校准的误差、跨多个合法范围的连续摘要策略仍有各自边界。

## 保留的验证缺口

- 未单独注入 Completed 终态 durable ACK 丢失与 post-commit control repair 的全部组合；当前代码 fail-closed 及既有单测不等于所有真实持久化故障都已复现。
- promoted waiter 被丢弃后再次 promotion 的事实幂等性需单独证明；当前已有测试主要覆盖随后的 invalidation/drain，本轮未证实生产缺陷。
- deferred async completion 期间仅切 route 的通知归属，需结合后续 request projection 场景补充契约测试；没有据此断言当前实现错误。
- Plan live handoff 已消费后的 owner 查询是独立已登记缺陷，不混入压缩改动。

完整入口矩阵见 [coverage.md](coverage.md)，原始失败探针与运行输出见 [probe-results.txt](probe-results.txt) 和 [reproduction.patch](reproduction.patch)。
