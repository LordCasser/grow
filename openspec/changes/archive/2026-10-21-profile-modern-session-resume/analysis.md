# 现代长会话恢复：剩余瓶颈

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

结论：原来的“局部快、整体不变”主要是样本没有覆盖热点。补充现代样本后，还发现了一条未使用 bulk restore 的 writer 前缀验证路径，以及两次随正文体积增长的 canonical projection 构造。长历史的主要成本落在这些路径，主 Timeline::from_events 已经不是大头。

## 1. 测量口径与样本

本机 macOS ARM64，dev/test 未优化构建，debug=0、incremental=0、jobs=2；冷进程/冷 actor，生成后文件页缓存温热，未清 OS 缓存。RPC 从 load 发出到 response 返回，排除 initialize/auth、fixture 建立、完整历史 oracle 和返回后工作。只有计时窗口内的 instrumentation 入表；父子 inclusive timers 不相加。各列独立取三轮中位数，括号为三次范围。采样诊断不纳入中位数。

现代 fixture 使用合法 request completed / response admission、空正文 ordering anchor、生产 projector 的 canonical payload/digest，以及当前 mock model route baseline。每 turn 一个 user 与一个完整 assistant；canonical 正文为 32 KiB 或 4 KiB，并带可见序号。Catalog 与 rewind 负载保留。缺 projection 样本每16轮遗漏一条但保留 anchor，检验 writer load 补写。全部数据在隔离临时目录、本地 mock 中生成，没有读写用户会话。

legacy 对照来自首轮；现代正式数据补齐了 route baseline，未改变生产恢复算法。Legacy 单大 chunk 同时改变 Timeline 正文大小和通知数，两项 legacy 都没有路由基线；不把它们的总差值解释成单变量分片收益。

| 样本 | Timeline / updates（MB） | 历史文本通知数 | 首条历史（s） | RPC 完成中位数（范围，s） |
| --- | ---: | ---: | ---: | ---: |
| Legacy：8 × 4 KiB | 2.62 / 19.65 | 4608 | 0.740 | 1.627（1.619–1.669） |
| Legacy：1 × 32 KiB | 17.30 / 18.97 | 1024 | 1.037 | 1.839（1.716–2.050） |
| 现代：128 turns × 32 KiB | 4.44 / 4.82 | 256 | 0.738 | 1.304（1.254–1.448） |
| 现代：512 turns × 32 KiB | 17.75 / 19.28 | 1024 | 2.683 | 3.967（3.967–4.957） |
| 现代：2048 turns × 32 KiB | 71.03 / 77.14 | 4096 | 10.355 | 17.815（17.720–19.443） |
| 现代：512 turns × 4 KiB | 3.07 / 4.60 | 1024 | 0.612 | 1.504（1.495–1.526） |
| 现代：512 turns，缺 32 条 projection | 17.75 / 18.22 | 1024 | 2.898 | 4.206（4.187–4.215） |
| 现代：512 turns，缺路由基线 | 17.75 / 19.28 | 1024 | 3.371 | 4.105（4.068–4.635） |

原始结构化数据见 comparison-measurements.json，三轮 stdout/stderr 按 profile-run-N.txt 保存。现代六组为18次正式 timing；另有两组 legacy 共6次。首轮12次缺路由基线的现代探索保存在 initial-measurements.json 和 unseeded-modern-*.txt，未混入现代正式中位数。

没有重建四项优化之前的整个现代版本。本轮建立的是优化后现代基线，不能据此声称“现代会话优化前后整体也不变”。

## 2. 原来的端到端为什么没体现四项局部收益

- R1：旧 append_perf_timeline 写的 assistant 没有 response_admission。最贵的 completed-attempt/seen identity/全 Surface quarantine 校验分支没有被触发。新测 legacy512 的普通 fold 约1.5 ms；不能把2048 admissions微基准的337→10 ms套进这里。
- R2：旧 updates 没有 canonical projection，P×U 二次查重热点没有代表性。新测 legacy512 的 append planning 仅约2–3 ms。去掉 clone 有收益，但远小于约1.6 s总量。
- S1：正常冷 load 不走主 exact-commit 查询热点；完整现代 projection 不需新 append。缺失 projection 也由已校验的 append plan 走普通 durable append，并非逐项 exact lookup。S1 的16次 commit收益主要对应写入/重试场景。
- U1：11.5→2.94 μs是干净布局维护，不能代替 Markdown render、终端输出、Shell等待或启动时间。上一轮完整 bounded replay frame 改善很小，已经记录；不是整体 TUI 帧提速4倍。

