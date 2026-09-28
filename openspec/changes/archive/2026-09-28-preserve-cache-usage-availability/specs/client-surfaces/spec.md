## MODIFIED Requirements

### Requirement: Usage exposes provider-model totals and cache-hit rates
`/usage` SHALL display total token consumption and provider/model breakdowns using full input plus output including cache-hit input. It SHALL show known cached input, cache-field availability, cache-hit percentages for measured input and input-token coverage overall and per provider/model, within the labeled ledger reporting window. Rates SHALL include only attempts with valid known cache read and full input; missing cache fields SHALL NOT become zero-hit samples. The identity SHALL be captured from the selected catalog route before sampling, not from provider-returned model aliases.

#### Scenario: Switch provider or model
- **WHEN** calls use different provider/model identities, including providers serving the same wire model name
- **THEN** prior charges remain in their original groups and overall usage is their cumulative sum.

#### Scenario: Weighted total rate
- **WHEN** models have different input volumes and cache hits
- **THEN** overall measured rate is known cache-hit input divided by full input of the same read-known attempts, rather than the arithmetic mean of model percentages; coverage is that measured input divided by all recorded known full input. Each model shows its own totals, available cached input, measured rate and coverage.

#### Scenario: Single model and empty input
- **WHEN** only one model has usage or an entry has zero input
- **THEN** the model identity is still shown and a zero denominator displays N/A rather than a fabricated percentage.

#### Scenario: Incomplete or invalid usage
- **WHEN** the ledger is incomplete or cached input exceeds reported total input
- **THEN** incomplete rates and coverage are identified as based on recorded usage and invalid ratios display N/A; no misleading exact overall percentage is asserted. Missing cache details alone do not mark independently known total token consumption as incomplete.

#### Scenario: Existing display paths
- **WHEN** usage is opened in fullscreen, inline or minimal mode
- **THEN** the same statistics projection is shown, and `/session-info` remains unchanged.

#### Scenario: Mixed reported and unreported cache reads
- **WHEN** A 报告 full input=100、read=80，B 报告 full input=900 且 read 缺失
- **THEN** 总输入为 1,000，详情显示已知样本命中率 80% 与已记录输入覆盖率 10%，缓存累计标为已知部分；B 若明确 read=0，则命中率为 8%、覆盖率为 100%。

#### Scenario: Cache writes and unknown reads
- **WHEN** read 已知而 write 缺失，或所有 read 都未知
- **THEN** 前者仍按已知 read 计算命中率并标记 write 未知，后者命中率显示 N/A；write 从不进入命中分子。

### Requirement: Ordinary agent status shows session usage
没有 Goal 的普通主会话 Agent 视图 SHALL 在原 Goal 插槽显示本 session 跨 resume 的 lifetime 累计 token 与输入缓存命中率，并在点击时打开 Usage 页。数据 SHALL 来自可恢复 session ledger 的变化投影，不使用定时轮询、context 窗口压力或 prompt 总额累加替代账本。子 Agent 内嵌视图不增加一个指向父会话用量的入口。

#### Scenario: Calls and late child settlement
- **WHEN** 普通会话产生主调用、已归属的子任务消费或不完整标记
- **THEN** 状态栏按账本的 lifetime 累计 input + output（含 cache hit）更新，重复累计快照不会重复加账，缓存明细与总量均完整时，缓存率按总 cached input / 总 input 计算；read 覆盖不完整时紧凑状态栏显示 cache N/A，详情页展示已知样本比例与覆盖率。

#### Scenario: Empty or incomplete usage
- **WHEN** 没有输入、缓存数超过输入、read 覆盖不完整或总消费账本不完整
- **THEN** 状态栏比例显示 N/A；只有总消费不完整才将累计显示为 ≥。缓存明细单独缺失时保留精确总量，详情明确已记录用量及覆盖范围。

#### Scenario: Open usage or Goal details
- **WHEN** 用户点击普通会话用量或已有 Goal 的状态区域
- **THEN** 普通用量打开既有 Usage 面板的 Usage 页，Goal 继续打开 Goal 详情；窄屏下点击区域不能超出可见区域。

