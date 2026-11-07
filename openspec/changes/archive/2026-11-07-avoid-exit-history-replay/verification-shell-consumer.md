# Shell consumer verification

初始 consumer 写面仅 `crates/codegen/shell/src/agent/mvp_agent/runtime_handoff.rs`（consumer、必要回归及长历史 fixture）和本记录。起点该 Rust 文件没有预先 dirty diff；其它 workspace dirty 改动全部保留。ChatState provider 已由父接受，未在本项重做。后续B真实ACP五秒失败后，父另明确授权 `session/actor/teardown.rs::stop_session_background_services` 函数内的service join诊断；只加观测，不改生命周期语义，原有其它teardown dirty改动保留。

## 验收边界

live `runtime_snapshot` 使用 canonical provider 的同 mailbox runtime + 全部 Spawned/Ended 投影，再经 `CanonicalChildFacts::scan` 检查 child 重复和身份。closed handle 仍 `load_light`，只对其已验证 Timeline 扫描；未知来源和恢复上的 `CanonicalChildFacts::read` 完整验证保留。三次挂起读取均返回 facts，最后读取必须在 child writer/finalization 退休确认之后。五秒绝对 deadline、持久 ACK、完整树和 writer 退休不放宽。

## 实验入口

`agent::mvp_agent::runtime_handoff::tests::long_history_exit_through_acp` 在隔离 HOME/GROW_HOME 子进程中创建真实 `MvpAgent` session，通过 ChatState canonical owner 追加 8,000 条 4,096 字节用户正文并等待持久 barrier，然后经实际 ACP `grow/session/suspend_for_exit` 入口退出。输出 canonical events、序列化 bytes、setup/exit elapsed 与阶段日志。没有 provider 执行，也没有 fake node/checkpoint ACK。成功时核 actor/writer 退休和 cold `load_light`。`GROW_TEST_EXIT_HISTORY_EVENTS` 只调整 fixture 数据量，不改变退出期限。

基线保留原 consumer。测量命令固定：

```sh
PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 /Users/lordcasser/.cargo/bin/cargo +1.93.1 test -p shell --lib agent::mvp_agent::runtime_handoff::tests::long_history_exit_through_acp --locked --offline -- --exact --ignored --nocapture --test-threads=1
```

