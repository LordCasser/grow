## ADDED Requirements

### Requirement: Hunk public model
Hunk SHALL 保存ID、路径、行区间、文本、来源和创建时间；selected不序列化。

#### Scenario: Hunk public model boundary
- **WHEN** 公开构造或反序列化
- **THEN** 不验证UUID、绝对路径、行区间或内容大小。

证据：`crates/codegen/hunk-tracker/src/types.rs` — `Hunk`。

### Requirement: Hunk tracking modes
TrackingMode SHALL 默认AgentOnly，支持AllDirty外部文件跟踪。

#### Scenario: Hunk tracking modes boundary
- **WHEN** AllDirty切回AgentOnly
- **THEN** 移除非agent文件及其hunk并发移除事件，保留agent文件。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `set_mode`。

### Requirement: Hunk command transport
Handle SHALL 通过unbounded channel发送命令，动作与查询使用oneshot回复。

#### Scenario: Hunk command transport boundary
- **WHEN** channel关闭
- **THEN** 单hunk动作返回NotFound，批量空成功，查询默认；不提供请求deadline。

证据：`crates/codegen/hunk-tracker/src/handle.rs` — `HunkTrackerHandle`。

### Requirement: Hunk actor lifecycle
Actor SHALL 在AllDirty启动扫描，并在命令处理边界观察取消。

#### Scenario: Hunk actor lifecycle boundary
- **WHEN** 命令正在await文件或Git操作
- **THEN** 不保证立即取消；spawn不提供JoinHandle。

证据：`crates/codegen/hunk-tracker/src/actor/mod.rs` — `spawn`。

### Requirement: Hunk notification coalescing
Actor SHALL 去重文件变化及刷新，删除后变化按重建处理。

#### Scenario: Hunk notification coalescing boundary
- **WHEN** 批次包含查询或agent写
- **THEN** filesystem先处理，其他命令只保持彼此顺序，不保证跨类别FIFO。

证据：`crates/codegen/hunk-tracker/src/actor/mod.rs` — `CoalescedBatch`。

### Requirement: Hunk content classification
分类 SHALL 区分Missing、Binary、TooLarge、LfsPointer、Symlink、Full，文本阈值1MiB。

#### Scenario: Hunk content classification boundary
- **WHEN** 识别LFS和Binary
- **THEN** 短LFS只检查prefix，NUL只检查前8000字节，再检查UTF8。

证据：`crates/codegen/hunk-tracker/src/actor/file_utils.rs` — `classify_bytes`。

### Requirement: Hunk bounded read limitations
磁盘读取 SHALL 将I/O错误映射Missing，以metadata预筛大文件和symlink。

#### Scenario: Hunk bounded read limitations boundary
- **WHEN** 文件增长、首read短读或lstat/open竞争
- **THEN** 不保证硬读取上限、完整内容或同一对象身份。

证据：`crates/codegen/hunk-tracker/src/actor/file_utils.rs` — `read_file_bounded`。

### Requirement: Hunk HEAD baselines
Git基线 SHALL 来自HEAD tree，批量读取共用一次tree解析。

#### Scenario: Hunk HEAD baselines boundary
- **WHEN** 对象缺失或读取失败
- **THEN** 返回Missing；blob先加载后分类，路径canonical可能跟随symlink。

证据：`crates/codegen/hunk-tracker/src/actor/git.rs` — `read_baselines_batch`。

### Requirement: Hunk Git status cache
dirty缓存 SHALL 合并TreeIndex与IndexWorktree，staged仅来自TreeIndex，scope替换整份缓存。

#### Scenario: Hunk Git status cache boundary
- **WHEN** 扫描初始化或条目错误
- **THEN** 可变成空或部分结果，JoinError保留旧缓存。

证据：`crates/codegen/hunk-tracker/src/actor/git.rs` — `refresh_git_dirty_cache`。

### Requirement: Hunk agent write baseline
普通agent写 SHALL 优先HEAD，Missing才回退previous_content，重算采用当前prompt。

#### Scenario: Hunk agent write baseline boundary
- **WHEN** 已有文件写非diffable内容
- **THEN** 直接清hunk但未同步turn_index和移除事件。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `record_agent_write`。

### Requirement: Hunk external content admission
外部通知 SHALL 在AgentOnly忽略未知路径，AllDirty结合HEAD与dirty缓存接纳。

#### Scenario: Hunk external content admission boundary
- **WHEN** 未知非diffable文件具有Full HEAD基线
- **THEN** 当前分支recompute(None)可能把current变Missing并产生删除hunk。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `process_file_change`。

### Requirement: Hunk baseline reset
reset_baseline SHALL 以缓存current替换baseline，清hunk并发Superseded和BaselineUpdated。

#### Scenario: Hunk baseline reset boundary
- **WHEN** 执行reset
- **THEN** 不读磁盘、不清session_stats，空turn集合可保留。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `reset_baseline`。

### Requirement: Hunk refresh synchronization
refresh SHALL 比较HEAD/index mtime跳过重复状态，并分组并行读取磁盘。

