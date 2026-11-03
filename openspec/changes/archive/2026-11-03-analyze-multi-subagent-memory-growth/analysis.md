# 多 subagent 内存增长分析

调查日期：2026-11-03，时间均为 Asia/Shanghai。本轮只分析，没有修改 Rust 或运行行为。

## 结论

确认了两个需要分别处理的保留问题：

1. **正常完成后，Pager 仍随累计 child 数量保留完整 child view 和已加载 transcript。** 没有终态 child 重状态的整体卸载；现有 render-cache 清理也只处理当前可见 view。这可以解释长时间运行后的基线增长，但没有分配栈可以把本次增长量归到这些对象。
2. **物理收尾卡住时，coordinator 存在不受现有准入容量限制的 `retiring` 积压路径。** 任务逻辑完成释放 active 名额，旧线程/持久化 owner 仍可存活；下一批任务继续准入。触发条件在源码上成立，本轮未在真实故障或运行测试中复现。

同时确认历史增长中包含显著的 allocator 空闲页驻留。**现存证据没有覆盖到用户报告的 50G 峰值，不能据此认定已定位该峰值的根因，也不能否定其发生。**

## 运行证据

旧进程 PID 86472 的 trace 为 `$GROW_HOME/memtrace/1793614522-86472.jsonl`，版本标记 2.5.5，覆盖 11-02 18:15 至 11-03 12:24。当前安装二进制为 `grow 2.5.5 (bbd2c4dfa) [stable]`，文件修改时间 11-03 09:22，晚于旧进程启动；不能将当前文件的 commit 直接当作旧进程的 build 身份。工作树存在其他任务的未提交改动。

旧进程不同时间的最近样本，单位 GiB：

| 时间 | 活跃分配 allocated | active pages | 进程 footprint | jemalloc resident |
| --- | ---: | ---: | ---: | ---: |
| 11-02 18:15:52 | 0.187 | 0.249 | 0.499 | 0.538 |
| 11-02 23:59:49 | 0.912 | 1.201 | 1.945 | 2.018 |
| 11-03 08:59:52 | 1.148 | 1.505 | 1.772 | 1.836 |
| 11-03 10:59:53 | 1.181 | 1.579 | 2.654 | 2.715 |
| 11-03 12:24:25 | 1.241 | 1.674 | 3.085 | 3.176 |

- 全 trace 峰值：footprint **3.30 GiB**，allocated **1.64 GiB**，resident **3.38 GiB**，RSS **1.90 GiB**；这些峰值并非同一采样点。
- 63 次 `subagent-replay` purge 均安装了实际 hook；单次 footprint 下降中位数约 **0.905 GiB**，最大 **1.872 GiB**。最大一次为 11-03 00:28:42，3.132 → 1.261 GiB。并发进程内活动可能影响前后差值，不将其全部归到某个 child。
- 约 09:00 → 12:24，allocated 净增加约 0.093 GiB，footprint 增加约 1.313 GiB。后段增长大部分没有对应同量的活跃分配增长，但末尾约 1.24 GiB 活跃分配也不能靠 purge 消除。
- 现存 205 个 memtrace JSONL 的最大 footprint 是 10-25 的 **17.50 GiB**。未找到 50G 样本；未发现 `.jsonl.1` 旋转文件。
- 默认约 30 秒采样，旧 trace 有两处间隔超过 60 秒，最长约 175 秒。旧 trace 末尾为 12:24:25，系统重启时间为 12:26:02；短暂峰值或最后这段窗口可能未被记录，缺口原因未知。
- 本轮初查重启后 PID 3999 约 1.1 GiB RSS。它不是旧进程的连续观测，不能将重启前后差值解释为某条清理路径的效果。

系统 `JetsamEvent-2026-11-03-071018.ips` 在 07:10:18 记录了实际内存压力，pageSize 为 16,384 字节。按当时 `rpages × pageSize`，Grow PID 86472 为 1.677 GiB，另有 CursorUIViewService 7.581 GiB、rustc 3.440 GiB、OrbStack Helper 3.418 GiB。它证明当时存在多进程压力，不证明用户看到的 50G 属于其他进程，也不能与 footprint 混算。当天五份 Grow `.ips` 是其他 PID 的签名类 SIGKILL，不是该旧进程的 OOM 证据。12:25:43 的 shutdown-stall 快照中 Grow 已不在进程列表。

### 实际会话规模

旧进程 resume 的主会话为 `01a19a05-5281-7201-bae3-89f599512353`。按 canonical subagent 事件时间截至重启：**106 个 Spawn、101 个 Ended、5 个尚未 Ended**；全部 `context_source=new`。因此，本会话没有证据支持“fork 整份父上下文复制”是此次主因。

分析时当前历史已有 108 个 child，其 timeline 文件合计 **662,076,085 字节（631.4 MiB）**，根 timeline 约 41.9 MiB。文件可能在继续运行；这个数值是读取时的磁盘规模，不是旧进程峰值时的 heap。旧进程保留的统一日志仅覆盖 09:43–12:24，其中 20 个 session 有采样/工具完成记录，共 1,444 次 inference_done、1,735 次 tool.exec_done。不能据此还原前半夜的并发数或历史 view 数。

## 源码路径一：完成 child 的前端重状态没有退休边界

