# 图片相关用户旅程与触发路径

当前不能给出“所有旅程都能完成”的结论。图片消费和主恢复链路已有实测，但自动 Memory Flush 的入口存在门禁缺口；未转换图片的只读辅助请求也有明确失败边界。

调查基于 2026-11-03 当前工作区，包含此前三个图片修复和其他并行修改。本轮未修改业务代码或测试。范围是本 chat 涉及的用户动作及其生产分支，不是整个 Grow 的全功能审计或任意调度的模型检验。

## 状态与判定

图片状态：`N` 无图片、`D` 全部已有确认描述、`P` 主转换/投影 ACK 待完成、`U` 未描述、`M` 已描述与未描述混合、`X` 已确认移除 unresolved 图片。路由状态：`K` 精确 transport 已确认拒图、`O` 未标记。实际视觉模型与未知纯文本模型都走 `O`，不能按产品名猜能力。

证据：**T** 为本轮运行的相关自动测试；**C** 为实际生产链的源码核对；**G** 为仍需完整入口/界面/进程验证的缺口。T 不代表整条 UI 链已经端到端执行。下表将共享同一谓词和终态的组合合并，按当前入口和影响控制流的状态类别穷举，共 50 条。

## 入口链

| 入口 | 实际链路 | 门禁与差异 |
| --- | --- | --- |
| 图片输入/工具产图后的主请求 | ACP prompt 或工具结果 → turn/mod → known-text 预投影 → request builder → sampler → handle_sampling_failure | 未标记先发原图；明确拒图才记录能力、转换并重建；不是所有 400 都恢复 |
| `/btw` | Pager slash/router/notes → effect `grow/btw` → feedback extension → SideQuestion command → run_loop → handle_side_question | 可在主任务忙时执行；extension 直接查 sessions，不等待并行 load；receiver 关闭丢弃工作 |
| 父子/peer 问答 | ask_parent/ask_subagent/ask_session → runtime 关系/身份校验 → RunCoordinationInquiry → target FIFO → handle_coordination_inquiry | 冻结接收方历史；peer 跨 cwd 需批准；忙碌本身不拒绝；只读无工具 |
| 手动 recap | Pager `/recap` → notes/effect → `grow/recap` → recap extension → Recap command → run_loop → handle_recap | feature/capability/session/真实 user turn；extension 等待 load，队列关闭明确报错 |
| 自动 recap | UI focus/away polling → active-session eligibility → 同一 extension/handler → 当前 away-period 通知筛选 | UI 默认阈值 30s，不等于服务端一定生成；服务端还要求至少 3 个真实 user turns、新 turn、距上次 API 请求 3min 等 |
| `/flush` | Shell 动态命令广告 → Pager host-command effect → `grow/commands/execute` → slash executor → BuiltinAction::FlushMemory | Memory/storage 与工具注册控制可用性；主 turn 忙时 host-command executor 拒绝；stale 命令再检查 memory |
| ACP 手动 flush | `grow/memory/flush` → memory extension → FlushMemory command → run_loop → run_memory_flush(None) | 不经过 `/flush` 的 busy gate；直接 sessions lookup，不等 load；memory off 返回错误 |
| idle flush | configured Sleep → run_loop timer gate → 比较 conversation_len 水位 → activity → run_memory_flush(None) | 默认 timeout=None，不自动触发；没有 foreground-idle 判断；timer 与新历史长度共同决定 |
| 压缩前 flush | 手动 compact / background prepare / overflow recovery → maybe_pre_compaction_flush → should_flush → snapshot → detached run_memory_flush(Some(snapshot)) | threshold、初始配置、cycle、activity；不等待 flush 写入完成；旧 snapshot 不重抓 |
| 会话结束保存 | runtime shutdown → on_session_end → 本地 metadata summary | 有独立 user-count/query-bytes/save_on_end 门禁，不调用模型，不是本次修复的 LLM flush |