#### Scenario: Hunk refresh synchronization boundary
- **WHEN** Git状态相同而工作区文本变化
- **THEN** 刷新可提前跳过；同步状态在后续扫描前更新，不保证失败回滚。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `refresh_all_baselines`。

### Requirement: Hunk diff budget
diff SHALL 相等早退，超过1MiB或达到配置的10秒构造预算时返回空结果。

#### Scenario: Hunk diff budget boundary
- **WHEN** 超限或超时
- **THEN** 结果不与无变化区分，后处理不受完整入口硬deadline约束。

证据：`crates/codegen/hunk-tracker/src/diff.rs` — `compute_hunks`。

### Requirement: Hunk line patch semantics
patch_lines SHALL 按行替换并以LF重组文本。

#### Scenario: Hunk line patch semantics boundary
- **WHEN** 输入CRLF或插入文本带尾换行
- **THEN** 不保证保留CRLF；尾换行由原content决定，坏行区间未完整校验。

证据：`crates/codegen/hunk-tracker/src/diff.rs` — `patch_lines`。

### Requirement: Hunk action accounting
动作 SHALL 找到hunk后先饱和累计统计并移除turn索引，再执行接受或拒绝。

#### Scenario: Hunk action accounting boundary
- **WHEN** 拒绝写入失败
- **THEN** 统计与索引不回滚，hunk可能仍在，重复失败可继续累计。

证据：`crates/codegen/hunk-tracker/src/actor/actions.rs` — `apply_hunk_action`。

### Requirement: Hunk acceptance baseline
Accept SHALL 仅修改内存基线，普通文本局部patch后重算剩余hunk。

#### Scenario: Hunk acceptance baseline boundary
- **WHEN** 接受新建或删除
- **THEN** 新建接受整个current；删除可留下Full空串，不自动提交Git或写磁盘。

证据：`crates/codegen/hunk-tracker/src/actor/actions.rs` — `accept_hunk`。

### Requirement: Hunk rejection disk writes
Reject SHALL 以缓存current生成反向patch，恢复删除文件或删除新建文件。

#### Scenario: Hunk rejection disk writes boundary
- **WHEN** 磁盘并发变化或写失败
- **THEN** 没有版本比较、原子替换、父目录创建或symlink防护保证。

证据：`crates/codegen/hunk-tracker/src/actor/actions.rs` — `reject_hunk`。

### Requirement: Hunk batch action boundaries
批量动作 SHALL 按文件收集目标、按坐标降序patch并每文件重算一次。

#### Scenario: Hunk batch action boundaries boundary
- **WHEN** 跨文件中途失败
- **THEN** 已完成修改保留，返回Err而非部分ID列表，不构成事务。

证据：`crates/codegen/hunk-tracker/src/actor/actions.rs` — `apply_action_batch`。

### Requirement: Hunk LOC attribution
LOC记录 SHALL 按显式attribution_source归属作者，纯删除采用old行区间。

#### Scenario: Hunk LOC attribution boundary
- **WHEN** 直接构造Updated记录
- **THEN** from_hunk仍给全计数，sink才计算signed delta。

证据：`crates/codegen/hunk-tracker/src/loc/mod.rs` — `from_hunk`。

### Requirement: Hunk LOC event accumulation
LOC sink SHALL 对Added累加全计数、ContentChanged累加新旧差值并用trigger归属。

#### Scenario: Hunk LOC event accumulation boundary
- **WHEN** 重复或缺前置事件
- **THEN** 不去重或校验prev，累计仅内存，Moved和文件事件不修复累计。

证据：`crates/codegen/hunk-tracker/src/loc/mod.rs` — `handle_event`。

### Requirement: Hunk LOC removal accounting
Accepted SHALL 保留已写贡献；Rejected/Superseded对非零累计写无作者冲销。

#### Scenario: Hunk LOC removal accounting boundary
- **WHEN** 累计缺失或为零
- **THEN** 不写Removed，aggregate对回退值max(0)，异常负累计与JSONL可能不同。

证据：`crates/codegen/hunk-tracker/src/loc/mod.rs` — `HunkRemovalReason::Accepted`。

### Requirement: Hunk LOC persistence
JSONL writer SHALL 首写建目录并append，每记录追加LF。

#### Scenario: Hunk LOC persistence boundary
- **WHEN** 写入失败
- **THEN** 日志丢弃，不回滚累计和先发aggregate，不提供fsync或残行修复保证。

证据：`crates/codegen/hunk-tracker/src/loc/mod.rs` — `JsonlHunkRecordWriter`。

### Requirement: Hunk LOC shutdown
LOC sink SHALL 在取消时try_recv排空队列并在退出时flush。

#### Scenario: Hunk LOC shutdown boundary
- **WHEN** 持续生产或writer阻塞
- **THEN** 不保证未来事件无丢失，没有shutdown deadline。

证据：`crates/codegen/hunk-tracker/src/loc/mod.rs` — `drain_remaining`。

### Requirement: Hunk snapshot restore
快照 SHALL 保存file_states、turn_index和stats，restore替换这些状态。

