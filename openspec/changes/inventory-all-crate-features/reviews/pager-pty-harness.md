# pager-pty-harness 逐包审计

状态：pending。当前只完整读取 Cargo.toml 和 src/env.rs；lib.rs 输出曾被截断，不登记为完整阅读。其余模块和测试/bench 尚待逐项读取，不以包清单的 source_files 数量代表全部交付文件。

## 已确认入口

Cargo 声明 pty-scenario、scroll-matrix 两个 binary，以及 pty_bench、paste_latency 两个 harness=false bench；包 publish=false，未声明 features。运行时能力仍需核对各入口代码，不能仅从清单注释认定。

## 二进制解析

已将 PTY harness binary resolution and implicit build 写入 test-harness-runtime delta。env.rs 共 94 行，完整读取；隐式 build 无 --locked/--offline 参数，当前仅检查目标存在，不进行新鲜度或执行权限校验。CARGO_TARGET_DIR 的相对路径不在 helper 中转绝对路径；构建 cwd 为工作区根。后续测试要显式控制构建目录与 PAGER_BINARY，防止误触发大规模构建。

本轮未构建、未启动 PTY，不能宣称任何 e2e 通过。

## timing.rs：128 行完整阅读

补入 PTY frame timing parser measurement boundary。VTE parser 跨 feed 保留状态；仅首个 mode 参数决定帧边界，_ignore 参数未参与判断。print 在帧外也累加，但下一个 begin 会清零；end 只消费起点，孤立 end 不输出记录。reset 不重建 VTE parser。该时间是读取/解析观察值，不能直接当作应用渲染或终端展示耗时。本轮未执行测试，下一步继续读取 screen、results 与 harness 数据投递链。

## results.rs 与 screen.rs 完整阅读

完整读取 results.rs 266 行（含 7 个单元测试）与 screen.rs 186 行（含 2 个单元测试）。补入聚合/百分位、基线持久化/比较、当前屏幕与历史查询、回复排队/排空 4 项契约。基线比较没有使用 FPS 或 jank 门槛；已有统计测试不覆盖文件写入和 compare_baseline。screen 的测试覆盖滚出文本和 CPR 回复可排空，不等于实际子进程收到了回复。本轮仅源码核对，未执行这些测试或编译。

## flows.rs：86 行完整阅读

补入三个共用交互契约，覆盖全部四个 helper。submit_turn 注释对重复 Enter 的安全性不能代替当前 pager 调用链验证，因此只规范 helper 实际重试和文本匹配行为。wait_for_labels_absent 丢弃超时错误；model 等待会主动新建会话且 update 不裁剪剩余预算。待检查上层用例是否另有断言弥补这些观察边界，暂不改变运行时代码。

## host_clipboard.rs：200 行完整阅读

覆盖文本读写、guard、roundtrip、PNG fixture 与平台图片写入。程序 wait/output/status 均未显式 timeout；macOS 路径直接插入 AppleScript 双引号字符串，含引号路径不能保证正确，后续单独处理。guard 仅恢复文本且无同步锁，roundtrip 本身会覆盖全局状态。本次没有执行任何宿主剪贴板命令或构建。

## leader.rs：237 行完整阅读

覆盖共享 socket 客户端启动、容错读取与完成等待。start 不直接启动 leader；客户端不存入 cluster，自身清理由 PtyHarness/PtyController 后续核对。扫描跨所有会话且无顺序保证；完成等待并不绑定本次 turn。两项单元测试仅验证 envelope payload 提取、坏尾行/空行过滤和 cancelled tag 匹配，不验证集群隔离或进程清理。本轮未启动集群或编译。

## pty.rs 分段阅读（一）

已读 1–230、485–672 行；尚未完整读取，不登记全文件 reviewed hash。补入环境层叠与 reader/drain 契约。spawn 使用 portable-pty，创建进程后附加 TestProcessTree；进程清理/状态恢复主体尚待 231–484 行，测试尚待 673–1094 行。无界通道与读取错误降为关闭是当前行为，不能从 drain 返回推导进程已退出。没有启动 PTY 或编译。

## pty.rs 分段阅读（二）

补读 231–484 行，生产实现现已全部阅读，测试 673–1094 行仍待检查。补入退出观察/状态缓存及 quit/Drop 清理契约。PendingStatus 已非运行中但无退出码，quit 可因此成功；drop 的轮询预算不是所有系统调用硬超时。尚不登记整文件 reviewed hash。