- `pager/src/app/agent_view/mod.rs::AgentView.subagent_views` 保存 `HashMap<String, Box<AgentView>>`，`agent_view/session.rs::insert_subagent_view` 按 child 身份插入。
- `app/acp_handler/session_notification.rs` 的 Spawned 建立 child view；live spawn 加载 child 历史。Finished 更新状态，并为 deferred child 调用 `ensure_subagent_child_replayed`，没有将终态 child 的 view/scrollback 从 parent 所有权中移出。
- `app/subagent.rs::replay_inherited_updates` 流式产生记录，但消费端仍把记录累计到 child 的 scrollback。流式读取降低瞬时解析峰值，不会减少最后保留的展示记录。
- `app/root/mod.rs::maybe_evict_offscreen_caches` 每次只选当前根 view 或打开的一个 child；`scrollback/state/layout.rs::evict_offscreen_render_caches` 只释放可重建的渲染缓存，不释放原始 transcript。离开一个 child 后，该 view 的已生成缓存也不会在此入口统一清空。

结果是前端驻留随**累计已加载 child 历史**增长，不只随同时运行数增长。coordinator 的 1,024 项完成缓存上限不会限制 Pager 的这棵 view 树。相关结构在 HEAD 与当前工作树均存在。

这不等于已证明不可达对象泄漏。它是仍可达对象缺少卸载边界；只要 parent view 存活，旧 child 的重状态就继续有 owner。规范要求首次 live/terminal 前补齐历史，本轮不把此既定行为擅自判为违反契约。

后续最小方向：在现有 AgentView、草稿和交互所有权边界内定义终态 child 的重状态卸载，保留必要元数据与 canonical 历史，复用现有按需 replay；切换离开 view 时处理可重建 render cache。不能直接删除子会话历史，也不能丢掉尚有 owner 的交互。

## 源码路径二：`retiring` 不计入物理资源准入

`tools/src/implementations/grow_build/task/coordinator.rs` 中：

1. Spawn 容量条件为 `pending.len() + active.len() + finalizations.len() >= MAX_FINALIZATIONS`，其中上限为 64。
2. `finish_child` 从 active/pending 移除逻辑任务；持有 physical retirement owner 时，将 completion data 放入 `retiring`。
3. `sweep_retiring` 仅在 `suspension_retired` 返回 `Ok(true)` 时删除；`Ok(false)` 或错误均继续保留，错误经 `.ok()` 转成未就绪。
4. `ShellCompletionData` 保留 child ChatState handle、SessionThread 等。`shell/src/session/actor/spawn.rs::SessionThread::is_finished` 要求 actor 线程和 persistence task 都完成；未完成时仍保留物理 owner。

因此存在以下条件路径：**child 正文执行/持久终态已完成 → 线程或 persistence 收尾停滞 → active 名额释放而 retiring 保留 → 新 child 继续准入 → 重复积压**。`retiring` 没有单独上限，也没计入上述 64 项条件。`RegistryCounts` 仅返回 pending/active/completed，不能从这三个计数证明没有旧物理运行时。

这是静态可达的资源核算缺口。现有 `completed_child_remains_a_retirement_blocker_until_its_owner_is_joined` 测试覆盖一个未收尾 child 阻止树挂起完成，没有覆盖多批完成任务能否绕过物理容量。本轮没有注入卡住的线程/存储，也没有旧进程退休集合或线程栈，故不能认定此次触发了这条路径。

后续最小方向：由现有 coordinator 对仍持有物理资源的 owner 统一核算和反压，保留不可丢弃的收尾责任；区分暂未结束、已结束但 panic、观察失败，不能超时后伪造成功退休。

## 已核对的其他边界

- coordinator 已有最多 64 项 finalization、合计 8 MiB 未持久结果正文保留；成功持久输出后会清空完成缓存中的 output。不能笼统认定所有 child 最终答案都被 coordinator 无界缓存。
- SubagentFinished 的 Grow 投影不携带完整答案；生命周期元数据、进度 publisher 与 preview credits 已有专门边界。64 MiB sampler fragment budget 是每 session 的边界，不是整个进程的总 heap 上限。
- `chat-state::Timeline` 同时保留 events 和当前 Surface，提交继续追加 events。上下文压缩减少模型可见 Surface，不构成全部历史/前端展示的内存上限；这属于规模解释，尚无 allocation profile 证明它贡献了本次多少内存。
- 正常 child 退出路径有 Shutdown、ActiveChildRegistration 的 Drop 移除路由以及 coordinator 的 retirement sweep。没有证据支持“所有已完成 child 都必然缺失 Shutdown”。

## 后续验证如何区分假设

前端：固定 root 和单个 child 的 transcript 规模，顺序运行多批 child，在完成静置、离开 child、重新打开、关闭 parent 各边界比较 live allocations 和 view/scrollback 数量。必须验证从磁盘重建后历史、终态和交互归属仍正确。只看 RSS 或文件大小不能验收回收。

物理收尾：让 child 返回已提交终态，同时保持其 physical owner 未退休；连续提交唯一 task ID，检查仍持有的 owner 是否进入现有容量核算。随后解除阻塞，确认同一 owner 只 join 一次、资源释放且准入恢复；单测只验证一个退休 blocker 不足以覆盖该路径。

再次出现异常时，最有区分力的是同一 PID 的 allocated/active/resident/footprint、child view 与 transcript 规模、retiring/线程数量及 heap allocation stack。当前 memtrace 只有整体数值，阈值文件是 `malloc_stats_print` 统计，不是分配栈；仅有安装的 heap-profile hooks 不能补出之前没有采集的栈。本轮不为此新增诊断协议或常驻监控。
