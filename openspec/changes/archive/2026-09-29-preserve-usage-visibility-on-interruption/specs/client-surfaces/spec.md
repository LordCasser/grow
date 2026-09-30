## MODIFIED Requirements

### Requirement: Ordinary agent status shows session usage

没有 Goal 的普通主会话 Agent 视图 SHALL 在原 Goal 插槽显示本 session 跨 resume 的 lifetime 已记录累计 token 与可测量的输入缓存命中率，并在点击时打开 Usage 页。数据 SHALL 来自可恢复 session ledger 的变化投影，不使用定时轮询、context 窗口压力或 prompt 总额累加替代账本。子 Agent 内嵌视图不增加一个指向父会话用量的入口。

总消费完整性与缓存率可计算性 SHALL 独立表达：总消费不完整时累计保留 `≥`；缓存率按 cache read 与 full input 均已知的同一批样本计算，未知 attempt 不进入分子或分母。只要样本输入大于零且 `0 ≤ cached input ≤ measured input ≤ recorded input`，状态栏 SHALL 显示该比例。总消费不完整或 cache read 覆盖不完整时，比例前 SHALL 标注 `measured cache`，不将样本比例表述为全部实际消费的精确比例。只有无有效分母或计数无效时才显示缓存率 N/A；尚无用量快照时保持未知占位。

#### Scenario: Calls and late child settlement

- **WHEN** 普通会话产生主调用、已归属的子任务消费或不完整标记
- **THEN** 状态栏按账本 lifetime 已记录 input + output（含 cache hit）更新；重复累计快照不重复加账。完整总量与完整缓存明细显示普通缓存率；部分测量样本显示 `measured cache` 比例，详情页展示同一口径的比例与已记录输入覆盖率。

#### Scenario: Empty or incomplete usage

- **WHEN** 账本没有可测量输入、cached input 超过 measured input，或 measured input 超过 recorded input
- **THEN** 缓存率显示 N/A；总消费不完整仍独立显示 `≥`。仅缓存明细缺失不会使精确总量显示下界。

#### Scenario: Steering interrupts an attempt without final usage

- **WHEN** 会话已有可信输入与缓存数据，用户补充输入使一个已开始的请求以未知用量结算
- **THEN** 状态栏保留此前样本的缓存率并显示 `measured cache`，总量标为下界；此未知请求不作为零命中样本，不清空已有计数，也不使有效比例变成 N/A。

#### Scenario: Known samples continue after repeated interruptions

- **WHEN** 会话先记录 input=100、cache read=80，随后有一次或多次未知用量打断，再记录 input=900、cache read=810
- **THEN** 缓存率按 890/1,000 显示 `measured cache 89.00%`；总量仍标下界，不能沿用首次比例、平均请求百分比或因后续成功清除未知消费事实。

#### Scenario: Partially reported cache reads

- **WHEN** 所有总消费已知，其中 A 的 input=100、read=80，B 的 input=900、read 未报告
- **THEN** 总输入为精确的 1,000，状态栏显示 `measured cache 80.00%`；Usage 详情显示已记录输入覆盖率 10.00%。若 B 明确报告 read=0，则显示完整缓存率 8.00% 与覆盖率 100.00%。

#### Scenario: Explicit zero reads and missing writes

- **WHEN** 有正数输入，所有 cache read 明确为零，但 cache write 未报告
- **THEN** 缓存率为 0.00%，cache write 缺失不使缓存率 N/A；只有总消费或 cache read 覆盖不完整才要求 measured 限定。

#### Scenario: No measured sample yet

- **WHEN** 会话只有未知消费，或所有已记录输入都没有可信 cache read
- **THEN** 缓存率为 N/A；首个有效样本随后到达时自动显示带适用限定的比例，不要求新建会话或手动重置。

#### Scenario: Narrow status preserves the scope qualifier

- **WHEN** 窄屏裁剪包含 measured 缓存率的用量区域
- **THEN** 不能留下完整可读的百分比却裁掉其 measured 限定；允许隐藏或截短该项，点击区域保持在可见范围内。

#### Scenario: Open usage or Goal details

- **WHEN** 用户点击普通会话用量或已有 Goal 的状态区域
- **THEN** 普通用量打开既有 Usage 面板的 Usage 页，Goal 继续打开 Goal 详情；窄屏下点击区域不能超出可见区域。

#### Scenario: Resume or reconnect

- **WHEN** 客户端重新连接存活进程或在新 actor incarnation 从持久 Timeline 恢复被打断过的会话
- **THEN** normal 状态栏按此前与当前 incarnation 的 lifetime 计数显示同一测量比例及 incomplete 限定，不重置为零或只显示 resume 后新增用量；resident reconnect 不重复累计。

#### Scenario: Usage details across resumes

- **WHEN** session 至少经历一次有新增模型消费的冷 resume
- **THEN** Usage 页顶部显示 lifetime 总计，并按 Initial run、Resume #1… 展示各 incarnation 的新增消费；各段之和与 lifetime 已知总量及缓存覆盖计数一致。

证据入口：`crates/codegen/pager/src/views/agent_status.rs::session_usage_status_line`、`crates/codegen/pager/src/app/status_blocks.rs::session_usage_block_text`、`crates/codegen/pager/src/app/acp_handler/tests/mod.rs::transient_session_usage_replaces_totals_without_context_or_scrollback`、`crates/codegen/chat-state/src/actor/state.rs`。新展示场景待本 change 实施验证。