#### Scenario: Resume or reconnect
- **WHEN** 客户端重新连接存活进程或在新 actor incarnation 从持久 Timeline 恢复会话
- **THEN** normal 状态栏显示此前与当前 incarnation 的 lifetime 总计，不重置为零或只显示 resume 后新增用量；resident reconnect 不重复累计。

#### Scenario: Usage details across resumes
- **WHEN** session 至少经历一次有新增模型消费的冷 resume
- **THEN** Usage 页顶部显示 lifetime 总计，并按 Initial run、Resume #1… 展示各 incarnation 的新增消费；各段之和与 lifetime 已知总量及缓存覆盖计数一致。

证据入口：`crates/codegen/chat-state/src/actor/state.rs`、`shell/src/extensions/usage.rs`、`pager/src/views/usage_modal.rs`、`pager/src/app/status_blocks.rs`。

### Requirement: Session usage retains agent attribution behind aggregate surfaces

会话用量账本 SHALL 将本会话主 Agent 与每个已结算子 Agent 的已知 token 消费分别归属，同时 SHALL 使总体等于这些消费的累计结果。当前 `/usage`、headless usage 与没有 Goal 的 normal 状态栏 SHALL 只展示既有总体及 provider/model 投影，不公开 Agent 分项。

#### Scenario: Main and child agents consume tokens
- **WHEN** 主 Agent 产生模型消费，两个具有不同 `subagent_id` 的子 Agent 完成并回传各自累计消费
- **THEN** 会话总体包含三者消费各一次，Agent 分项能分别识别主 Agent 与两个子 Agent，并保留每项已知 token 数。

#### Scenario: One child uses multiple models
- **WHEN** 同一子 Agent 的终态累计快照包含多个 provider/model 分项
- **THEN** provider/model 分项继续按模型累计，该子 Agent 分项累计这些模型消费，且会话总体不遗漏或重复任何一项。

#### Scenario: Existing aggregate displays
- **WHEN** 包含子 Agent 消费的会话账本投影到 `/usage`、headless usage 或 normal 状态栏
- **THEN** 当前界面和公共 usage 形状显示包含子 Agent 的总体及缓存字段可用性/覆盖信息，但不增加 Agent 分项字段或逐 Agent UI。

#### Scenario: Incomplete child settlement
- **WHEN** 子 Agent 回传已知下界并标记用量不完整，或其用量无法可靠应用
- **THEN** 已知消费仍按原 `subagent_id` 归属，既有 incomplete 规则继续阻止总体被表示成精确完整值。

证据入口：`crates/codegen/chat-state/src/usage.rs`、`crates/codegen/shell/src/session/actor/updates.rs`、`crates/codegen/shell/src/extensions/notification.rs`、`crates/codegen/pager/src/app/status_blocks.rs`。

### Requirement: Usage surfaces include auxiliary model consumption

`/usage` SHALL display the owning session's known main, child and Sideband model consumption in lifetime and resume segments, grouped by the frozen provider/model route. Full input plus output includes cache-hit input. Incomplete Sideband attempts SHALL preserve the lower-bound marker and prevent unknown cost from appearing exact. Sideband calls SHALL NOT increment the public main-loop turn count. Aggregate UI and headless usage SHALL retain the existing ownership and total-consumption semantics while carrying cache-field availability and coverage. Missing cache details with independently known totals SHALL NOT mark total consumption incomplete.

#### Scenario: Auxiliary calls across a resume

- **WHEN** a session's main loop, recap and memory Sidebands consume known tokens across two incarnations
- **THEN** `/usage` and the ordinary status projection include all three charges once, each in its proper segment and route group, while `numTurns` counts only main-loop rounds

#### Scenario: Auxiliary call with unknown usage

- **WHEN** a Sideband provider request completes or fails without trustworthy usage
- **THEN** `/usage` and headless reporting show recorded totals as an incomplete lower bound and do not present unknown cost as exact

#### Scenario: Structured usage retains cache availability
- **WHEN** 主调用、子 Agent 或 Sideband 的 usage 经 ACP/headless 投影，且缓存明细部分缺失
- **THEN** 结构化输出保留字段未知与显式零、已知缓存累计和覆盖分母；重连或恢复后不把缺失改为零，不公开额外的逐 Agent 明细。
