# Resume 与退出的现场边界分析

## 目标与必要修正

历史应在运行过程中持续持久提交。退出的关键路径只应处理在途工作结算、当前续跑状态和尚未确认的增量。resume 应恢复执行所需的完整状态，历史展示独立加载。

“现场”不能等同于当前模型请求的 messages。Goal/Plan、未结束子树、待交付结果、未消费输入、人工交互、工作流、权限与配置引用、当前 Surface/分支、序号和用量结算都可能影响下一步，即使没有出现在模型上下文中。这些仍是现场。已闭合历史保留磁盘事实与可查询身份，不应每次重新装入运行态。

下面区分源码确认的行为与建议。没有对用户原会话运行新构建或采集分阶段 profile，不能把静态路径当作 197 秒加载的耗时分解。

## 当前实现证据

### 1. 写入已经是增量的，挂起准备仍复制历史

- `crates/codegen/chat-state/src/actor/mod.rs` 的 durable commit 在持久确认后 accept。同文件 `GetTimelineEvents`（约 1097 行）对所选事件范围执行 `to_vec()`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` 的 `LedgerPrefix`（约 94 行）保存当前 writer epoch 的已确认前缀和续算 hash 状态，正常追加无需重读旧 ledger。外部修改导致 stamp 变化时仍有完整核对路径。这是内存 writer 优化，不是可直接跨进程恢复的现场快照。
- `crates/codegen/shell/src/session/actor/suspension.rs` 的 `suspend_node`（约 1052 行）调用完整 `timeline_events()`，随后 `runtime_delegation_events()` 对 root 又调用一次。前者用于恢复交互、执行游标和操作证据，后者用于委派关联；并非都需要历史正文。child 的 delegation 查询还会打开并完整验证 root 历史。
- 同文件约 1353 行最终调用 `checkpoint_runtime`，追加一个 checkpoint。`RuntimeCheckpoint` 在 `crates/codegen/chat-state/src/session_runtime.rs` 约 92 行，仅有 frontier、runtime source hash、Turn continuation 和 Step，不包含可独立水合的完整状态。
- 当前 `runtime_tree_snapshot` 已使用 `Timeline::subagent_lifecycle_events()` 的位置索引提取窄投影，不能把旧版树快照全历史重放再次报告为当前缺陷。该索引仍包含历史 Spawned/Ended，不等同于只包含未结束义务。

因此退出变慢不必意味着历史被重新写盘；复制、重建和查找历史也会使退出随历史增长。

安全停机仍需停止新执行准入，结算在途模型/命令/工具，保存 Interrupted 或 OutcomeUnknown，等待必要 owner 与 writer 退休。相关路径在 `suspension.rs`、`runtime_handoff.rs::suspend_root_tree_locked`、`session/actor/teardown.rs`。这些等待随活跃工作和外部资源变化，不能以跳过 ACK、强制结束或固定退出时限来替代。11-11 的 `wait-for-exit-suspension` 已取消退出整树共享 elapsed-time cutoff；不能照搬 11-07 文档的五秒假设。

### 2. resume 重新读取、折叠和验证全部事实

- `crates/codegen/shell/src/session/persistence.rs::load_light`（约 3026 行）只是延后 replay updates 与 rewind 文件，不意味着只加载现场。
- `storage/jsonl/mod.rs::load_validated_writer_timeline`（约 728 行）读取全前缀，再 `Timeline::from_events`。观察路径 `OpenedSession::validated_timeline`（约 313 行）也完整读取并折叠。
- `storage/jsonl/ledger.rs::read_journal_prefix_data` 从 `0..complete_len` 扫描，同时生成事件和 HistorySnapshot。
- `chat-state/src/timeline.rs::Timeline`（约 1890 行）同时持有完整 `events`、当前 Surface、lifecycle 与去重结构；`from_events`（约 2205 行）逐条恢复。把整个 Timeline 序列化为 checkpoint 仍会随历史增长，不满足目标。
- `validate_timeline_evidence` 调用 `persistence::verify_timeline_prompt_blobs_from_directory`，后者先执行 `sampling_evidence::verify`，遍历历史 observation 并读取、校验它引用的原始采样 body chunks。这些原始采样证据注明不用于恢复 native continuation，却仍进入当前完整验证边界。
- `actor/state.rs::usage_from_timeline` 及控制、signals、announcement 等投影也从历史派生。恢复快照要覆盖这些当前投影，否则仅跳过第一遍解码不能完成现场水合。

观察用户会话 `01a1c9fe-b28a-7e71-a452-056a5e03fa67` 时，Timeline 约 131 MB、5.6 万行，sampling 目录约 286 MB、5,595 个文件，不含子会话。会话仍在增长，数字是 2026-11-13 本次只读观察的近似规模；目录总大小不等于本次 load 的实际读取字节数。此前日志的 load 为 197,139ms，没有足够分阶段数据确定 CPU、文件读取、验证、结算和 UI 各自占比。

### 3. 已结束子任务被重复核对

- `agent/subagent/mod.rs::reconcile_orphaned_subagents_with_backend`（约 3880 行）重新取得并折叠父 Timeline，再遍历全部 Spawned/Ended。
- terminal 循环（约 4103 行）先执行 `finish_from_durable_facts`，即使 UI 已有 finished 投影，也会读取并验证对应 child Timeline。随后核对 planning result 和 completion receipt。
- `session/actor/planning.rs::accept_planning_task_result`（约 64 行）在确认 link 已 terminal 且 result 相同之前调用 `validate_planning_result_reference`。该验证涉及 child 历史、父全量事件以及再次折叠；`subagent/mod.rs` 约 2900–3050 行可见完整链路。

当有 K 个历史 planning-linked terminal 时，父历史重复折叠存在 K × N 级工作，另外还有 child 历史和 artifact 的重复验证。条件不满足的 child 不一定经过所有分支，不能声称每个 child 都有完全相同成本。优化应让 actor 从已验证事实回答精确身份和当前待修复义务，不以 UI 行或未验证 cache 代替证据。

### 4. UI 历史仍是全量屏障

- `session/history/projector.rs::HistorySnapshot::prepare`（约 79 行）遍历全部 sources，形成 `PreparedHistory.updates: Vec<SessionUpdate>`。cursor 在此投影基础上判断已有前缀。
- `agent/mvp_agent/mod.rs::replay_session_updates`（约 1203 行）转发全部更新并等待 completion。随后 load 响应才结束 Pager 的 loading 状态。
- `pager/src/app/root/dispatch/session/load.rs`（约 162、558 行）在回放前开始 batch，在 SessionLoaded 后结束 batch 并清除 loading。`ScrollbackState.entries` 保存全部历史；离屏 Markdown 延迟处理没有消除全量数据加载。
- `grow/historyCursor` 是完整来源记录已被应用的 reconnect 证明，不是向前翻页的 cursor。名称带 stream 的现有 storage reader 仍先准备全部历史。

当前 `client-surfaces/spec.md` 约 1976、4304、4319 行明确要求完整历史校验和全历史 ready，并排除分页、提前 ready、后台恢复。因此新目标需要修改契约，不能仅提前清除 `loading_replay`。

## 建议的最小架构方向

### 运行现场与历史查询分开，但事实权威保持一个

Timeline 保留 canonical 事实。增加或演进一个能独立恢复当前投影的 durable checkpoint，绑定同一 session、分支/generation、已提交 frontier 和 schema。checkpoint 是已提交事实的恢复投影，不拥有独立任务状态或另一套提交真相。

应保存当前 Surface 与稳定来源引用、续跑位置、运行控制、未结束任务和未闭合交付、输入/交互队列、当前工作流、必要计数和用量投影。已完成任务正文、旧模型请求/响应、过去展示记录和历史 usage 分段留在可索引的磁盘事实中。跨全历史的去重/冲突查询仍需精确的持久身份索引，不能删除去重证据，也不能把全部历史 ID 塞进所谓小快照后宣称与历史无关。

运行期推进 checkpoint，重用现有 canonical owner 的增量状态，在合适的已提交边界发布。退出先停准入并结算现场，确认尾部落盘，再原子发布与该 durable frontier 对应的状态。checkpoint 不可领先于 WAL；进程在发布任一点崩溃后，要么使用上一份快照加尾部，要么使用新快照，不能跳过未落盘事实。

resume 读取现场 checkpoint 和之后的已提交尾部，只恢复未结束树与待闭合交付。只对缺失部分进行原身份结算，不遍历所有已闭合 child。快照缺失/不兼容时可以显式采用完整重建路径或版本迁移，不能把慢路径隐藏成正常快路径。正常退出与 crash resume 分别验收；crash 后尾部可能更长。

### 历史最后一页优先

运行现场恢复与用户历史展示用两个完成状态。最新页可以先显示；输入和自动续跑仍需等待完整的执行现场与控制状态准备好。旧页加载不再持有运行准入门禁。

存储提供基于 record/turn、branch 和固定 cut 的位置索引，使查找最近一页不需要遍历整个 JSONL。可复用现有 record identity、checksum、canonical projector 和 contained directory。具体采用稀疏 offset 索引还是分段日志应在实现设计中决定，不先引入新数据库或另一份 transcript 真相。

页面按新到旧取数，每页内部和最终合并顺序仍按原事实顺序。不要倒序调用现有事件 reducer。分页单位需要保证一个多 part response 的闭合，以及工具调用/结果、anchor、quarantine、rewind 的关联；需要的跨页摘要或来源引用由 canonical 投影提供。

旧页 cursor 与 live/reconnect highwater 分开。后台插入旧页不能改变当前 Goal、工具执行状态、待输入交互或消费前沿。固定 cut 之前是历史，之后是 live；按稳定身份合并，保留滚动和选择锚点，切换会话时丢弃旧请求结果。

建议先展示最近页并低优先级预取相邻页，再按滚动继续读取。后台无限读完整历史只把阻塞移走，仍占用 I/O、CPU 和全量内存。导出、全历史搜索与 rewind 继续走各自完整读取/验证入口，不能把当前已加载页面当作完整历史。

### 完整性需要明确的新边界

当前规范要求每次完整校验所有历史。它与“不读取历史就快速恢复”不能同时原样成立。建议明确现场 checkpoint、尾部及其执行依赖在释放前校验，冷历史与原始采样证据在访问时校验，完整 audit/export 可另行扫描。

旧 hash 或文件大小本身不能证明未读取的旧字节没有被改写。新 checkpoint 的可信提交链、不可变历史块或已有所有权机制的信任边界必须说清楚。当前进程内的 LedgerPrefix stamp 优化不能直接充当跨进程的证明。若仍要求 resume 当场发现任意旧字节篡改，完整读取的成本就不可消除。不得靠跳过校验、相信 summary.json 或后台补验来悄悄放宽执行安全。

## 实施顺序与验收建议

1. **先消除重复工作**：以既有 actor 的窄查询替换 suspension 全历史副本和重复 fold；复用同一 verified cut 的 child/result 证据，定位真正待修复交付。保留当前完整冷恢复校验。这个增量能降低成本，但还不能使 resume 脱离总历史长度。
2. **建立现场恢复路径**：明确上述完整性契约，设计自足 checkpoint、增量尾部和历史身份查找；同时覆盖无活跃 Turn 的 Goal、人工等待、后台 child 与半完成交付。不要先把整个 Timeline dump 成 snapshot。
3. **拆分 UI 就绪与历史分页**：基于可定位的存储事实提供最新页，后台按需补旧页，执行与输入只等待现场条件。必须与前一步共同完成，单纯 UI 虚拟化或提前返回 load 不满足目标。

记 H 为历史规模，W 为当前现场大小，D 为快照后尾部，A 为活跃树/未闭合责任，P 为最近展示页。目标退出关键路径随 W、D、A 和实际 owner 停止成本增长；resume 核心读取随 W、D、A 增长，首屏另外读取 P。历史索引查询可有对数成本，不承诺绝对 O(1)，也不掩盖当前现场本身增长。

验收固定 W/A，构造 1×、10×、100× 的已闭合历史，并分别扩大历史正文、已结束 child 和采样证据。记录退出/恢复每阶段时间、文件与字节读取、复制/重放事件数、峰值内存、durable ACK 次数、首屏时间、输入 ready 时间、首次合法执行时间。快路径不应读取冷历史正文，也不应逐个重开已结算 child；不要仅验证几次 wall time 更短。

失败/竞争覆盖：checkpoint 发布前后崩溃与半行尾部；快照陈旧或引用错误；原 writer 尚活跃；child 退出时刚完成且回执未落盘；迟到结果和重复 delivery；被压缩或 rewind 的正文不复活；历史页与 live 并发到达；分页失败/重复/跨会话回包；checkpoint 仍缺必要依赖时不得提前执行。历史旧页加载失败不应冻结已经完整恢复的现场，但必须按新契约明确报告失败。

本分析仅建议以上后续 change，不实施它们，不承诺尚未测量的速度收益。
