# 实施审查（验证进行中）

本记录是代码审查发现与修正，不替代测试结果。最终结果及可重现命令统一记入 verification.md。

| 反例 | 修正与验收点 |
| --- | --- |
| 把 JSON updates 搬到另一个文件名，仍复制 response 正文 | 物理 journal 仅接收 Domain 或 closed HistoryFact。candidate 无正文，admission 引用 anchor；生产不读写旧 updates，旧 projection DTO/算法只在 test/test-support 用作 oracle。独立原始 echo、早期工具展示和时点快照是必要内容，不从当前 Surface 猜测。 |
| 校验依赖再次编码整个 response | checksum 覆盖版本、物理位置和原始 fact 编码；borrowed RawValue 直接核对。内部 tagged enum 与 RawValue 不兼容的实测失败改为明确的 flat header，未知字段/版本仍拒绝。 |
| buffered 原始输入 echo 尚未写入，Consumed 先获得 ACK | 原 writer 在 Domain durable 提交前冲刷排队展示；steer 先写原始 echo，再提交 Consumed。未消费的 steer echo 有明确标记，不进入已接纳历史。 |
| rewind 用普通 prompt 截断函数会多保留被废弃一轮 | projector 在准备结束后统一应用既有 rewind 分支过滤，避免 inclusive/exclusive 边界混用，也不在每次 marker 上反复扫描。 |
| 第一次 fork 正常、再次 fork 仍引用祖先的 event/item，或误把 seed 当 event 0 | inherited Message 每次投影都输出当前 seed 来源；新 fork 按实际 SurfaceId 和逐项 event 重绑 seed ordinal，不假设 event 0，并清除父 admission/candidate 权威。strip_reasoning 同时过滤 Surface 与展示事实。 |
| 客户端伪造 cursor `complete=true`，跳过尚未完整展开的多 part response | 除 cursor 声明外，还核对对应投影行实际 `complete`；半响应 cursor 的变异用例覆盖此边界。 |
| 捕获后的 offline journal 被截断，却按已验证前缀继续读 | typed captured handle 保留截点校验；源短于捕获长度时 fail closed。 |
| 交叉 session 的 owner 引用被当作本地历史 | 校验 `owner_session_id` 与当前 session 身份一致后才接受引用。 |
| 把 UiNotice 先折叠为 receipt 引用，再检查是否与 receipt 对应 | 在折引用前核验原 body、source、display；receipt 索引保留 compact proof。Inquiry 用 `(correlation, subject)` BTree 前缀做有界 source 查找，缺失 source key 拒绝；同一 source session 的重复 phase 不覆盖首次来源。 |
| Domain 合成 receipt ID 的数字尾部超过 Pager 分配水位，或 hidden/noReplay 记录未计入 | writer counter floor 纳入全部物理位置（包含 hidden/noReplay）和已存最大 ID；Pager highwater 不会被合成 receipt 尾部超越。 |
| bootstrap 为恢复控制状态再次解析完整 History | 复用已加载的 `HistorySnapshot` 选取所需 control receipts，避免重复读取与解析。 |
| 将 candidate preview 的 eventId 复制给 canonical response，尾部 cursor 带 `isReplay=false` 后按数字 ID 去重掉后续 parts | canonical 多 part 输出不复用 preview eventId，使用已验证 cursor 与 Message 来源；prefixed delta 和三 part role/order 单测覆盖后续 part 不丢失。 |
| 旁路增量 reader 混用物理位置与 Domain seq，或错误吞下坏的 presentation | Trajectory cache 逐条调用统一 decoder 和 `AppendState`；拒绝时恢复已验证物理前缀及 Timeline/projector 状态。测试包含 Domain/presentation/Domain 交错与 checksum 损坏后修复续读。Search `TimelineLedgerReader` 固定使用捕获的 `snapshot_len`，不把快照之后的 append 混进当前读取。 |
| resume 暂停后才启动的新 attempt 未被登记 | actor 自己持有 writer barrier；release 时读取当前 sampling preview，先安装 resync 再释放 writer。load 取消也按此顺序解锁。不得把建立屏障时的一次 key 读取当成完整覆盖证明。 |
| 导出逐项取当前文件，得到混合时刻数据 | 导出绑定已验证 journal 截点及原字节 hash；import 先校验 typed 引用和全部传入 artifacts，再以既有 staging publication 发布，不要求尚未落地的 Notice artifact 已能被 projector 读取。 |
| 同身份不同正文或同步失败后仅“读得到”就 ACK | 原 identity/payload 冲突与 file/directory durable barrier 保留；prefix 先跟进已写入行，重试同步原记录。普通展示 append ACK 与 durable ACK 分开，不把每条通知升级成 fsync。 |
| 只检查消息数，文字还在但归属或排版错位 | 验收包含完整 wire 字段、多类型混排、canonical 多 part cursor、双 fork，以及逐前缀 root/child 的角色、工具映射和 80/120 列展开渲染。真实 PTY 另查持续滚动、慢 writer、Ctrl+D 和完整 ready barrier。 |
| 只迁移主 session reader，测试夹具或资源专用 reader 仍依赖裸 Domain/updates | 旁路 reader fixtures 覆盖 Skill、root-child-grandchild、orphan、tool-duration、leader、plan、export、image 与 search；memory gates 保留原阈值。此处记录覆盖范围，不声称测试运行通过。 |
| 删除 cache validator 时丢掉原有失败义务 | 输入 fingerprint 在统一 History 上验证；离线选中分支的非 Recovery committed user turn 仍要求 exact prompt 的 TurnCompleted。旧 oracle 的失败场景保留，并补 typed 路径断言。 |