原 consumer 首轮已启动，source SHA-256 为 `1c7eedf956c1e10c43971861504f1544e45602b8d023a9a97aedec5f4aeef351`。原始完整日志：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b447-eff1-7542-802e-33251f2d4832/terminal/call_0110d25be66a4d44a30db8b0b8fbe663.log`。运行结束前未切 consumer。`get_command_output` 两次返回 observation timeout；已核实际 Cargo/rustc 进程仍运行，并用 monitor 观察该进程退出，不由观测工具失败推断测试结果。

## 原 consumer 实际结果

原 exact 命令 exit 0，编译 2m52s，测试体 120.04s（其中 setup 117,830ms）。canonical 8,006 events / 34,227,645 serialized bytes。实际 ACP `suspend_for_exit` 106ms 返回 `{"status":"suspended"}`；actor/writer 退休通过，cold `load_light` 读回 8,011 events。**原绿，未复现五秒未确认**。该 fixture 是普通 User Append 长正文，不能用其成功解释现场真实工具会话，也不证明窄 consumer 已修用户问题。

| 阶段 | 原 consumer 累计 elapsed ms |
| --- | ---: |
| tree initial_facts | 4 |
| tree frozen_facts | 18 |
| Session sampler_settled（从 node started 起） | 0 |
| Session commands_settled | 5 |
| Session owners_drained | 18 |
| Session checkpoint_committed | 29 |
| tree children_retired | 51 |
| tree latest_facts | 55 |
| tree confirmed | 105 |
| ACP result | 106 |

编译出现既有 `session/actor/input_admission.rs:2528` irrefutable-pattern warning，未修改此处。父收到真实基线后明确要求不再两分钟重复生成同类 fixture；本项定位为窄 consumer 优化，后续用快速空会话核真实退出/cold 并执行树回归。最终 CLI 同 binary 双 Ctrl+D/恢复里程碑由父协调，仍未完成。

## 真实工具历史的定向差异（静态证据，未认定因果）

`Timeline::from_events` → `accept_replayed`（`timeline.rs:3756`）逐事件先验证 content，再 apply Surface。普通 User Append 的形状验证只扫描当前新增 items（5032）。当前 accepted `ResponseAdmission`（4971–5002）已使用 completed-attempt/identity HashMap/HashSet 与 `SurfaceToolIdentitySummary`，仅扫描新 assistant calls；不能凭 accepted-response 数量推断整份 Surface 重扫。

最小可疑机制候选是工具历史中的全 Surface 替换事实：`IntegrityRepair` 的 validation（5127）进入 `valid_integrity_repair`（7142），至少复制旧 Surface、修复，再由 `conversation_slices_match`（7187）将 before/replacement 全量转 JSON 比较；不同既有修复策略可产生多次复制/比较。`ToolResultPrune`（8204）同样遍历全 replacement，对非 ToolResult items 转 JSON。旧 `CanonicalChildFacts::read` 在每次 snapshot 都会重新执行这些历史 content proofs，普通 User-only fixture 没有此类事实。

最小后续证据：现场 ledger 中 `Messages.cause` / `surface` 的分类计数、每类 items/bytes，按原始 source 测 `from_events/read` 阶段，再与窄 snapshot 比较。这只是候选，不是现场已有这些事件或它们耗尽五秒的证明。未改 ChatState、Session projection 或另立 provider。

## Consumer 实施与回归

最终 consumer 返回 `(RuntimeLifecycle, CanonicalChildFacts)`；live 用现有 `runtime_tree_snapshot` 后 `scan`；closed 用原 `load_light` 后 `from_validated_timeline`。`from_validated_timeline` 在本项起点已直接 scan，本次没有重复修改。三个树协调 snapshot 直接消费 facts，末次读取仍在 `CheckSuspendedRetirement` 确认后。`child_has_verified_terminal` 与 restoration 的 `CanonicalChildFacts::read` 完整 ledger 验证保持；无 ChatState、Session actor、Skills、SDK、Pager 或主 spec/docs/backlog 改动。

已编译/测试的 consumer 源码 SHA-256 为 `8525934825b103f728f74a0995f6e4bb5fe154740096ff68abdd8c5a03490ebb`。之后只增加了下节 ignored probe；新增 probe 尚未编译，不能称其通过。

完整原始输出目录：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b447-eff1-7542-802e-33251f2d4832/terminal/`。

| 检查 | 实际结果 | 原始日志 |
| --- | --- | --- |
| 原 consumer exact ignored 长历史（上文完整 Cargo 命令） | exit 0；1 passed；ACP 106ms，原绿 | `call_0110d25be66a4d44a30db8b0b8fbe663.log` |
| 新 consumer `cargo +1.93.1 test -p shell --lib agent::mvp_agent::runtime_handoff::tests --locked --offline -- --nocapture --test-threads=1` 首轮 | exit 101；10 passed / 3 failed / 2 ignored。新增 Cancelled terminal 夹具漏非空 error，Timeline 在 consumer 前拒绝 `InvalidSubagent`，不是产品失败 | `call_8b6b418617c24b3fb2afb512a769dff8.log` |
| 同命令修正夹具后重跑 | exit 0；13 passed / 0 failed / 2 ignored；编译1m39s，测试体0.13s | `call_0d6d60db0a2248519482e39446b458f0.log` |
| 同 unit binary，`GROW_TEST_EXIT_HISTORY_EVENTS=0 ... --exact agent::mvp_agent::runtime_handoff::tests::long_history_exit_through_acp --ignored --nocapture --test-threads=1` | exit 0；1 passed，2.11s；6 events / 12,737 bytes，真实 ACP 72ms，actor/writer 退休、cold facts 一致、fresh storage 验证拒绝坏 JSON 全通过。此小数据与原8k基线不可作性能比较 | `call_d359dfd44dcb420895d25cc5b5fb7c21.log` |
| 同 unit binary，`agent::mvp_agent::tests::suspension_admission_tests --nocapture --test-threads=1` | exit 0；6 passed；5.16s。scope 隔离、lost waiter、物理 writer、共享 deadline、freeze retry/ACK unknown 保持 | `call_2e765edca55d4f2b9feab2648ad7a487.log` |
| `openspec validate --all --strict --no-interactive` | exit 0；31 passed / 0 failed | `call_35b69cfa3e164559b7992b896bce6bcd.log` |