## pty.rs 分段阅读（三）：全文件读完

补读 673–1094 行，现 1094 行全部读取并登记 SHA-256。共有 14 个测试函数：

- `exit_poll_distinguishes_pending_running_and_errors`
- `wait_deadline_preserves_pending_running_and_errors`
- `quit_completion_accepts_non_running_states_and_propagates_errors`
- `observed_exit_echild_is_typed_only_after_portable_reap_capability`
- `consumed_status_recovery_requires_a_cached_status`
- `observed_exit_then_echild_recovers_cached_status_once`
- `pty_waits_are_idempotent_and_pid_is_hidden_after_reap`
- `pty_drop_tree_cleanup_is_bounded_and_reaps_grandchild`
- `pty_tree_diagnostics_surface_enrollment_state`
- `apply_child_env_strips_all_host_terminal_markers`
- `apply_child_env_uses_sandbox_baseline`
- `apply_child_env_remove_deletes_sandbox_credential`
- `inherited_env_projection_is_set_only_and_preserves_unrelated_ambient_vars`
- `apply_child_env_caller_env_overrides_survive_strips`

状态分类、deadline、quit、ECHILD 与 consumed status 主要用纯函数验证；Unix 实际 /bin/sh fixture 验证 exit 7 缓存和 PID 隐藏、portable kill 后状态恢复、Drop 后 sleep 孙进程退出。Drop 测试检查自身小于 1 秒且 3 秒内孙进程不存活，不证明所有平台/所有后代模式都可清理。环境测试检查 CommandBuilder 投影，不启动对应真实 pager；无测试自动比较 HOST_TERMINAL_ENV_VARS 与 detector 所读变量全集。

本次只审阅测试源码，未执行测试；未由静态阅读声明通过。还需读完高层 lib.rs，确认回复转发、等待和输出保存如何使用底层状态。

## lib.rs 分段阅读（一）

本次完整读取 110–423 行；此前 1–109 已显示但全文件输出有截断，归档前仍按段核对剩余部分。content spawn 各便利入口最终使用 content.sandbox()，Set-only 接口转 EnvOp，extra_args 原样传入，不自行追加会话参数。补入输出分发、查询回复、屏幕直注入与 resize 顺序契约。转发开关关闭时回复未排空，feed_screen 不主动转发回复；后续 PTY chunk 才触发开启情况下的 drain。lib.rs 尚未读完，不登记完整 hash。

## lib.rs 分段阅读（二）

读完 423–650 行，补入 condition/stability/idle 与 asciinema 输出契约。full_text 等待包装在错误后附上完整历史；Kitty 等待调用 scripted::count_kitty_graphics，具体过滤条件待 scripted.rs 核对，其循环用 update 不直接将 EOF 当错误。关闭诊断固定标注非运行中，不代表实际 liveness 查询。待补 1–109 与 651–756 行，不登记完整文件 hash。无编译或终端启动。

## lib.rs 分段阅读（三）：全文件读完

补核 1–109 与 651–756 行，现 756 行完整阅读并登记 SHA-256。追加退出与尾部排空契约；PendingStatus 在 wait_exit_code 可立即返回，但 wait_for_exit_and_drain 必须等到真实 code。后段独立预算和 200ms quiet 都可在 EOF 前成功返回，不能把方法名理解为完整无损排空。reset_timing 不重置录制数据。公开 re-export 在对应模块核对；Kitty 计数仍待 scripted 实现确认。未执行测试或编译。

## content.rs 分段阅读（一）

读取 1–205 行，覆盖双 endpoint turn handle、controller 默认启动与显式配置种子。补入两项契约；unsatisfied_diagnostics 的实现筛选 unsatisfied，不等于注释所称未 claim。server 的转发方法 set_response/enqueue_response/set_chunk_delay 按底层 mock 实现解释，不能由包装注释推导更强流顺序。剩余 206–528 行待读，不登记全文件 hash。无 mock server 启动或编译。

## content.rs 分段阅读（二）：全文件读完

补读 206–528 行，现完整 528 行并登记 SHA-256。逻辑 turn 注册使用 foreground endpoint matcher，text 是输出不是请求匹配条件；可通过底层自定义 matcher 限定请求。五个 tokio 测试覆盖默认 settings 200/{}、固定默认文本、两个 endpoint 任一满足、终止屏障 release、未使用 expectation 诊断和 panic；请求由 reqwest 直接发送，未启动真实 pager，不能推导 pager 集成通过。本轮未运行这些测试。