主要定位：`shell/src/session/actor/run_loop.rs:988,2103,2727,2737,2761`；`actor/compaction.rs:550,680,2362,3170`；`actor/slash_exec.rs:523,724,1430`；`extensions/{feedback,memory,recap}.rs`；`pager/src/app/root/{dispatch/notes,effects/mod,event_loop}.rs`。

## 50 条旅程

| ID | 用户动作/状态 | 实际结果与有效性 | 证据 |
| --- | --- | --- | --- |
| J01 | 无图文本继续、任意已准备路由 | 常规文本请求；不会由历史图像触发 content.type 拒绝 | C/T |
| J02 | 新用户图片，视觉路由 O | 已接纳 ACP 图片进入主请求，能力未被预先猜测；本轮未驱动各平台剪贴板/拖拽 UI | C/T actor；UI G |
| J03 | read_file 等工具产生图片，路由 O | 完整工具交换携图进入主请求 | C/T |
| J04 | 历史图片后由视觉模型切到纯文本 O，再触发主请求 | 一次明确拒绝 → 视觉辅助/OCR → durable ACK → 重建文本请求 | T |
| J05 | 已知纯文本 K，又收到新用户/工具图片 | 主请求前主动生成/复用文字投影，避免重复同类 400 | C/T |
| J06 | 视觉辅助可用并成功 | 只发送图片组和固定 prompt，原图证据保留 | T |
| J07 | 视觉辅助缺失/失败，OCR 成功 | 使用 OCR 描述后重建 | T |
| J08 | 两种描述方式都失败 | 主 owner 确认移除当前 Surface 图片并使用 canonical 文字；原始 Timeline 保留 | T |
| J09 | 图片投影 durable write/ACK 失败 | 主请求失败，不发送未确认的有损重试 | T |
| J10 | 转换或 OCR 中按取消/停止 | 主生产者停止后续组、OCR 子进程和主请求，不把取消当普通失败继续降级 | T |
| J11 | 已描述 D、返回已标记纯文本 K | 主请求选择文字，不重新调用辅助模型 | T |
| J12 | D 切回另一未标记视觉路由 O | 主请求允许保留原图；只读辅助请求仍优先选已有描述 | C/T |
| J13 | 已移除 X 后换模型 | 当前 Surface 不复活被移除图片；恢复原媒体需显式新输入/rewind | C/T |
| J14 | 不同 alias 指向相同 model/backend/endpoint/query | 共享精确 transport 的拒图记录 | C；相关身份测试既有 |
| J15 | alias 相同，但 endpoint/backend/model/query 改变 | 新 transport 独立，不能继承旧拒绝 | C/T |
| J16 | D 后 `/btw` | 等待已开始投影，冻结后以文字单次回答，overload 重试保持同一来源 | T；UI/ACP 全链 G |
| J17 | D 后 child ask_parent | parent 冻结其自身历史回答；child 模型是否视觉不决定 parent wire | C/T |
| J18 | D 后 parent ask_subagent | child 自身冻结历史回答；只允许直接持有的活跃 child | C/T |
| J19 | D 后 peer ask_session | 同一图片分支；跨 cwd 批准、无 UI 拒绝，不越过授权 | C；相关协调测试既有 |
| J20 | D 后手动 `/recap` | 手动 recap 有描述，无原图，display-only，不提供工具 | T；UI 连续操作 G |
| J21 | D 后离开并返回触发自动 recap | 仅在 UI + 服务端全部 gate 满足时生成，旧 away-period 结果不展示 | C/T 门禁；图片+真实 focus G |
| J22 | D 后空闲时 `/flush` | 经过动态广告和 host executor，最终使用描述并可能写记忆 | C/T actor；slash→文件 G |
| J23 | D 后 ACP 手动 flush，主 turn 忙 | 可发起独立 Sideband；不是 `/flush` busy 拒绝语义 | C/T actor；ACP busy 全链 G |
| J24 | D，配置正 idle timeout，历史长度增长 | timer 到时触发 flush；不是严格“用户已空闲”的证明 | C；定时器全链 G |
| J25 | D，手动 compact 达 preflush 阈值 | 捕获旧来源、异步 flush；不阻塞完整压缩等待写完 | C/T snapshot；调用点 G |
| J26 | D，background compact 达 preflush 阈值 | 同一 preflush hook 与旧来源规则 | C；调用点 G |
| J27 | D，provider overflow 启动压缩 | 同一 preflush hook；压缩失败保持既有主请求终态边界 | C；组合旅程 G |
| J28 | P 时进入问答/recap/flush capture | 等待主转换及 ACK；成功后再按 D/U/M/X 处理，不重复启动转换 | T |
| J29 | U/M + K，最终请求仍需原图 | 辅助请求在 HTTP 前明确失败；flush 不写记忆；主恢复 owner 与辅助职责不同 | T |
| J30 | U/M + O，在任何主转换前先旁路询问 | 仍发送 unresolved 图片，纯文本 provider 可返回同款 400；不保证成功 | T |
| J31 | J30 失败后反复重问/重做 recap/flush，主历史未转换 | 可重复 400：辅助不记录负能力，也不拥有转换；不是“必然只失败一次” | C，J30 的状态保持断言 T |
| J32 | 视觉主模型 + 纯文本 flush_model，主模型从不拒图 | 若一直没有描述，每轮 flush 均可能拒图，直到其他主 owner 生成文字；该组合未全程闭合 | C/T override 分支；完整配置 G |
| J33 | M + K，但 recap/flush 裁剪后不再发送 U 部分 | 可发出合法保留窗口；未发送旧图不阻止消费 | T |
| J34 | 两个图片组不同描述/移除结果 | 主投影原子提交，辅助只使用提交后的各组结果 | T 主投影；混合多组辅助全链 G |
| J35 | 先捕获 D snapshot，再有新输入/压缩替换 | flush 继续使用旧 input range/revision/IDs，不把新历史拼入；真实文件 reload 有效 | T 新输入推进；真实 compact 组合 G |
| J36 | 先捕获 U snapshot，后 live Surface 才有描述 | 旧 snapshot 不重抓/不自动套后来的投影；可能继续 J29/J30 | C；独立交错测试 G |
| J37 | 后台压缩未标记路由拒图，尚未提升 | 失败结果可被丢弃，后续同步路径负责恢复；后台失败不等于立刻发生转换 | C |
| J38 | 后台压缩提升后拒图，原 transport 未变 | 前台进行受控图片恢复，再开启新的压缩事务 | C；完整提升+图像旅程 G |
| J39 | 后台压缩期间换 transport | 旧拒绝不得标记或恢复新 transport，失效后不错误发布 | T |
| J40 | 图片描述完成但 projection 未提交时重启 | 新 actor 复用完成 Sideband 描述并提交投影，不重复调用视觉模型 | T 真实文件/新 actor |
| J41 | 已投影 D/OCR 后重启，当前辅助路由不可用 | 按有效来源复用描述，与新辅助配置可用性分开 | T |
| J42 | 历史描述 prompt/provenance 不符 | 不把 legacy/future/错来源结果当可复用描述，必要时重新描述或按失败规则结束 | T |
| J43 | fork/rewind 后调用辅助或主模型 | 复用各入口规则，新的 provenance/native/image-budget 域独立；图片组合尚无本轮 fork/rewind 全链证明 | C/G |
| J44 | 关闭 `/btw` 接收端、关闭/挂起会话 | receiver 关闭终止对应工作；session cancellation 释放等待，无新 provider；普通主 turn 完成不擅自结束 inquiry | T/C；真实 UI close 组合 G |
| J45 | 初始启用记忆 → `/memory off` → 到压缩阈值 | preflush 仍可沿初始标志发模型请求，写入时才发现无 storage；确认的入口门禁缺口 | C，见 F1 |
| J46 | idle flush 失败/锁冲突，历史长度不再增加 | watermark 已推进，后续 timer 不自动重试该历史；尽力策略并非保存成功 | C，见 F2 |
| J47 | reconnect/load 中触发 recap/btw/ACP flush | auto recap 在 UI 被抑制，recap extension 等 load；btw/flush 直接查 map，可报 session not found，需要重试 | C/T recap 接纳；其他全链 G |
| J48 | 结束会话/看到 flush 完成回执 | shutdown metadata 保存不调用 LLM；flush handler 成功/执行过不等于已写文件，NO_REPLY、重复、provider/write 错误须看 result/path | C |
| J49 | headless/旧客户端把 `/flush` 作为普通 prompt 提交 | 经 input admission/排队，开始该 host turn 后 resolve BuiltinAction，再进入同一 flush 执行器；不同于忙时直接发 grow/commands/execute | C；完整 prompt→写文件 G |
| J50 | 非 Pager 客户端仅把 `/btw` 或 `/recap` 当文本 prompt | 它们是 Pager 保留命令而非相同的 Shell builtin；不能把文本提交当作已调用 SideQuestion/Recap，应调用对应 grow extension | C；接口边界，不是 helper 修复失效 |