所有 Cargo 命令使用标准 PATH、1.93.1、JOBS2、INCREMENTAL0、STACK16MiB、locked/offline。直接 binary 命令同样设置这些环境（不启动 Cargo）。unit binary 为 `target/debug/deps/shell-34fc88a27b856cb5`，SHA-256 `c3fde019f7b681ce8e742c24c7f45559bab004b5713cce38365e7d805f7bcb00`。当前没有本项运行中的 Cargo/测试；窗口已明确释放父级给 AC07→Skills→PB05。

新增小回归包括直接 scan 的重复/身份检查、不可用 live owner 错误、gapped 投影的完整 child 与晚到 Ended、新旧 snapshot 独立、正常 open child checkpoint 保留、退休确认时才提交的 terminal 被末次读取观察并因缺 canonical result reference 拒绝树 barrier。后两个测试使用受控 node/coordinator 夹具，不能替代真实 CLI 或完整 child result 链里程碑。未放宽原断言、五秒预算或退休屏障。最终 rustfmt check 与局部 diff check 通过；自查不等于独立 review。

## 现场 fixed-prefix probe：已执行，重复 replay 超过五秒

父提供的原现场 root 为 `01a1ac97-7c83-7a82-82d1-74485a78ef4d`。父 read-only 证据为约291MiB、17条 IntegrityRepair / 49条 Replace；最后一次 SuspendRequested→CommandsSettled 2204ms，之后无 checkpoint/tree。该来源不同于原 user-only fixture，不能将当前 consumer 优化称为用户退出修复已完成。

已通过源 `timeline.jsonl` 的 O_RDONLY pinned fd 取得初始 fstat 长度，只复制此固定长度到 `/tmp/grow-exit-history-lvyq1tc4.jsonl`。没有调用任何源 session load/resume/writer 接管或修改。

- 副本/source prefix bytes：304,768,726。
- SHA-256：`400bdd6b93406013a0114ff7c439e81952a0bf1c7beac4ca3d34be129fedc833`；末尾完整 newline。
- 源 device 16777233 / inode 76936560；复制前后 size 均304,768,726、mtime_ns 均1794024079904091048。
- 仅对副本做 JSON envelope 扫描：104,382 physical records，末 physical seq104381；76,713 domain events，末 domain seq76712，最后是 control。该扫描不是 Rust canonical validation，不能代替 probe。

probe 为 `agent::mvp_agent::runtime_handoff::tests::fixed_prefix_child_facts_probe`，只接受 canonical path 位于 `/tmp` 的副本。它记录 bytes/hash/newline、read/decode、首次完整 Timeline validation、旧 full-copy+read、新窄 filter+scan，以及投影数量/字节与 facts 等价。只使用内存 Timeline，不持有原源 writer，也不启动原 session actor。阶段计时独立，不把某个 replay 耗时当真实五秒 ACP 未确认。