## 场景注册表与 idle_cost 完整阅读

完整读取 scenarios/mod.rs 和 idle_cost.rs 并登记哈希。六个 benchmark 与两个独立 regression 模块分开识别；idle_cost 实际是欢迎页帧计时，不是注释所称 CPU 测量，也未提交内容或断言零帧。将实际行为写入两项契约，未运行场景。

## scroll_stress 与 resize_storm 完整阅读

登记两个场景源码哈希及行为契约。scroll_stress 的就绪代理是 Lorem+500ms，不证明预渲染完整；resize_storm 在欢迎页面运行，没有注入响应，与注释暗示的每个 entry wrap 压力存在覆盖差别。所有窗口包含实际调用耗时，输入节奏不是外部精确调度。未执行基准，不宣称性能结论。

## 其余三个 benchmark 完整阅读

完整读取 streaming_render、mixed_interaction、large_codeblock 并登记哈希及契约，六个 Scenario 实现均已读取。streaming/mixed 本身未调用 set_chunk_delay，不保证测量与流式重叠；所有 j 压力函数未验证焦点或位移。大代码块函数每行可预测但没有样式或换行断言。这里记录基准工作负载而非声称对应产品行为已端到端验证；bench runner 是否提供额外约束待后续检查。未运行基准或编译。

## benches/pty_bench.rs 完整阅读

完整读取并登记哈希。runner 为每个场景独立建 mock/sandbox、显式 seed config，但没有附加流式 pacing，确认前述 streaming/mixed 覆盖限制仍成立。错误场景输出零值但阻止基线更新，准备错误与场景错误分开处理；成功零帧仍可写为基线，是否有效需独立判断。未执行 bench 或构建。

## empty_enter_send_now.rs 完整阅读

已登记完整源码哈希和场景契约。使用终止屏障保证注入前首轮尚未结束，但空 Enter 后随即 release，需实际运行才能确认取消/完成竞争是否被断言捕获。请求断言只找首个包含 follow-up 的 user blob，没有 session/request 身份或恰好一次断言。与 bench runner 不同，此 helper 没有 seed_llm_config；后续检查测试入口及启动 gate 时复核是否能达到目标路径。未运行，不能作为 send-now 已验证证明。

## empty_enter_send_now 测试入口与配置前置追踪

完整读取 tests/empty_enter_send_now.rs 并登记哈希：tokio multi_thread/2 workers，#[ignore]，仅调用场景函数，没有模型配置补充。继续核对 test-support/src/sandbox.rs 的 build/apply_mock_url：创建目录并设置 mock URL/API key 环境，不写 config.toml。pager/src/app/mod.rs 的 ensure_llm_configured 在校验失败时输出配置引导，TTY 下等待 Enter。由此确认已检查的入口链没有补 seed，存在达不到欢迎页的前置风险；尚未运行实际 binary，且整体配置读取/环境覆盖链未完整复核，不能宣称该测试必然失败。后续用受控 binary 验证并单独修复，不改变本次纯文档范围。

## plan_approval_resume.rs 完整阅读

完整读取并登记哈希，补入恢复驱动与 Timeline fixture 校验契约。场景主动 seed LLM，与 empty-enter helper 不同；计划状态由测试写盘，不能据此证明真实 Plan 生成/停放链完整。artifact 先写后追加事件，无跨目录原子事务；读取 seq/version 严格，但 revision 仅取最后 control 再饱和增加。quit 返回语义沿用底层，不扩大成所有后代清理与状态获取的独立证明。未运行场景。

## 测试入口与基线开发说明核对

完整读取 tests/plan_approval_resume.rs、tests/env_op_compile.rs 和 benches/pty_baselines/README.md。计划恢复测试为非 ignored 的 tokio/2-worker 测试，普通集成测试会进入 pager_binary，缺少二进制时可能自动构建 cli；empty-enter 为 ignored，两者运行条件不同。EnvOp 测试只检查四种构造映射到 Set/Remove，不检验非 UTF-8 环境真实子进程传递。