## 发现与边界

### F1：关闭记忆后，压缩前 gate 没有跟随 runtime 状态

可达动作链是：最初启用 memory（`spawn.rs:1442` 固定 `compaction.memory_flush_enabled`）→ `/memory off`（`slash_exec.rs:1466-1485` 移除工具、backend 和 storage）→ 满足压缩 preflush 阈值（`compaction.rs:550-598`）→ `run_memory_flush(Some(snapshot))`。preflush 没有检查 `memory.is_enabled()`，执行器本身也不检查，正常 budget/capability/活动准入都通过时仍会向模型发请求，最后无法写入。

这是 P2 门禁缺口，不是源码中任意两个不同布尔变量就推断出的竞态。启动时全局 `memory.enabled=false` 经 `agent_ops.rs:178` 被转成 None，不能把该启动情况当作同一已证明旅程。`/memory off` 后的路径则实际保留初始 preflush 标志。后续应在已有 runtime admission 所有者统一确认 memory 状态，并用真正 toggle→compact 回归验证；本轮未修复。

### F2：idle 是周期计时加长度水位，失败不重试同一历史

`run_loop.rs:963` 创建 Sleep；`:988-1025` 到时只检查 timeout、shutdown/suspend、memory 与 flush lock，再用 `current_len > last_len` 判断新历史。仅该分支重设 timer，没有随每次用户活动重置，也没有 foreground-idle gate。配置字段 `config-types/src/memory.rs:315-318` 的“无用户消息达到时长”说明与实际计时条件不同；规范尚无完整 idle flush 时序契约，不能把注释单独升级为已归档行为要求。