probe 已由父安排的 Shell runner 编译并实际执行。精确原始日志：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b472-6a7c-79c0-8263-bc7b4da5b5f5/terminal/call_8cc2c36fbb20499089d0b099.log`，1 passed / 0 failed，测试体81.22s。该日志核对同一副本 hash、完整 newline、全部76,713 domain events及facts等价。

| 实测阶段 | 实际耗时/数据 |
| --- | --- |
| 固定副本 read | 48ms |
| physical decode（104,382 records） | 23,651ms |
| 首次完整 Timeline validation | 23,719ms |
| 旧 full events copy | 727,469µs |
| 旧 CanonicalChildFacts::read replay | 24,637,884µs |
| 旧 copy＋replay 总计 | 25,365,353µs |
| 新 filter＋scan | 3,608µs |
| 新 Spawned/Ended 投影 | 382 events / 918,083 bytes |
| child facts | 191 Spawned / 191 Ended，facts_equal=true |

实际 causes：Assistant2617 / Compaction31 / ContextRebuild2 / IntegrityRepair17 / MemoryContext75 / SkillLoadModel31 / ToolResult5686 / User39；49条 replacement。此结果证明该复杂 canonical 历史的旧 consumer 重复 replay 一次就消耗超过五秒；初始化/decode不是 live snapshot 的必需工作，不能把它们相加声称退出耗时。该测量使用内存已验证 Timeline，尚不等同真实 ChatState mailbox／ACP／CLI 新绿。同步 replay 不可被 Tokio timeout 抢占，后续 trusted-owner 回归将同时记录实际返回、elapsed 和 deadline 是否已耗尽。

执行命令：

```sh
PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 GROW_TEST_EXIT_TIMELINE_COPY=/tmp/grow-exit-history-lvyq1tc4.jsonl /Users/lordcasser/.cargo/bin/cargo +1.93.1 test -p shell --lib agent::mvp_agent::runtime_handoff::tests::fixed_prefix_child_facts_probe --locked --offline -- --exact --ignored --nocapture --test-threads=1
```

后续范围已由父确认：先用全部现场 domain 事实初始化真实 trusted ChatState owner，通过现 handle 对比旧精确 body 与新 `runtime_snapshot_before`；真实 ACP/CLI 采用合法 synthetic 复杂历史，保留 accepted assistant/tool、IntegrityRepair、Compaction 事实和 typed journal，不复制191个 child tree。原现场源不执行 load/resume/writer 接管。

## Trusted-owner 与真实复杂历史退出：A 已验证，B 进行中

父在 Shell runner 明确释放后解除 Rust freeze，并在 PTY Service 命令实际退出后预授本项独占 Cargo。只在 runtime_handoff 测试与本记录实施，没有生产接口变化。

- `validated_owner_snapshot_deadline_probe`：完整固定副本 decode/初次validation 与 `ChatStateActor::spawn_from_validated_timeline` 初始化分别计时，真实 mailbox ready 后复现旧 runtime-state/full-events查询，再在原调用方位置同步 `CanonicalChildFacts::read`；记录其实际return、query/replay/总耗时、共享五秒deadline是否已过。新路径调用实际 `runtime_snapshot_before`，核runtime与child facts等价、实际返回和deadline。只测 trusted-owner boundary，不制造 node 或 writer ACK。
- `complex_history_exit_through_acp`：默认256 accepted assistant/tool exchanges，每个工具结果64KiB；17次实际duplicate-result去重后IntegrityRepair全Surface替换；31次 `replace_compaction_range` 小范围合法 CompactionMeta 替换。无child/sideband/blob依赖。生成可完整验证的typed domain journal，每个admission用 `ResponseAnchor::Admission`；先实测同source旧read，再走实际Mvp `load_session`→ACP `suspend_for_exit`、actor/writer退休、原source prefix hash及cold TreeSuspended验证。old<5s也先记录实际ACP结果，最终明确夹具未达旧红，不伪造超时。

B可指定 `GROW_TEST_COMPLEX_EXIT_HOME` 保存 `/tmp` 隔离会话给父级同binary CLI双Ctrl+D与恢复；`complex-exit-fixture.json` 保存sessionId/cwd/sourceBytes/sourceEvents/hash/旧replay实测。`GROW_TEST_COMPLEX_EXCHANGES` / `GROW_TEST_COMPLEX_RESULT_BYTES` 只控制历史负载，不改退出预算。

A命令（固定PATH/1.93.1/JOBS2/INCREMENTAL0/STACK16MiB、locked/offline）：`GROW_TEST_EXIT_TIMELINE_COPY=/tmp/grow-exit-history-lvyq1tc4.jsonl cargo +1.93.1 test -p shell --lib agent::mvp_agent::runtime_handoff::tests::validated_owner_snapshot_deadline_probe --locked --offline -- --exact --ignored --nocapture --test-threads=1`。实际exit0，1 passed / 0 failed，编译1m47s、测试73.09s。原始日志：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b47c-2e09-78d1-a28e-a66b6b5269c8/terminal/call_653207fc6f884c94ad4a501f54d1e82a.log`。