## final-10 至 build12 运行发现与修正状态

- fork memory gate 未改阈值：final-10 为 1392020 B stream peak / 433260 B journal（3.21290x，要求 <2x）；final-11 为 2.31917x，仍失败。保留完整 rewind/cursor/顺序逻辑下，将只读 reader 改为借用/消费同一 projector 的 owned fact，并将稀疏 Grow variant 装箱；final-12 为 742658 B / 433260 B（1.714116x），300 个 response history 项通过原 <2x gate。该修正降低同一 input 的解析峰值，不改 input 规模或阈值；正式 A/B 性能测量仍 pending。
- build10 的 image cold-restore 超时和 subagent bash status 失败经定位来自 test persistence fake 丢弃 typed display update / 未保留 persistence owner。fixture 现写入、ACK 并发布 deferred delivery；build12 对应五个 image restore 与三个 bash status 用例通过。旧 fake 的“读取队列即送达”假设已从这些路径移除，未删除消息或 failure assertion。通知桥 build12 为 33/33，actor updates 为 20/20，actor spawn 为 27/27；完整 actor 组仍有一个 ACK-loss fixture 的路径 `NotFound`，详见 `final-12-session-actor-tests.log` 和 `actor-failure-analysis.md`，不得记为通过。
- Timeline 的重试契约将瞬时错误和 ACK 不明保留为 exact pending 事件，永久错误才终止 writer epoch。故障注入须区分一次可重试错误与永久失败；测试外层超时也必须大于被测 30 秒持久化截止时间。持续 ACK 丢失依然要验证没有 Accepted、provider continuation 或工具执行，且物理 admission 只存一次。

## build12 验证边界