基线 README 原命令 cargo run -p pager --release --bin pty-bench 与当前 Cargo.toml 不符；已做纯文档修正为 cargo bench -p pager-pty-harness --bench pty_bench，保留参数与输出路径。README 关于 CI 首次 seed 的描述尚待工作流核对，不视为已验证事实。登记修正后的文件哈希；未运行构建或基准。

## 基线 CI 声明复核

搜索当前 .github 全目录未发现 pager-bench、pty_bench、pty-bench、write-baseline 或 pty_baselines 引用。读取 core-regression.yml 确认其运行指定 --lib crate 测试并构建 cli，不包含本基准。基线 README 原称 CI 首次自动 seed 与现有工作流不符，已改为手动生成、显式传入平台文件；没有新增 CI job 或性能门槛。结论限定当前检入的 GitHub workflows，不推断仓库外 CI。更新文档哈希，未编译。

## prompt_history_durable_quit.rs 完整阅读

完整读取并登记哈希。两个 ignored 测试，SIGINT 仅 Unix；双 Ctrl-C 包含 --continue/Up，SIGINT 仅退出后磁盘检查。submit_and_settle 使用 ACK+固定 1 秒而非稳定 idle，且同样没有 seed_llm_config，运行前需验证配置前置。collect_named_files 对目录读取错误传播，不跟随 symlink 目录；Timeline 错误 JSON 行跳过，文件读取错误传播。show-cursor 子串与 typed turn 存在分别是有限断言，不证明全终端恢复或全部接纳竞态。未执行场景或编译。

## 滚动矩阵注册入口完整阅读

完整读取 tests/scroll_matrix_curated.rs 与 src/scroll_matrix/mod.rs，登记哈希。八个 async 用例加一个清单一致性测试；binary 解析位于 mutex 之前，不能把该锁解释成覆盖自动 cargo build。mod.rs 是模块导出/架构注释，字段严格性和 invariant 内容须继续核对 log/cells/runner 等实现，尚不据注释认定全部条件成立。未运行矩阵或编译。

## scroll_matrix/report.rs 完整阅读

完整读取并登记哈希；四个测试覆盖退出策略、JSON 可选字段/状态拼写、落盘与 ASCII 样例对齐。实现没有通用 ANSI 清理，测试样例不含 escape，因此不能从测试名或注释认定任意输入都是安全 ASCII。表格列宽用 bytes、格式化宽度用字符，非 ASCII 对齐无保证；exit_code 信任 cell.status，聚合规则待 runner。未执行测试或编译。

## scroll_matrix/log.rs 分段阅读（一）

已读 1–290 行，覆盖全部生产解析/分组/等待函数与测试开头。补入两项契约，强调原始 finalize 子串计数不等于完整 JSON 可读，且未知字段允许但未知 evt 仅在分组时拒绝。schema 必填字段已按当前 struct 读取，生产者对应字段全集仍需跨文件核对。测试余段未读，不登记完整 hash；未运行解析测试或编译。

## scroll_matrix/log.rs 分段阅读（二）：全文件读完

补读 291–502 行，现完整 502 行并登记 SHA-256。八个测试覆盖 producer-shaped fixture、未知字段、carry 缺失报行号、方向切换/尾流、孤立记录/双 start、跨流首 spacing 排除，以及文件分阶段/并发 append 等待。等待测试第二条 finalize 只有 ts_ms/evt，仍满足计数，直接证明等待不验证完整 schema；未覆盖 UTF-8 坏尾或包含标记但 JSON 未完成的具体案例。

交叉读取 pager/src/input/scroll_log.rs 的 ScrollLogConfigEcho 与 ScrollLogRecord：当前必填键与 consumer 非 Option 字段一致，三项可选 bookkeeping 和七项 flattened config 对应。类型并非逐字相同：producer usize→consumer u64，i32→i64，avg_interval f32→f64，evt/trigger enum→String；因此 consumer 类型更宽，字段一致不代表所有非法值都会被拒绝。没有将生产者整文件登记为本轮完整阅读，也未运行测试。

## scroll_matrix/runner.rs 分段阅读（一）

读取 1–190 行，核对 run_cell 的 timeout/错误转报告与 inner 前半。run_cell 同步创建目录后才启动 spawn_blocking，并用 60 秒 timeout 等待；目录失败、join panic、body 错误或超时生成 Fail、空 invariants、streams=0 和 note。超时不取消 blocking 任务，因此返回失败报告不代表进程/日志清理完成。同步目录操作和 runtime 前置不受该 timeout 保证，注释 never hangs/never panics 不能作无条件契约。