| A trusted-owner 阶段 | 实测 |
| --- | --- |
| 全部76,713 domain events decode＋首次validation | 47,847ms，退出预算外 |
| spawn_from_validated_timeline＋真实mailbox ready | 20ms，退出预算外 |
| 旧 runtime_state＋timeline_events mailbox查询 | 709ms |
| 原调用方同步 CanonicalChildFacts::read | 24,341ms |
| 旧总计／实际返回／五秒期限 | 25,050ms／Ok／deadline_expired=true |
| 新实际 runtime_snapshot_before／五秒期限 | 3,916µs／Ok／deadline_expired=false |
| 等价性 | runtime相同，191 Spawned／191 Ended facts相同 |

旧查询返回后同步重放不能被 Tokio timeout 抢占，测试明确报告旧返回为Ok但共享deadline已耗尽，未伪造准时Err。该用例真实经过canonical actor/handle，NullTimelinePersistence仅用于初始化内存owner，不承担任何node或writer退休证明；真实退出由B核验。

B首轮同binary执行exit101，0.51s，在构造Compaction replacement时 `CompactionReplacementNotOpen`；未开始真实load/exit，属于夹具缺完整transaction，不是产品旧红。原始日志本续跑terminal `call_fd019c2dc7ae4c88be72df9fabdf25a9.log`。现已补Started→Summary→Replace→Completed及31个隔离自生成小Sideband(Request/Attempt/Result/End)作为必要摘要证据，使用既有storage durable sideband writer；不复制任何外部源artifacts或child，也不更改生产API。旧“无sideband依赖”仅指不依赖外部资料；合法Compaction契约确需本地证据。

修正后首个编译命令误将隔离HOME传给rustup，触发工具链下载尝试，已立即终止，尚未到Cargo/test；记录于 `call_fd75a2bdb528424ba133daaddced5226.log`，不计测试失败或产品旧红。正式重跑显式固定已有 `CARGO_HOME=/Users/lordcasser/.cargo` / `RUSTUP_HOME=/Users/lordcasser/.rustup`，其余工具链/资源配置保持，B测试源码SHA-256 `445f20bd880394d0498652a9b9a36fc6ea1361270fdeae20a85d06fbe670e0ca`。持久隔离HOME为 `/tmp/grow-complex-exit-x6s9g7kk`。

## B 真实 ACP 失败与下一个最小观测