Amdahl 定律适用：T = T_other + T_hit，只优化 T_hit 才会影响总量。旧样本的 T_hit 接近零；现代样本中，优化过的主 fold 虽快，其他校验/编码仍保留了大量工作。

## 3. 2048 turns 的关键路径

以下取总耗时中位数所在的实际一轮（17.815 s），从父阶段中选择互不重叠的节点：

| 非重叠主阶段 | 耗时（s） | 占 RPC 总量 |
| --- | ---: | ---: |
| load_light | 4.247 | 23.8% |
| 主 replay | 6.855 | 38.5% |
| actor 启动注册 | 6.401 | 35.9% |
| stale task 对账 | 0.210 | 1.2% |
| 其余与传输边界残差 | 0.102 | 0.6% |

主 load_light、replay、actor 互不重叠。下述细分已包含在表中，不能再次加到总量。

### 3.1 Writer 首次追加触发另一遍历史恢复

actor call 约6.10 s。源码链为 append_timeline_event_with_durability → spawn_blocking → append_timeline_line_in_directory_sync_with_probe → load_timeline_prefix。prefix cache 为空时读取完整已提交前缀，逐事件 Timeline::accept；后者 validate 克隆整个累计 LifecycleFold，应用后还要析构旧 fold。已有主 Timeline::from_events 使用私有 accept_replayed，避免了这个累计复制；prefix 校验没有走它。

这不是“actor 没复用 validated Timeline”，也不是“多次调用主 from_events”。actor 复用了该对象，独立的持久化 writer 为首次 append 再验一次 prefix。其累计 lifecycle 克隆在请求数增长时呈二次工作量。主 fold 在本轮仅14.5 ms，和这条私有验证路径必须分开看。

独立 2秒 CPU 采样 中，选中的 blocking worker 共292次样本全部在 load_timeline_prefix，其中约76%在 Timeline::accept 及 lifecycle 克隆/析构路径。该比例只描述这个线程和时间窗，不是整个进程或总恢复的占比。采样运行未计入 timing，见 命令 与 诊断运行。这证实 prefix 恢复是 actor call 中持续消耗 CPU 的热点，但没有把整个6.10 s都精确计作 prefix 独占时间。

相关源码：[prefix恢复](../../../../crates/codegen/shell/src/session/storage/jsonl/mod.rs:1921)、[实时校验克隆](../../../../crates/codegen/chat-state/src/timeline.rs:3865)、[bulk replay入口](../../../../crates/codegen/chat-state/src/timeline.rs:2087)。这是 session-timeline 中 Bulk replay avoids cumulative lifecycle copying 契约的实现覆盖缺口；不通过缩窄契约掩盖，已单独登记 R5，当前 change只记录和测量。

### 3.2 同一 load 构造两遍 canonical projection

typed append plan 中 canonical 构造约2.739 s，随后 raw replay 再构造约2.748 s，合计5.487 s，占总量30.8%。每次包括从 Timeline 推导 admitted responses、序列化完整 ConversationItem 生成 digest及构造 canonical；不能把整个 span都称作纯 SHA耗时。

源码分别在 plan_updates_for_response_projections 和 reconcile_raw_replay_lines_with_mode。R2去掉的是二次全量 existing 扫描及多余输出，并没有让这两层共享一次 canonical 结果。raw planner遇到 candidate anchor时，即使物理 exact已存在，也会以 canonical在anchor处重排；projection_envelope又构造Value并序列化JSON。其余 raw response reconcile 约1.37 s，包含物理projection解码、规划与输出物化，未进一步把它全部归因于某一个函数。

相关源码：[typed canonical](../../../../crates/codegen/shell/src/session/response_projection.rs:336)、[raw canonical与物化](../../../../crates/codegen/shell/src/session/storage/mod.rs:4266)、[anchor选择](../../../../crates/codegen/shell/src/session/response_projection.rs:250)。

### 3.3 解码和交付也按正文体积增长

Timeline read/decode约0.925 s、完整物理 updates decode约0.547 s。version校验先解析Value，随后再typed解码；TimelineEvent内部也通过Value重构kind。read_snapshot仅约25 ms，但这是温热本地页缓存，不代表真正冷盘。重点是重复解码/物化，不是仅看文件大小就归因于磁盘。