inner 已见删除旧 capture、cell env 后追加 GROW_SCROLL_LOG、固定 400 marker、按预延时回放 SGR、5 秒 finalize 等待与 300ms drain。其余判定和测试未读，不登记全文件哈希。本轮 delta 生成脚本语法失败，未产生该项需求；本节保留已核对事实，后续读完并补入规范。未执行矩阵或编译。

## runner.rs 分段阅读（二）

补读 190–280 行，成功补入上轮未生成的 timeout 契约及 quiet/teardown 判定流程。quiet 取样在 streaming release 之前，结果只覆盖该 500ms 窗口；两段 completion 等待分别有 30 秒预算。quit 的底层 PendingStatus 成功语义仍适用，不能单凭此处注释证明所有 recorder 完整关闭。剩余 check_screen/classify 与测试待读。未运行矩阵。

## runner.rs 分段阅读（三）

读取 280–398 行，补入 I-SCREEN 模拟和 classify 真值表。顶部仅最终 clamp，与逐步顶部限制不同，当前测试仅覆盖单次超顶，不能外推反向多 stream 行为。classify 不验证 xfail 为 outcomes 子集，该约束需另查 cells 测试。测试余段待读，未登记完整 hash，未执行测试。

## runner.rs 分段阅读（四）：全文件读完

补读 399–462 行，完整 462 行并登记哈希。六个测试覆盖 SGR 坐标、底部/单次顶部限制、quiet 阈值、屏幕无 marker/streaming 拒绝、分类优先级、tier 标签。没有在本文件直接执行 run_cell 的超时、panic、目录失败或 teardown 失败路径；check_screen 测试 groups 为空，不证明真实记录回放和 marker 位移一致。仍按源码证据记录这些实现，不能用六个纯判定测试替代进程级验证。本轮未运行测试、未编译。

## gestures.rs 分段阅读（一）

读取 1–290 行，生产表和分发均已读，测试余段待读。G1=1/3；G2/G8=5个刻度、50ms；G3=60密集；G4=66事件，头8/8ms，尾40/44/50/55/60/70ms；G5=20事件，交替4/60ms；G6=10上10下、8ms；G7=10下3上、8ms；G9a=8单事件55ms，G9b=8×3/55ms；G10=12事件40ms；G11=两刻度120ms。预设流数量不保证宿主调度后实际一致。新增分发边界契约，尚未完整登记哈希或运行测试。

## gestures.rs 分段阅读（二）：全文件读完

补读 291–371 行，完整 371 行并登记哈希。四个测试核对数量/方向、刻度间隔、jerk 头尾形状，以及全部 12 手势在 ept=1/3 的预设流计数；测试使用本文件镜像阈值，不自动证明与生产 mouse 常量同步。没有执行 PTY 时序测量，宿主 sleep 下限与子进程实际接收间隔不是同一证据。G5 实际头间隔为 4+60=64ms，注释说 heads 60ms apart 是简化；保留精确表值作审计事实。本轮无编译。

## cells.rs 完整阅读

完整读取 403 行并登记 SHA-256，核对全部 25 行及六个单元测试。8 curated +17 full 是代表性子集，不是组合穷举；TMUX 标记与 G9 事件不等于真实 tmux。当前全部 xfail 为空，历史 jerk_xfail 名称不改变断言。xfail 子集测试仅特别断言 jerk 为空，不能按注释扩大成全表未来永远无 xfail 的测试保证。expected_profiles_agree_with_env 使用本地重写规则，未链接生产配置实现。新增两项规范；本轮未执行 Rust 测试、未编译。

## session.rs 完整阅读；invariants.rs 生产判定阅读

session.rs 完整327行，登记哈希；三个测试只覆盖 marker 字符串、屏幕文本解析和声明延迟预算，没有实际执行会话。marker_line 是至少四位，解析仅四字节；count=0不支持 settled/streaming末项等待。两种 settled session 同一构造，baseline 在 Tab 前取；streaming guard 只查可见 STREAMDONE，不是持续传输证明。

invariants.rs 已读1–510行，包含全部生产函数及测试构造开头，余下测试待审，不登记完整哈希。补入路由/缺失 finalize、cadence/carry/accel容差、pricing/coast和config echo规范。生产者时钟不自动证明高负载下所有测试不失败；判定只覆盖实际记录和可选字段。未执行 Rust 测试或编译。