修正合法Compaction后的B已实际执行完成，exit101，测试58.48s、编译1m46s。完整原始日志：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b47c-2e09-78d1-a28e-a66b6b5269c8/terminal/call_71b2398677d14573b45b80e06f48db01.log`。真实source通过完整storage validation并成功 `load_session`；随后ACP在共享五秒deadline返回unconfirmed，生产失败断言保持。

- source：305,653,152 bytes / 2,494 domain events，256 accepted tool exchanges、17 IntegrityRepair、31完整Compaction事务和31个自生成小Sideband。
- source SHA-256：`e03d6a470d6b971aef4078e5d5a2e06aaa940d5d6f74a52f4aeae94441302ef8`。
- 生成8,179ms；旧copy＋replay4,001ms。该B负载未达到五秒旧红，作为原绿控制记录；现场A的旧25,050ms已独立提供机制旧红。
- 实际public load：22,967ms，退出预算外。session id `01a1b491-f49e-72a3-a667-912e1d2ca935`。
- 实际ACP返回：`{"status":"unconfirmed","error":"runtime 01a1b491-f49e-72a3-a667-912e1d2ca935 suspension remains unconfirmed"}`，elapsed5,002ms。
- root initial_facts41ms / frozen_facts55ms；Session node sampler_settled0ms / commands_settled5ms / draining_services12ms；五秒前无draining_auxiliary_owners、owners_drained或checkpoint。

父随后收拢判据：B只验真实load→ACP→退休/prefix/cold，不要求B再造旧helper>5；仅删除末尾旧负载>=5的夹具断言并保留计时。A严格现场旧>5断言和B所有真实ACP/持久ACK/cold/sourceprefix断言保留。没有任意增加负载。B本次实际失败发生在真实ACP断言，不能用此收拢改写为通过；本次未执行post-exit prefix/cold断言，也不宣称writer已确认退休。测试panic结束过程不是产品成功退出。fixture metadata保留在 `/tmp/grow-complex-exit-x6s9g7kk/complex-exit-fixture.json`，尚不能作为已确认suspended的CLI里程碑source。

精准已证失败调用为 `suspend_root_tree_locked` 等待 `SuspendRuntimeNode`，Session进入 `suspension.rs:1277` → `stop_session_background_services(false)` 后未返回。此函数在 `teardown.rs:62-73` 对 `notification_reconciliation_worker` 等进行 `join_background_service_exact`，不能detach blocking owner。当前日志不能区分该函数内部哪个join仍等待，需最小动态观测，不能据静态候选宣称已锁定owner。

定向只读发现的最小可疑路径：`spawn.rs:3436-3445` 的notification reconciliation worker并行执行三个reconciler；`input_admission.rs:792-809` 的user-image资产reconcile在开始后通过spawn_blocking调用 `image_describe.rs:299-307`，那里无条件读取完整物理Timeline并 `Timeline::from_events`，之后才检查assets目录是否存在。即使B没有images/assets，也会执行此完整read/decode/replay；shutdown token在blocking调用前检查，一旦已开始，exact join等待其真实完成。该机制与B的冷load后立即退出、卡draining_services相符，尚需动态确证。

已先上报父级，未越过当时生产写面。父随后授权最小生产观测，结果如下。Session其余全history clones是否仍占预算尚未被此实验判定，不据此新增通用projection。

## Service join 动态归因：已确认 image reconciliation 整史读阻塞

父明确授权仅在 `teardown.rs::stop_session_background_services` 内对各join/shutdown增加service label、started/finished与累计elapsed。只记录诊断；取消、调用顺序、所有等待/timeout、错误传播和退休屏障保持。该文件本来已有 `drain_final_persistence_owners` 等dirty改动，完整保留。本轮未改图片清理算法或ChatState查询。

同参数B在新隔离HOME `/tmp/grow-complex-exit-observe-_kfu_qik` 执行，原失败source保留。原始完整日志：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b47c-2e09-78d1-a28e-a66b6b5269c8/terminal/call_60982b6811d94dca9ad6b2abe8b6aeaf.log`。实际exit101，测试59.37s，编译1m43s。source仍305,653,152bytes/2,494events，hash `b90a30f8a4e61847e3374ebc560f833169ec690cf32173134b3a861e761814b0`；生成8,231ms，旧copy/replay3,977ms（控制值）；real_load22,883ms。

| 观测 | 结果 |
| --- | --- |
| root initial/frozen facts | 41/46ms |
| Session commands_settled/draining_services | 5/14ms |
| ask-user-question service join | started/finished均0ms |
| context-recall service join | started/finished均0ms |
| notification payload reconciler exact join | started0ms后无finished，下一service未开始 |
| ACP | unconfirmed，实际6,276ms返回；原共享deadline仍5,000ms |

在notification join开始时，monitor对实际测试PID做了一次1秒只读 `/usr/bin/sample`。采样artifact：`/tmp/grow-complex-exit-observe-_kfu_qik/service-join.sample.txt`。其中304–315行明确给出blocking task调用栈：`SessionActor::reconcile_user_image_assets` closure（input_admission.rs807）→ `image_describe::reconcile_user_image_assets`（image_describe.rs303）→ `JsonlStorageAdapter::read_timeline_from_directory` → `read_journal_prefix_data/scan_journal_range` → `history::decode_record` → `serde_json`。因此不是仅凭静态猜测：动态join最后owner和同期blocking栈已共同确认无assets的B也在image资产reconcile整史读，持有真实退休责任。