forward/enqueue约1.569 s，等待gateway/client completions约0.840 s。raw projection在forward处还要typed解码、展开为ACP，然后跨管道序列化和客户端解码。这个client只计数，timing时不保留历史Vec；其中没有TUI绘制。forward循环是同步的，首批gateway任务需要等它让出执行；因此首条历史时间还包括前面的全量准备和enqueue工作。

现代512只有正文从32 KiB降到4 KiB，Timeline事件和最终通知数相同，canonical合计由约1.362 s降为0.185 s，而主fold仍约3–4 ms；总量3.967→1.504 s。这组同族对照比单看turn数更清楚地显示正文成本。

### 3.4 固定启动与恢复写入

coordination backend安装ACK约0.30 s；prepare与snapshot publish均很小。安装命令在actor循环中更新tool bridge resource再ACK，所以这段包含actor循环启动/调度及资源更新等待；未把它进一步武断归因于registry锁或某个固定sleep。短样本中这个固定等待占比更高。

缺32条projection的样本，durable appends约236.5 ms，总量比完整512样本多约239 ms。这是实际文件/目录同步成本；不允许为了基准省略durability。

缺模型路由基线的512对照，model_route_append约626 ms；prefix验证提前发生于路由补写，随后actor call较短。补齐baseline后，同一类首次writer校验移到actor startup，首条历史更早，但整体并不自动少掉这项工作。因此区分“工作消除”和“工作移动”。

## 4. 下一步按收益与边界排序

1. **R5：私有 writer prefix 恢复改用 bulk fold。** 最小方向是读有界records并更新原BLAKE3 hash，收集同一批typed events后一次from_events，不调用实时accept逐条复制。保留append锁、完整前缀长度/hash/seq检查、失败私有性和file/directory sync；不改live accept，不先建新缓存框架。
2. **R6：一次恢复内复用 canonical 构造，并减少exact投影重编码。** 限于同一validated snapshot/lease；已有exact payload仍需核对，anchor顺序、冲突、rewind/cursor与repair不能被缓存命中绕过。分别测digest构造和JSON物化，不能直接把5.49 s都当作可消除时间。
3. **解码与forward链。** 减少version Value/typed重复解析、canonical→Value→String→typed往返；验证是否需要有界让出执行以改善首条历史。继续保留load响应前全部notification completion的顺序屏障。
4. **短会话再细分coordination ACK固定等待。** 不能为缩短计时提前发布未安装好的actor。
5. TUI同时区分历史末尾可见与真正SessionLoaded。先处理writer算法造成的等待，再按实测考虑收尾状态提示，不用虚构播放进度。

以上均是独立后续切口，本轮未修改这些算法，也没有新增持久索引或跳过验证。

## 5. 真正的CLI/TUI边界

同一构建的真实grow --continue，临时mock配置，两个PTY测试各跑三次。CLI样本保留每turn8次catalog、16个commands，比Shell样本的catalog大，因此不能把二者总时差当成纯TUI成本。以下是三轮中位数：

| PTY样本 | 末尾历史可见（s） | SessionLoaded observed（s） | ready按键p95（ms） |
| --- | ---: | ---: | ---: |
| 128 turns | 2.044 | 2.255 | 7.7 |
| 512 turns | 6.427 | 7.513 | 7.8 |
| 512 turns，40ms慢写注入 | 6.484 | 7.510 | 58.0 |

512正常终端中，历史末尾约6.427 s已经可见，而真正加载完成约7.513 s，存在约1.1 s的收尾窗口。慢写样本三轮都看到11次历史推进，最大内容间隔351–357 ms；之后仍需等writer startup。ready后的输入回显很快，这支持“加载阶段工作未完成”，不支持把等待误判为composer处理慢。
 
首次PTY用画面稳定+模型标签判断ready，512轮时注入被正确忽略，导致探针失败；见 原始失败。现改为明确观察pager.replay.end_batch（SessionLoaded dispatch）再测试输入，同时单独记录末尾标记首次可见时间，不把500 ms画面稳定等待加入这两个时点。输入延迟与内容推进断言均保留，六次实际测试执行全部通过。

正常终端第1轮、第2轮、第3轮；慢写第1轮、第2轮、第3轮。

512普通终端的一个实测轮次中，79次frame render合计约592 ms，diff约57 ms、write约1.8 ms，分布在约7.7 s启动过程中。这些计时与Shell处理可重叠，不从wall time直接相减；128轮另观察到约205 ms的单帧峰值，不能声称所有TUI停顿已消除。
