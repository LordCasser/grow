## Context

问题证据及规范偏差见 [analysis.md](analysis.md)。修复包含两个独立边界：采样不能丢掉已确认账单；展示不能因另一笔账单未知而隐藏已有测量值。后者是本会话截图的已确认原因，前者是同一取消路径中源码可定位、待回归复现的窗口。

现有所有权与数据流保持：

```text
provider stream → protocol parser → attempt usage sink
  → ChatState durable settlement → UsageLedger
  → PromptUsage / grow/sessionUsage → Pager status + /usage
```

Shell 同时将同一 attempt 交给适用的 Goal 和子任务输出预算；账本只在持久 ACK 后更新，Timeline 恢复按身份去重。`UsageLedger.incomplete` 是持久的消费未知事实，不能为了 UI 清零。

## Goals / Non-Goals

Goals：正常中途补充输入后缓存统计仍有用；取消与结束帧竞争不丢失已确认 usage；保持取消响应性、计费诚实、预算约束及恢复幂等。

Non-Goals：重建未知 token、从字符数估算账单、后台等待或追查 provider 账单、增加公共事件或持久化格式、重构整套采样恢复、改变 provider 缓存参数。Messages 的中间 delta 不成为精确用量，已取消候选也不因账单已知而恢复执行。

## Decisions

### 1. 已测量比例与整个账本完整性分开

复用现有字段，令 `I` 为已记录 input、`M` 为 cache read 已知样本的完整 input、`C` 为这些样本的 cache read：

```text
measured_rate = C / M                 (0 < M ≤ I, 0 ≤ C ≤ M)
recorded_input_coverage = M / I       (I > 0)
```

只使用分子分母累积，不平均请求百分比。`usage_is_incomplete` 或 read 未知调用、或 `M < I` 表示需要限定；它们不是拒绝计算有效样本的理由。write availability 不参与比例判定。无快照保留 `—`；无样本、计数越界为 N/A；显式 read=0 仍是有效样本。

| 状态 | 状态栏缓存部分 | token 部分 |
| --- | --- | --- |
| 总量及 read 覆盖完整 | `97.06% cache` | 精确已记录总量 |
| 总量未知，有有效样本 | `measured cache 97.06%` | `≥` 已记录总量 |
| 总量精确，read 部分缺失 | `measured cache 80.00%` | 精确总量 |
| 无有效样本或计数无效 | `N/A cache` | 独立按总量完整性决定 `≥` |

不使用 `~97%`，因为这是已测量样本的比例，不是对未知消费的推算。也不只在弹窗解释后让状态栏显示无限定百分比。`measured` 放在比例前，兼容现有从右侧截断单项的窄屏布局，避免限定词被截掉后留下无条件百分比。详情复用现有 `/usage` 比例、覆盖率和 incomplete 提示；说明 100% coverage 仅指已记录输入，不能覆盖未知请求。

### 2. 完整 usage 的寿命属于 attempt

在 Chat 流层现有确认 usage 的位置，同步保留一份仅属于当前 attempt 的可选 usage/cost 快照；该槽位由 attempt owner 持有，生命周期位于可被取消而丢弃的 stream future 之外。它是传递已知消费的短期状态，不是新增账本，也不逐 chunk 持久化或发公开通知。快照保留 `TokenUsage` 的 cache 字段可用性，累计 provider 数值采用当前解析规则替换，不相加。

若同一 Chat chunk 同时携带终止理由、完整 usage 和最后一段预览，解析器在第一次预览 yield 前先确认并存入快照，避免取消恰好发生在该 yield 处时丢账单。

`run_one_attempt` 在取消返回时通过现有 attempt 结果路径携带这份已确认数据；外层取消、逻辑期限转换与正常终态共享一次 `AttemptUsageSink` 结算。已有完整终态 usage 是首选，取消回退仅取该 attempt 当前有效的已确认快照。槽位每个 attempt 重新建立；任何已确认的失效/冲突处理不能被回退快照绕过。known 与 unknown 只选择一次，禁止先提交 unknown 再用相同身份覆盖为 known。

不为取得 usage 多 poll 尾部，不延长现有 2 秒可选尾部窗口，不新增 drain timer。取消仍停止 provider 工作，只等待原有 evidence 与所有适用账本的 ACK。已确认账单不授权发布 Completed、接纳正文或执行工具。

Messages/Responses 的完整性判断与正常终态路径保持。Messages 等待 message_stop 前的计数属于中间状态；不能为了提高统计覆盖率将其升为 Known。只在实际存在取消丢失窗口的 Chat 路径增加快照，不预建通用 observer 框架。

### 3. 真正未知消费仍保留

usage 尚未到达时保持现有 `AttemptUsage::Incomplete`、Timeline settlement 和预算门槛。用量恢复与 child fold 不新增格式；后续成功、模型切换、重复快照或 resume 均不能洗掉原 lifetime incomplete。费用仍沿现有 `PromptUsage` 规则隐藏不可信总额。

旧会话不回填历史 unknown。升级展示后，已有账本中的分子分母立即可显示 measured 比例；采样修复仅避免今后出现可防止的数据丢失。

### 4. 有边界的验证

先用 fake SSE / 受控流复现 Chat `finish → usage → pending tail → cancel`，通过确认解析进度的 barrier 触发取消；不能靠随机 sleep 猜时序。对照在 usage 之前取消、逻辑期限、终态同时 ready、结算 ACK pending、下一 attempt 隔离。known 回归必须检查真实 sink 收到原数值和一次结算，而不只检查一个内部字段。

展示测试覆盖已知 A → unknown steer → 已知 B、重复 unknown、首个有效样本到达、子账单迟到、read 缺失/零值、write 缺失、非法比例以及窄屏。贯通一个真实结算/恢复 fixture，经 `PromptUsage` 投影后验证状态栏与详情的数值口径一致；沿用现有重连去重与精确预算回归，避免新增并行统计实现。

## Risks / Trade-offs

- 采样数据的“已收到”与“已确认完整”不同 → 仅在现有协议确认点捕获，不从 raw 字节或中间计数推断 Known。
- 使用旧快照掩盖协议冲突或跨 attempt 串账 → 槽位与单 attempt 同寿命，沿当前确认/失效规则维护；加隔离回归。
- measured 可能被误读成完整会话比例 → 限定词位于比例之前，详情保留已记录输入覆盖率及总量未知说明。
- unknown 仍可能长期存在 → 这是计费证据边界，保留 `≥` 与预算限制；产品仍可持续提供已测量统计。
- 工作区已有其他改动 → 本 change 实施只触及列出的采样与用量边界，文档、测试按归属审查，不混入其他 UI/存储改动。

## Migration Plan

不迁移 Timeline、ACP 或配置，不重写旧会话。实现通过定向回归和 OpenSpec 校验后归档 delta，再更新说明的契约链接。本次仅立项：生产代码、运行验证和归档均未执行。