## invariants.rs 测试与两个 CLI 完整阅读

补读 invariants.rs 511–800，全文件完成并登记哈希。15 个测试使用人工 JSONL 经解析/分组后判定；canonical 覆盖11类日志判定，forced-wheel独立，mutation验证倒序/cap/accounting/cadence/定价/accel/carry/config/mux，jerk历史坏形状验证两项失败，另有coast边界和routing。测试名 min_lines_substitution 的样例原始定价为1，没有真正覆盖截断为0的替代分支；没有把该名称当成分支覆盖证明。历史 xfail 注释不改变当前 cells 空 xfail。

完整读取 scroll_matrix.rs 和 pty_scenario.rs 两个 bin，登记哈希。新增命令选择、并发/排序和脚本状态退出映射3项规范。矩阵超时 body 继续执行意味着释放 permit 后底层工作可能重叠；脚本配置及产物行为仍须读 scripted.rs。两个 CLI 本文件均无测试，本轮只核对源码，没有启动程序、编译或运行测试。

## scripted.rs 分段阅读（一）

已读1–770行，覆盖场景反序列化、动作/locator/dimension定义、runner生命周期、workspace materialization及run_step开头；全文件尚未完成，不登记哈希。新增4项边界契约。runner不seed_llm_config，仅写用户config_toml；不能假定任意无模型配置场景可直接启动交互。未设置workspace时真正cwd须以下层spawn实现为准，未沿用注释“inherits test cwd”作为本轮结论。skip前prepare fixtures，step失败capture/raw为best-effort，quit错误不影响Passed，这些都是当前源码事实。其余动作实现、helper/default值与测试待审；本轮无运行、无编译。

## scripted.rs 分段阅读（二）

续读770–1800，run_step、截图/SVG、报告结构/bugs输出、fixture生成与payload、图片/请求/临时文件断言、定位和背景均已读；OSC52解码刚进入仍待后续。新增8项规范。PasteClipboardImage只是路径paste；AssertInlineImages实际into_dimensions不是完整像素decode。SVG画布取最长run而非行总长；fixture命名没有workspace同等路径拒绝。这些限制按当前实现记录，未混入修复。尚未完整读取文件，不登记hash；没有执行场景或编译。

## scripted.rs 分段阅读（三）：完整阅读完成

续读1801至文件结尾，现完整2365行并登记SHA-256。20个测试覆盖schema/default部分字段、safe_name单例、workspace路径拒绝与真实git init、fixture尺寸/CRC、尺寸范围、请求包含/索引、tmp扫描及Kitty计数。CRC测试调用load_from_memory，与AssertInlineImages的into_dimensions不同，不能移用为后者完整解码保证。未看到本文件直接覆盖runner失败落盘、真实鼠标交互或OSC52坏尾的测试。新增协议扫描/指针编码/默认值4项规范；本轮仅静态审阅，无编译或运行。

## paste_latency.rs 完整阅读

完整488行并登记哈希，新增3项契约。基准是真实macOS剪贴板，不同于scripted PasteClipboardImage路径模拟；此轮没有运行以免无必要改动用户剪贴板。计时排除pbcopy/预置PNG与清理，包含PTY注入和屏幕处理，chip时间在burst等待后读取。iterations=0仍有结果且全零；dashboard失败可被丢弃并成功退出。spawn_ready不是durable idle证明，清理仅看子串。无本文件单元测试，未编译。

## scroll_correctness_ptyctl.rs 与文件遍历闭合

完整读取162行，登记哈希。一个非ignored PTY测试，使用seed_llm_config；上/下混合Page键及wheel，断言区间存在和屏幕变化，不作精确位移/独立wheel效果/性能界限证明。未执行该测试。

重新枚举crate目录全部文件：40个，全部已有完整审阅登记；逐一重新计算SHA-256，无漂移、无漏登。当前79项功能契约。这里只完成文件遍历闭合，尚未完成全项语义对照和本crate必要运行验证，status仍为pending，不勾选总任务。
Luna/high子代理完整读取pager-pty-harness的Cargo.toml、38个Rust文件和bench README，共40个文件、11481行Rust；79条既有test-harness-runtime delta与178个精确来源符号保持一致，全部文件SHA-256已复核。未运行Cargo、PTY、bench或外部服务。
