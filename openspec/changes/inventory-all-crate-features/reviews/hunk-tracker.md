# hunk-tracker 逐包核查

包路径：`crates/codegen/hunk-tracker`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本包已运行198项单测通过，1性能测试与1文档示例ignored。

## 模块与开关

- `crates/codegen/hunk-tracker/Cargo.toml`
- `crates/codegen/hunk-tracker/src/actor/actions.rs`
- `crates/codegen/hunk-tracker/src/actor/file_utils.rs`
- `crates/codegen/hunk-tracker/src/actor/git.rs`
- `crates/codegen/hunk-tracker/src/actor/hunks.rs`
- `crates/codegen/hunk-tracker/src/actor/mod.rs`
- `crates/codegen/hunk-tracker/src/actor/mutations.rs`
- `crates/codegen/hunk-tracker/src/actor/queries.rs`
- `crates/codegen/hunk-tracker/src/actor/state.rs`
- `crates/codegen/hunk-tracker/src/actor/tests.rs`
- `crates/codegen/hunk-tracker/src/commands.rs`
- `crates/codegen/hunk-tracker/src/diff.rs`
- `crates/codegen/hunk-tracker/src/events.rs`
- `crates/codegen/hunk-tracker/src/handle.rs`
- `crates/codegen/hunk-tracker/src/lib.rs`
- `crates/codegen/hunk-tracker/src/loc/mod.rs`
- `crates/codegen/hunk-tracker/src/loc/tests.rs`
- `crates/codegen/hunk-tracker/src/types.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Hunk public model](../specs/hunk-attribution/spec.md#requirement-hunk-public-model)：Hunk SHALL 保存ID、路径、行区间、文本、来源和创建时间；selected不序列化。
- [Hunk tracking modes](../specs/hunk-attribution/spec.md#requirement-hunk-tracking-modes)：TrackingMode SHALL 默认AgentOnly，支持AllDirty外部文件跟踪。
- [Hunk command transport](../specs/hunk-attribution/spec.md#requirement-hunk-command-transport)：Handle SHALL 通过unbounded channel发送命令，动作与查询使用oneshot回复。
- [Hunk actor lifecycle](../specs/hunk-attribution/spec.md#requirement-hunk-actor-lifecycle)：Actor SHALL 在AllDirty启动扫描，并在命令处理边界观察取消。
- [Hunk notification coalescing](../specs/hunk-attribution/spec.md#requirement-hunk-notification-coalescing)：Actor SHALL 去重文件变化及刷新，删除后变化按重建处理。
- [Hunk content classification](../specs/hunk-attribution/spec.md#requirement-hunk-content-classification)：分类 SHALL 区分Missing、Binary、TooLarge、LfsPointer、Symlink、Full，文本阈值1MiB。
- [Hunk bounded read limitations](../specs/hunk-attribution/spec.md#requirement-hunk-bounded-read-limitations)：磁盘读取 SHALL 将I/O错误映射Missing，以metadata预筛大文件和symlink。
- [Hunk HEAD baselines](../specs/hunk-attribution/spec.md#requirement-hunk-head-baselines)：Git基线 SHALL 来自HEAD tree，批量读取共用一次tree解析。
- [Hunk Git status cache](../specs/hunk-attribution/spec.md#requirement-hunk-git-status-cache)：dirty缓存 SHALL 合并TreeIndex与IndexWorktree，staged仅来自TreeIndex，scope替换整份缓存。
- [Hunk agent write baseline](../specs/hunk-attribution/spec.md#requirement-hunk-agent-write-baseline)：普通agent写 SHALL 优先HEAD，Missing才回退previous_content，重算采用当前prompt。
- [Hunk external content admission](../specs/hunk-attribution/spec.md#requirement-hunk-external-content-admission)：外部通知 SHALL 在AgentOnly忽略未知路径，AllDirty结合HEAD与dirty缓存接纳。
- [Hunk baseline reset](../specs/hunk-attribution/spec.md#requirement-hunk-baseline-reset)：reset_baseline SHALL 以缓存current替换baseline，清hunk并发Superseded和BaselineUpdated。
- [Hunk refresh synchronization](../specs/hunk-attribution/spec.md#requirement-hunk-refresh-synchronization)：refresh SHALL 比较HEAD/index mtime跳过重复状态，并分组并行读取磁盘。
- [Hunk diff budget](../specs/hunk-attribution/spec.md#requirement-hunk-diff-budget)：diff SHALL 相等早退，超过1MiB或达到配置的10秒构造预算时返回空结果。
- [Hunk line patch semantics](../specs/hunk-attribution/spec.md#requirement-hunk-line-patch-semantics)：patch_lines SHALL 按行替换并以LF重组文本。
- [Hunk action accounting](../specs/hunk-attribution/spec.md#requirement-hunk-action-accounting)：动作 SHALL 找到hunk后先饱和累计统计并移除turn索引，再执行接受或拒绝。
- [Hunk acceptance baseline](../specs/hunk-attribution/spec.md#requirement-hunk-acceptance-baseline)：Accept SHALL 仅修改内存基线，普通文本局部patch后重算剩余hunk。
- [Hunk rejection disk writes](../specs/hunk-attribution/spec.md#requirement-hunk-rejection-disk-writes)：Reject SHALL 以缓存current生成反向patch，恢复删除文件或删除新建文件。
- [Hunk batch action boundaries](../specs/hunk-attribution/spec.md#requirement-hunk-batch-action-boundaries)：批量动作 SHALL 按文件收集目标、按坐标降序patch并每文件重算一次。
- [Hunk LOC attribution](../specs/hunk-attribution/spec.md#requirement-hunk-loc-attribution)：LOC记录 SHALL 按显式attribution_source归属作者，纯删除采用old行区间。
- [Hunk LOC event accumulation](../specs/hunk-attribution/spec.md#requirement-hunk-loc-event-accumulation)：LOC sink SHALL 对Added累加全计数、ContentChanged累加新旧差值并用trigger归属。
- [Hunk LOC removal accounting](../specs/hunk-attribution/spec.md#requirement-hunk-loc-removal-accounting)：Accepted SHALL 保留已写贡献；Rejected/Superseded对非零累计写无作者冲销。
- [Hunk LOC persistence](../specs/hunk-attribution/spec.md#requirement-hunk-loc-persistence)：JSONL writer SHALL 首写建目录并append，每记录追加LF。
- [Hunk LOC shutdown](../specs/hunk-attribution/spec.md#requirement-hunk-loc-shutdown)：LOC sink SHALL 在取消时try_recv排空队列并在退出时flush。
- [Hunk snapshot restore](../specs/hunk-attribution/spec.md#requirement-hunk-snapshot-restore)：快照 SHALL 保存file_states、turn_index和stats，restore替换这些状态。
- [Hunk snapshot path rewriting](../specs/hunk-attribution/spec.md#requirement-hunk-snapshot-path-rewriting)：rewrite_paths SHALL 优先canonical旧前缀再raw前缀，同时改文件key和hunk路径。
- [Hunk Git discovery caching](../specs/hunk-attribution/spec.md#requirement-hunk-git-discovery-caching)：Git发现 SHALL 缓存成功仓库或NotARepo。
- [Hunk accepted baseline reconciliation](../specs/hunk-attribution/spec.md#requirement-hunk-accepted-baseline-reconciliation)：接受后变化 SHALL 在匹配HEAD时恢复HEAD基线并清accepted标记。
- [Hunk deletion notifications](../specs/hunk-attribution/spec.md#requirement-hunk-deletion-notifications)：删除 SHALL 在AllDirty为未知HEAD文本建立删除hunk，AgentOnly忽略未知路径。
- [Hunk refresh scope cleanup](../specs/hunk-attribution/spec.md#requirement-hunk-refresh-scope-cleanup)：AgentOnly刷新 SHALL 限定tracked路径，空scope清缓存；AllDirty可发现新dirty文件。
- [Hunk grouping and empty files](../specs/hunk-attribution/spec.md#requirement-hunk-grouping-and-empty-files)：重算 SHALL 将连续插删聚合并在Equal处分块，非空Full/Missing形成整文件变化。
- [Hunk identity and source preservation](../specs/hunk-attribution/spec.md#requirement-hunk-identity-and-source-preservation)：匹配 SHALL 优先内容近邻再旧范围交集，每旧ID最多claim一次。
- [Hunk event reconciliation](../specs/hunk-attribution/spec.md#requirement-hunk-event-reconciliation)：事件 SHALL 区分Added、Moved、ContentChanged和Removed，内容变化携带trigger与旧计数。
- [Hunk cached query surface](../specs/hunk-attribution/spec.md#requirement-hunk-cached-query-surface)：查询 SHALL 从缓存返回路径、来源过滤、hunk和文件内容，单ID线性查找。
- [Hunk pending summary](../specs/hunk-attribution/spec.md#requirement-hunk-pending-summary)：summary SHALL 只将AgentEdit计入turn及pending，外部计unattributed。
- [Hunk patch formats](../specs/hunk-attribution/spec.md#requirement-hunk-patch-formats)：补丁 SHALL 提供similar unified patch和手工单hunk patch两种生成入口。

## 边界

- 不验证UUID、绝对路径、行区间或内容大小。
- 移除非agent文件及其hunk并发移除事件，保留agent文件。
- 单hunk动作返回NotFound，批量空成功，查询默认；不提供请求deadline。
- 不保证立即取消；spawn不提供JoinHandle。
- filesystem先处理，其他命令只保持彼此顺序，不保证跨类别FIFO。
- 短LFS只检查prefix，NUL只检查前8000字节，再检查UTF8。
- 不保证硬读取上限、完整内容或同一对象身份。
- 返回Missing；blob先加载后分类，路径canonical可能跟随symlink。
- 可变成空或部分结果，JoinError保留旧缓存。
- 直接清hunk但未同步turn_index和移除事件。
- 当前分支recompute(None)可能把current变Missing并产生删除hunk。
- 不读磁盘、不清session_stats，空turn集合可保留。
- 刷新可提前跳过；同步状态在后续扫描前更新，不保证失败回滚。
- 结果不与无变化区分，后处理不受完整入口硬deadline约束。
- 不保证保留CRLF；尾换行由原content决定，坏行区间未完整校验。
- 统计与索引不回滚，hunk可能仍在，重复失败可继续累计。
- 新建接受整个current；删除可留下Full空串，不自动提交Git或写磁盘。
- 没有版本比较、原子替换、父目录创建或symlink防护保证。
- 已完成修改保留，返回Err而非部分ID列表，不构成事务。
- from_hunk仍给全计数，sink才计算signed delta。
- 不去重或校验prev，累计仅内存，Moved和文件事件不修复累计。
- 不写Removed，aggregate对回退值max(0)，异常负累计与JSONL可能不同。
- 日志丢弃，不回滚累计和先发aggregate，不提供fsync或残行修复保证。
- 不保证未来事件无丢失，没有shutdown deadline。
- 不校验数据、不刷新Git缓存或mode，不重放FileAdded和HunkAdded。
- 区外保持原路径并警告，碰撞可覆盖map，不修复索引。
- 同样缓存NotARepo，本模块不自动重试失效。
- Full忽略单个尾CRLF/LF，部分非文本只比类型；清hunk未同步全部事件与索引。
- 改按变化处理；不存在则current Missing并保留tracking。
- 非agent停止tracking，agent保留并清hunk；非文本依赖dirty缓存判断。
- 不产生hunk，不等于文件未被跟踪。
- 保留agent来源；agent写采用当前prompt，created_at和selected不随ID继承。
- 按重叠而非最终ID集合判断移除，可能漏消失ID；相同位置内容可不报告归属变化。
- turn索引不使整体O(1)，非Full侧当无文本，不重读磁盘。
- files_modified和files_with_pending均为0，不表示历史无修改。
- 不保证CRLF、无尾换行标记或所有偏移后的header坐标准确，未由git apply测试证明。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 测试证据边界

17个Rust文件及manifest全部阅读。actor/tests.rs5742行包含真实Git reset/rebase/scoped-cache测试，LOC测试782行使用人工事件。UI与内存测试只检查API字段，不验证renderer或峰值RSS；overlap fallback未触达时只日志。mixed_states通过不能证明未知非diffable首通知分支正确，需direct actor定向复现。批量动作测试多为正常成功，缺跨文件失败、symlink、并发覆盖、部分写与取消断言。旧bug demonstration注释不代表当前实现，需以实际断言和代码为准。

本次生成器重建了本包审阅文件；功能事实保留在36项映射与场景中，历史逐段阅读进度见verification.md。整体change仍活动，剩22包，不代表全仓库已完成。