采样可能暂停/扰动进程，诊断版6,276ms不能作无观测性能结论；前轮**无采样**的5,002ms真实unconfirmed才是原deadline失败证据。两轮均保留真实失败断言，没有等后台预热或延长deadline、跳join、强制退出。由于B未确认成功，post-exit prefix/cold断言仍未执行，CLI同产物里程碑尚未进入。下一最小修复契约由父冻结在image reconciliation owner，必须保留已提交图片引用与清理完整性；当前不顺带实施Session其它projection。

最终已测源码hash：runtime_handoff.rs `262cca7c91617d5b2a9bbb4a84e234dd469cdcbc84f3438f34a8672bc2c08d3a`；teardown.rs `c0b89044fe93284491a773a3db51f20316380d80eb298f7abb25363d993ab8e8`。同unit binary `target/debug/deps/shell-34fc88a27b856cb5` SHA-256 `d8b63f1e0fb8bea50faf48e8b560acf2374475ca82db0bd5c2c44f9d314c3159`。rustfmt check/局部diff check通过；该轮Cargo/测试/monitor已结束并明确释放。以下记录第三增量后新一轮真实B结果，不覆盖原失败证据。

## 第三增量后：B 真实五秒退出新绿

2026-11-07，父确认ChatState `committed_user_image_paths` provider 7/7通过、Shell清理consumer source ready后，授予独占Shell lib窗口。本轮没有修改任一owner业务源码或测试；只运行授权验证并更新本记录。runtime_handoff/teardown与前轮源码hash一致。A完整现场owner旧红/新绿未重复，B负载未改变，未预热、采样、延期限或跳任何退休/持久屏障。

本轮原始日志目录：`/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b4a2-108e-72e2-85f5-199a572526f4/terminal/`。

### 实际命令与结果

所有命令使用 `PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`。唯一Cargo命令为：

```sh
/Users/lordcasser/.cargo/bin/cargo +1.93.1 test -p shell --lib user_image_asset_reconciliation --locked --offline -- --nocapture --test-threads=1
```

任务 `01a1b4a2-6f72-7eb2-a1bb-040e363c8e31`，exit101：编译1m49s成功，8 tests实际7 passed / 1 failed / 0 ignored，测试0.48s。原始 `call_e8599beb4253475d9ae45732b031083c.log`。失败为 `session::actor::input_admission::tests::user_image_asset_reconciliation_retains_current_and_historical_owner_refs`，在 input_admission.rs920 的 `replace_context_durably` 前置夹具上返回 `Invalid(InvalidMessageShape)`，未执行清理断言；其余3 actor / 4 helper通过。已精确交父由原owner窄修，不擅改其写面，不将此项算作通过或清理算法失败。原有irrefutable-pattern warning位于input_admission.rs2813，本轮未修改。

使用上述唯一编译生成的同一 unit binary `target/debug/deps/shell-34fc88a27b856cb5`，SHA-256 `7e76552a730e2f90cf74c92c81d2e0157f1b95d5a6babaa9e59046e1097541f3`，按顺序执行：

```sh
HOME=/tmp/grow-complex-exit-fixed-96t0c57f GROW_HOME=/tmp/grow-complex-exit-fixed-96t0c57f GROW_TEST_COMPLEX_EXIT_HOME=/tmp/grow-complex-exit-fixed-96t0c57f target/debug/deps/shell-34fc88a27b856cb5 --exact agent::mvp_agent::runtime_handoff::tests::complex_history_exit_through_acp --ignored --nocapture --test-threads=1

target/debug/deps/shell-34fc88a27b856cb5 session::image_describe::tests:: --skip user_image_asset_reconciliation --nocapture --test-threads=1

target/debug/deps/shell-34fc88a27b856cb5 agent::app::leader_skills_tests:: --nocapture --test-threads=1
```

| 检查 | 实际结果 | 原始日志 |
| --- | --- | --- |
| B原复杂history真实退出；task `01a1b4a4-e39d-7611-a579-a4dabee87530` | exit0，1 passed / 0 failed；测试77.89s；ACP **123ms suspended**；actual writers retired / source prefix hash / cold全部通过 | `call_945d25353af14a1eaa6845c3274acc07.log` |
| 原image cases（不重复已通过4 helper） | exit0，21 passed / 0 failed / 0 ignored；0.27s | `call_f4c8ae12e6b34018b86e9c418dc3750a.log` |
| leader4，默认TMPDIR，无/private/tmp环境覆盖；task `01a1b4a6-eac1-7271-aaa9-4ac47e00b4a6` | exit0，4 passed / 0 failed / 0 ignored；34.38s | `call_737a5cc693d04daf92dd680e33d055fd.log` |