Pager 本轮 `profile.test=true` scoped groups 为 755 passed、0 failed、1 ignored；pager-pty-harness 普通 unit tests 为 96/96。Shell 的 history、JSONL、persistence、trajectory、transcript、reader 等 scoped groups有通过结果，但 `session::actor::tests` 为 441 passed、1 failed、1 ignored，失败为 `response_admission_physical_ack_loss_blocks_tool_dispatch_and_recovery` 在 test fixture 路径读取处得到 `NotFound`；`extensions::session_state::tests` 为 9 passed、1 failed，失败为缺少合法 session summary format version。build12 CLI manifest 的结果不能当作 smoke 通过：runner 记录的前 11 项实为 0 passed（ignored 项未执行，部分 exact test 名未匹配）；两个强制运行 ignored 项也失败，分别为 ACP initialize transport closed、测试配置没有 LLM provider。根代理已识别 runner 缺陷并在 manifest13-cli-real 中重跑；以该次真实执行结果为准，不将旧 build12 CLI 记录计入通过数。完整 scoped 结果见 `final-12-pager-results.json`、`final-12-shell-results.json`、`final-12-cli-results.json`。

截至 build12，`session_load_peak_rss_under_budget` 通过（peak growth 0.78125 MiB / 1024 MiB budget），而 `session_load_e2e_peak_rss` 未产出测量。memory13 后续通过 load 与 e2e RSS gates，具体值见 `final-13-memory-results.json`；这不代表正式 A/B 或完整 PTY/RSS 矩阵完成。build12 本身不是最终总体验收。

## build13–20 当前证据与待复验项

build13 Shell scoped groups 共 1164 passed、0 failed、4 ignored（18 groups，见 `final-13-shell-results.json`）；其中 actor 442/0/1、actor updates 20/20、actor spawn 27/27、notification bridge 33/33，先前 build12 ACK-loss fixture failure 已通过。Pager scoped groups 为 755/0/1；所用 Pager test binary SHA-256 `16a09447d5aa59317804f220ccf5928635d767721988472dc5b96e4c4fb4f4f6` 与 build13 manifest相同。pager-pty-harness 普通单测 96/96。300 项 fork replay memory gate 为 1.714116x（原 `<2x`），memory13 roundtrip、load peak RSS 和 e2e RSS gate 均通过；这些属于 gate/correctness 运行，不是正式 paired A/B。

CLI 的逐项结果只记实际运行：final13-real #01–07 各 1 passed；#08 因缺 `ContentController::seed_llm_config()` 失败，#09–11 在相同缺配置问题处按授权终止。final14 minimal #08 为 1 passed；final16 leader reattach #09 为 1 passed。其余 CLI 与 PTY 组合尚未由这些单项结果覆盖。build12 CLI runner 曾把 ignored/exact 名称未匹配项报告为 0 passed，该批数字不计作通过；修复后的逐项 runner 保留每份原始日志。

最近 `agent::subagent::tests` 为 163 passed、4 failed；四项都报 `persisted Timeline storage version 27 is incompatible with current version 1`。已定位为测试 helper 仍写裸 Domain fixture，而不是生产 codec 故障；helper 已迁移，待 final-unit-build复验。orphan PTY 原来因 fixture 缺少合法 parent turn而触发 turn-boundary failure，修正后的物理证据现已定位为 seq40–44 的新 notification turn/Hook/recovery 交错；清理顺序修正与确定性回归待 build21/CLI4。dense slow-writer PTY 的 run20 长负载结果已通过，512-turn一帧旧失败与2048-turn多帧通过的差异见下文；不能将旧短 fixture 失败解释为长 replay 卡顿。

final20 dense slow-writer PTY 对 2048 turns（每条 chunk 4 KiB）通过：首个可见序号在 508.0 ms、末个在 622.5 ms，共 2 次可见推进，最大间隔 114.5 ms。原 512-turn 失败日志显示 first/last 都约 287 ms、只出现最高序号 4095 一次；这是完整 history 快速单帧完成，不是长时间卡顿。测试仍使用原 `>=2` frames 与 `<2s` gap 阈值，未放宽。

final20 `leader_reattach_cancellation_roundtrips_durable_log` 通过。证据确认 T1 已先完成 admission，随后才取消 T2；T1 正文是合法 canonical history，不应按未准入 candidate 删除。旧期待将此正文视作 unadmitted 不成立。