#### Scenario: Hunk snapshot restore boundary
- **WHEN** 恢复快照
- **THEN** 不校验数据、不刷新Git缓存或mode，不重放FileAdded和HunkAdded。

证据：`crates/codegen/hunk-tracker/src/actor/mod.rs` — `HunkTrackerSnapshot`。

### Requirement: Hunk snapshot path rewriting
rewrite_paths SHALL 优先canonical旧前缀再raw前缀，同时改文件key和hunk路径。

#### Scenario: Hunk snapshot path rewriting boundary
- **WHEN** 路径在区外或改写碰撞
- **THEN** 区外保持原路径并警告，碰撞可覆盖map，不修复索引。

证据：`crates/codegen/hunk-tracker/src/types.rs` — `rewrite_paths`。

### Requirement: Hunk Git discovery caching
Git发现 SHALL 缓存成功仓库或NotARepo。

#### Scenario: Hunk Git discovery caching boundary
- **WHEN** 临时访问错误导致发现失败
- **THEN** 同样缓存NotARepo，本模块不自动重试失效。

证据：`crates/codegen/hunk-tracker/src/actor/git.rs` — `NotARepo`。

### Requirement: Hunk accepted baseline reconciliation
接受后变化 SHALL 在匹配HEAD时恢复HEAD基线并清accepted标记。

#### Scenario: Hunk accepted baseline reconciliation boundary
- **WHEN** Full尾换行差异或部分非文本同类型
- **THEN** Full忽略单个尾CRLF/LF，部分非文本只比类型；清hunk未同步全部事件与索引。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `baseline_accepted`。

### Requirement: Hunk deletion notifications
删除 SHALL 在AllDirty为未知HEAD文本建立删除hunk，AgentOnly忽略未知路径。

#### Scenario: Hunk deletion notifications boundary
- **WHEN** 已跟踪路径实际仍存在
- **THEN** 改按变化处理；不存在则current Missing并保留tracking。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `handle_file_deleted`。

### Requirement: Hunk refresh scope cleanup
AgentOnly刷新 SHALL 限定tracked路径，空scope清缓存；AllDirty可发现新dirty文件。

#### Scenario: Hunk refresh scope cleanup boundary
- **WHEN** 文件变clean
- **THEN** 非agent停止tracking，agent保留并清hunk；非文本依赖dirty缓存判断。

证据：`crates/codegen/hunk-tracker/src/actor/mutations.rs` — `is_clean`。

### Requirement: Hunk grouping and empty files
重算 SHALL 将连续插删聚合并在Equal处分块，非空Full/Missing形成整文件变化。

#### Scenario: Hunk grouping and empty files boundary
- **WHEN** 创建或删除空文件
- **THEN** 不产生hunk，不等于文件未被跟踪。

证据：`crates/codegen/hunk-tracker/src/actor/hunks.rs` — `recompute_hunks`。

### Requirement: Hunk identity and source preservation
匹配 SHALL 优先内容近邻再旧范围交集，每旧ID最多claim一次。

#### Scenario: Hunk identity and source preservation boundary
- **WHEN** 外部匹配已有agent hunk
- **THEN** 保留agent来源；agent写采用当前prompt，created_at和selected不随ID继承。

证据：`crates/codegen/hunk-tracker/src/actor/hunks.rs` — `recompute_hunks`。

### Requirement: Hunk event reconciliation
事件 SHALL 区分Added、Moved、ContentChanged和Removed，内容变化携带trigger与旧计数。

#### Scenario: Hunk event reconciliation boundary
- **WHEN** 拆分或合并
- **THEN** 按重叠而非最终ID集合判断移除，可能漏消失ID；相同位置内容可不报告归属变化。

证据：`crates/codegen/hunk-tracker/src/actor/hunks.rs` — `emit_hunk_diff_events`。

### Requirement: Hunk cached query surface
查询 SHALL 从缓存返回路径、来源过滤、hunk和文件内容，单ID线性查找。

#### Scenario: Hunk cached query surface boundary
- **WHEN** 读取turn或生成文件patch
- **THEN** turn索引不使整体O(1)，非Full侧当无文本，不重读磁盘。

证据：`crates/codegen/hunk-tracker/src/actor/queries.rs` — `get_file_hunk_data`。

### Requirement: Hunk pending summary
summary SHALL 只将AgentEdit计入turn及pending，外部计unattributed。

#### Scenario: Hunk pending summary boundary
- **WHEN** 所有agent hunks解决
- **THEN** files_modified和files_with_pending均为0，不表示历史无修改。

证据：`crates/codegen/hunk-tracker/src/actor/queries.rs` — `compute_session_summary`。

### Requirement: Hunk patch formats
补丁 SHALL 提供similar unified patch和手工单hunk patch两种生成入口。

#### Scenario: Hunk patch formats boundary
- **WHEN** 使用手工单hunk patch
- **THEN** 不保证CRLF、无尾换行标记或所有偏移后的header坐标准确，未由git apply测试证明。

证据：`crates/codegen/hunk-tracker/src/diff.rs` — `generate_hunk_patch`。