watermark 在 spawn 前的 `run_loop.rs:1001` 就推进。provider 失败、图片未就绪或执行阶段发现 flush lock 冲突，都不会撤销；下一次相同长度不会重试。compaction 会同步重设长度水位（`compaction.rs:2057` 及 ConversationReset handler），因此“压缩后旧长度永远挡住新 flush”的猜测不成立。真正的缺口是失败/冲突时的新历史资格如何消费，以及“idle”是否应要求空闲，需要独立契约和 timer/command-loop 测试。

`idle_timeout_secs=0` 当前可直接映射 Duration::ZERO，可能造成高频 timer 唤醒；本轮没有运行零间隔负载或证明具体 CPU/饥饿结果，单列为配置边界待验证，不作资源消耗量结论。

### F3：只读辅助的 unresolved 图片旅程仍是明确失败边界

`freeze_sideband_surface` 只选择已有描述，不生成描述。`validate_sideband_image_input` 只会拒绝已知 K 的 unresolved 图片；O 仍携图。`/btw`、协调、recap、flush 都不会因为图片拒绝而写负能力或修改主 Surface。

所以 J30/J31/J32 是当前契约允许的失败，不能因为请求重试或后台函数存在就标为闭环。尤其不能把上一轮“复用已有描述已修复”解读成“切模型后先做任何辅助调用都会自动恢复”。若产品要求这些旅程也成功，需要明确沿现有主图片 owner 的准备/恢复时机，避免为每个辅助入口各造一个转换 owner。