orphan recovery 的 `final-19-orphan-debug.log` 物理记录显示 seq40 为新的 notification Turn、41 `HookTriggered`、42 `Recovery`、43 `HookCompleted`、44 Recovery `TurnEnded`。旧清理在同一恢复轮次误把新工作创建的 Hook 关闭。root 已调整为先恢复 child/parent state，再清理，随后发 UI/receipts；复用已有 maps，不增加 second fold，并通过内部 `restored` 标记禁止补发 presentation 或重跑 `SubagentStart`。确定性 unit（收到 receipt command后新建合法 Hook仍保留）及 orphan真实 child + 旧 Hook 只完成一次的 PTY 回归仍待 build21/CLI4 验证。

final13–20 的有限结果不表示最终完成：build21 与 CLI4 最终验证、orphan 与 Hook 回归、正式 final-paired（旧 root paired 数据只作为 reorder 前 candidate）、完整 PTY/RSS 矩阵、剩余 CLI/leader 场景、agent subagent fixture复验及 OpenSpec strict 仍 pending；不勾选 tasks、不归档 change。

## 独立既有边界

Subagent reconcile 对 `CompletedOutputUnavailable` 的映射未由本次 reorder 引入：`result_from_inspection` 仍把它转换为 `Completed` 且无 output，reorder 仅改变调用处的借用方式并把 child-result/parent-terminal repair 移到 interruption cleanup 前。若 child entity 存在但尚无合法 `SubagentResult`，这可能把不可验证的完成观察固化为成功终态；现有 `unavailable_completed_output_keeps_parent_spawn_open` 只用缺失 child 覆盖，未覆盖该组合。已记入 [backlog](../../../backlog.md)，不是本 change 的通过项；需另行补测试与行为 change。已有合法 canonical result 的恢复路径仍按 exact result link 验证。

## 成本和边界

- 新格式是显式 v7，不提供生产双读或自动迁移。Domain 事件 schema 仍为 v27，物理 record 为 v1；物理位置与领域 seq 不混用。
- 新 checksum 与 typed metadata 有 CPU/空间成本。所有事实、必要引用及完整展示仍须处理，复杂度仍为 O(历史事件和正文)。没有检查点或提前 ready。
- 单文件不等于跨 artifact 事务；artifact 先发布而日志失败仍可留下无引用对象，沿既有策略处理。
- runtime writer lease、Sideband、summary、rewind 和 workflow 资源继续各司其职。局部合并不能代替这些所有权或 durability 机制。
- live 可撤回 preview 与独立通知继续沿原协议交错；canonical 历史以 anchor 为序。恢复中断后的 replacement 仅替换 exact attempt 的 provisional 条目，不能撤回已确认的邻轮或工具/notice。
- 旧 repair profile 在新格式下没有第二份 projection 可修复；比较保留同一业务内容，但必须把这项结构性差异写入结果，不能声称新路径仍执行旧 repair。

## 最终补验（以最终 verification 为准）

final22 Shell 19 组 1331/0/4，final21 Pager 755/0/1、harness 96/96、完整历史 oracle 4/4 与全部 memory gates 通过。final21 CLI #01–12、dense #14 为真实 pass；最终工具/cold ACP 组合 final25 通过。原 runner 空选择、旧 fixture、内存门槛和中间失败不删，分别保留原日志；新结果不把它们改写为通过。

工具补验没有给 metadata-only ToolEvent 增加一份正文，而是核对生命周期 success 与其余两个来源实际拥有的 rawOutput/ToolResult；负例还暴露了测试 headless canonical cwd 与 ACP raw cwd 的命名空间差异。fixture 按 Summary.info.cwd 加载并确认仍为原隔离目录。完成后 cold load 的命令标记文件、推理请求数不变；详见 final25 与 scenario-coverage。正式 paired 数据另存 final-paired，不以早期 CLI3 候选成绩替代最终结果。