父在此期间将Cargo窗口交PTY Service，明确要求不启动Shell integrations或重编；本轮未执行 `test_plugin_install_persistence` / `test_sdk_mcp_registration_owner`，交父下一batch，未运行需要fresh GROW_BINARY的leader真实集成。第一条Cargo退出后，全部命令只使用既有binary，未与父Cargo争用。

### B完整阶段与持久证明

原负载仍256 accepted tool exchanges×65,536 bytes结果，17实际IntegrityRepair / 31完整Compaction及31自生成summary Sideband。source305,653,152bytes / 2,494 domain events；生成8,131ms；旧copy＋read3,934ms，作为原绿控制，未宣称B单helper旧红。真实public load22,533ms在退出预算外，返回后立即走ACP原五秒入口。

| 阶段 | elapsed ms |
| --- | ---: |
| root initial_facts / frozen_facts | 40 / 46 |
| node sampler_settled / commands_settled | 0 / 4 |
| node draining_services | 13 |
| notification payload reconciler join | started与finished均0 |
| 全service joined | 1（从service退休起点） |
| node owners_drained / checkpoint_committed | 15 / 24 |
| root children_retired / latest_facts | 72 / 72 |
| tree confirmed / ACP suspended | 122 / 123 |
| post-exit cold full validation | 17,953（退出预算外） |

真实成功断言：`control_session_handle`已移除、`session_threads`已移除（实际线程join）；原305,653,152字节source prefix SHA-256保持；cold `load_light`完整验证成功，runtime phase Suspended、tree一个root，canonical新增到2,504events。未fake checkpoint/node/writer ACK，未调用force。与前轮同机制无采样B 5,002ms unconfirmed形成真实产品链红绿；seed IDs/timestamps与内容hash按新隔离fixture变化，历史负载形状相同。

### 成功source交父CLI里程碑

- HOME/GROW_HOME：`/tmp/grow-complex-exit-fixed-96t0c57f`。
- metadata：`/tmp/grow-complex-exit-fixed-96t0c57f/complex-exit-fixture.json`。
- sessionId：`01a1b4a4-e3c6-7272-b965-a03e50bb26e4`。
- cwd：`/private/tmp/grow-complex-exit-fixed-96t0c57f/workspace`。
- source prefix SHA-256：`54618f647d2c835c7e50dad4177c25d2eab690afbe0217a77a0f8e613e574465`。
- timeline：`/tmp/grow-complex-exit-fixed-96t0c57f/sessions/%2Fprivate%2Ftmp%2Fgrow-complex-exit-fixed-96t0c57f%2Fworkspace/01a1b4a4-e3c6-7272-b965-a03e50bb26e4/timeline.jsonl`。

该source已真实durably suspended、原writer退休且cold可验证，已立即交父用于fresh CLI同产物双Ctrl+D与恢复。原现场源session从未load/resume/writer接管；两个失败synthetic source与sample也保留。CLI最终里程碑、独立整体review及图片fixture修正后的重编8/8仍由父后续完成，不能把本轮ACP成功扩大为全部门禁通过。

本轮编译时源hash：input_admission.rs `eafe5448a29737732a514864096e3db20804f749647955a4b7d2662e8879fa0d`；image_describe.rs `09eed434a710395f608d006bf29d230a51cfd5254fceb2af5da4f1edff412c11`；ChatState handle.rs `52604317db500333918b0fa4493068de043f5788affe7886d234a88887f13b97` / actor/mod.rs `d4396e76665726f740c1849ec43660c39e8538558416421d76fc3599ea0ecac7`。这些源码属于两owner，本轮未写；其后owner修正测试需下一batch新binary再验。所有本项命令已实际结束，执行与源码scope明确释放父级，无提交或归档。