### F4：入口语义和测试覆盖不能合并

`/flush` 忙时经 host executor 拒绝，ACP flush 可在忙时运行；前者由命令广告约束 memory/tools，后者直接检查 runtime storage。`grow/memory/flush` 最终丢弃执行 bool 并返回 Empty；bool=true 本来就代表执行过，不代表写入成功。MemoryFlushCompleted 的 result/path 才描述结果。这些是实际差异，是否统一需要独立决策，不直接判定为图片实现回归。

recap extension 的等待 load 与闭队列接纳已有专门测试，不应笼统说 ACP 层完全无覆盖。btw/flush、idle timer、三个 precompaction 调用点、真实 focus+图片、fork/rewind+图片仍缺对应完整链路证明；已有 helper/actor/原生片段测试不能顶替这些组合。

## 本轮验证

- `cargo test --locked -p shell --lib image_input_recovery_tests -- --test-threads=2` 完成当前库构建后，默认 test thread 栈在一个既有 async side-question 用例溢出。按该测试体系此前使用的 32 MiB 设置，复用同一产物重跑：`RUST_MIN_STACK=33554432 target/debug/deps/shell-fa5e9ad84e948877 image_input_recovery_tests --test-threads=2`，**37 passed**。覆盖主 turn、图片归一化、截图同款拒绝、工具图片、Goal、辅助/OCR/移除、取消、冷恢复，以及三个已修复只读消费者的 HTTP/ACK/文件回归。
- 同一产物，`recap_extension_acknowledges_only_enqueued_commands`：**1 passed**，验证真实 extension 对关闭/可入队 command 的接纳区别。
- `session::actor::run_loop::side_question_lifecycle_tests`：**2 passed**，覆盖接收端提前关闭与运行中关闭。
- `session::slash_commands::tests::flush`：**3 passed**，覆盖解析、命令遮蔽和 memory gate 广告；不是完整 slash 执行证明。
- `session::actor::tests::recap_display_only_tests`：**20 passed**，覆盖手动/自动门禁、display-only、取消、配置快照、工具证据和 Sideband 终态。
- `background_image_compaction_rejects_changed_transport_before_recovery`：**1 passed**。

本轮这些过滤组共 64 个不同测试，不将矩阵内的参数组合另外算作独立测试数。所有 provider 请求都是 loopback mock；没有付费模型请求。未构建/驱动完整 CLI/TUI 旅程，未运行当前无缓存的 Pager test binary；不把旧 debug/release grow 产物当作最新修复的端到端证据。源码结论 F1/F2 未新增运行探针，保留对应 G 和后续验证范围。本轮不创建新 target，不删除并行 Cargo 使用的共享缓存。

## 后续验证范围

先闭合 F1 的 toggle→manual/background/error compact gate，再明确 F2 的计时与失败资格规则。补完整 command-loop/ACP 旅程时要延迟图片 ACK、推进真实控制状态、检查 provider 请求数和文件结果；不能只直接调 `run_memory_flush` 后断言有文字。J30–J32 的成功性属于当前只读契约之外的产品选择，单独决策后再实现。

旅程矩阵 J01–J50 已校对为连续、唯一的 50 行。`git diff --check` 对本次分析与 backlog 通过；归档前全量 `openspec validate --all --strict --no-interactive` 为 20 passed、0 failed。使用 `openspec archive analyze-image-user-journey-triggers --skip-specs --yes` 归档，无主规范改动。归档后全量 strict 为 19 passed、0 failed；archive 首次仅报告本 change 待登记的归档后任务，登记后最终为 811 passed、0 failed。
