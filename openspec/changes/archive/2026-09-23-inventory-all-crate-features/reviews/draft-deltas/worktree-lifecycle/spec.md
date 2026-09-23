## ADDED Requirements

### Requirement: Worktree feature availability
包 SHALL 提供同步创建、复制、同步、删除及Git快照接口；metadata开放DB/discovery/GC，Linux开放BTRFS与overlay，bench开放基准binary。

#### Scenario: Worktree feature availability boundary
- **WHEN** 使用默认Cargo features
- **THEN** metadata并未默认启用；default-bazel才启用metadata，不保证所有API跨平台可用。

证据：`crates/codegen/fast-worktree/Cargo.toml` — `default-bazel`。

### Requirement: Worktree builder defaults and modes
WorktreeBuilder SHALL 默认HEAD、Linked、PreserveWorkingTree、Ignored Skip、自动并行度和256 channel buffer，支持Standalone/GitCheckout及CleanTracked/CleanAll配置。

#### Scenario: Worktree builder defaults and modes boundary
- **WHEN** 调用standalone(false)或传入参数
- **THEN** false不撤销此前Standalone设置；setter不提供完整输入合法性校验，策略执行仍受平台条件影响。

证据：`crates/codegen/fast-worktree/src/api.rs` — `WorktreeBuilder`。

### Requirement: Worktree plan parallelism
计划 SHALL 对普通和ignored复制分别选择显式并行度，否则使用CPU数。

#### Scenario: Worktree plan parallelism boundary
- **WHEN** 只配置普通parallelism
- **THEN** 计划ignored阶段不继承它；copy_ignored_only入口却会在ignored未配置时继承普通值。

证据：`crates/codegen/fast-worktree/src/worktree/plan.rs` — `effective_ignored_parallelism`。

### Requirement: Worktree disk space error context
create SHALL 在错误链含StorageFull或选定ENOSPC英文消息时增加磁盘不足上下文并保留原错误。

#### Scenario: Worktree disk space error context boundary
- **WHEN** 调用copy_ignored_only或底层复制函数
- **THEN** 不保证有同一顶层磁盘不足标注；创建失败不能据此推导已完整回滚。

证据：`crates/codegen/fast-worktree/src/api.rs` — `not enough free disk space`。

### Requirement: Worktree privileged delegate boundary
BtrfsDelegate SHALL 提供Send加Sync创建删除委托及overlay mount/unmount扩展接口。

#### Scenario: Worktree privileged delegate boundary boundary
- **WHEN** delegate没有实现overlay扩展
- **THEN** 默认返回不支持；委托返回路径与原计划dest没有统一身份校验，能力由实现决定。

证据：`crates/codegen/fast-worktree/src/api.rs` — `BtrfsDelegate`。

### Requirement: Worktree linked materialization
Linked复制路径 SHALL 先Git detached no-checkout登记，再复制与finalize，Preserve使用源index及复制stat，Clean模式执行reset和相应clean。

#### Scenario: Worktree linked materialization boundary
- **WHEN** ignored Copy再次遍历时
- **THEN** 只排除前面成功复制的集合，先前dirty跳过项可能被重复制；Clean与Ignored Copy组合不保证最终仍干净。

证据：`crates/codegen/fast-worktree/src/worktree/execute.rs` — `execute_create_worktree`。

### Requirement: Worktree standalone materialization
Standalone复制路径 SHALL 复制独立.git并并行复制文件，随后等待.git复制结果、finalize索引并处理非HEAD ref。

#### Scenario: Worktree standalone materialization boundary
- **WHEN** 早期复制初始化失败或源HEAD并发变化
- **THEN** 部分提前返回发生在线程join/cleanup guard建立之前；HEAD报告可读取source而不是最终dest。

证据：`crates/codegen/fast-worktree/src/worktree/execute.rs` — `Standalone`。

### Requirement: Worktree GitCheckout strategy
GitCheckout SHALL 调用Git checkout并传入checkout.workers，创建真实linked工作树。

#### Scenario: Worktree GitCheckout strategy boundary
- **WHEN** 设置工作区模式、ignored策略或取消token
- **THEN** 该策略不执行复制模式的这些处理，也不提供同样的失败清理guard。

证据：`crates/codegen/fast-worktree/src/worktree/execute.rs` — `checkout.workers`。

### Requirement: Worktree snapshot strategy selection
Linux Linked及Standalone SHALL 尝试overlay/BTRFS快照，失败可回退复制；GitCheckout使用Git路径。

#### Scenario: Worktree snapshot strategy selection boundary
- **WHEN** 快照成功
- **THEN** 快照包含源ignored内容，不执行普通ignored筛选或token取消检查；复制统计0不代表没有文件。

证据：`crates/codegen/fast-worktree/src/worktree/execute.rs` — `snapshot`。

### Requirement: Worktree snapshot Git cleanup
快照finalize SHALL 清理.git中选定瞬态文件及递归lock并按工作区模式清理，来源marker采用best-effort。

#### Scenario: Worktree snapshot Git cleanup boundary
- **WHEN** snapshot中.git是指针文件或已有marker
- **THEN** 不会作为独立.git目录完整改写；CleanAll的Git clean -fd仍保留ignored，不等于删除所有未跟踪内容。

证据：`crates/codegen/fast-worktree/src/worktree/execute.rs` — `CleanAll`。

### Requirement: Worktree ignored only copy
copy_ignored_only SHALL 先收集unignored集合，再关闭gitignore遍历剩余文件，使用skip patterns与独立worker配置。

#### Scenario: Worktree ignored only copy boundary
- **WHEN** IgnoredFilesMode为Skip或执行中取消
- **THEN** 该入口仍执行复制；取消在复制后检测并返回错误，已落盘内容不回滚。

证据：`crates/codegen/fast-worktree/src/api.rs` — `copy_ignored_only`。

### Requirement: Worktree copy traversal and filtering
copy引擎 SHALL 使用ignore walker、显式.git排除、glob与skip_files过滤并分发文件、目录、符号链接。

#### Scenario: Worktree copy traversal and filtering boundary
- **WHEN** 条目被skip或walker读取失败
- **THEN** skip不保证剪枝整个子树；遍历错误可被忽略，集合不等同Git index tracked语义。

证据：`crates/codegen/fast-worktree/src/copy/engine.rs` — `skip_files`；`crates/codegen/fast-worktree/src/copy/skip.rs` — `git_ignore`。

### Requirement: Worktree copy sharding and backpressure
复制 SHALL 以父路径hash分片到有界worker队列，自动线程数在macOS与其他平台分别有上限。

#### Scenario: Worktree copy sharding and backpressure boundary
- **WHEN** 取消或worker异常
- **THEN** 取消主要停止walker，worker继续排空；send/join错误可能被忽略，保留receiver时worker panic可能造成发送阻塞。

证据：`crates/codegen/fast-worktree/src/copy/engine.rs` — `channel_buffer`；`crates/codegen/fast-worktree/src/copy/shard.rs` — `short_path_hash`。

### Requirement: Worktree copy report semantics
worker SHALL 累计复制文件、处理目录及非致命issues，并尝试记录文件metadata供index stat更新。

#### Scenario: Worktree copy report semantics boundary
- **WHEN** mkdir失败、metadata失败或部分条目复制失败
- **THEN** 目录缓存可能已写入；统计不证明全部内容成功落盘，issues没有独立数量上限。

证据：`crates/codegen/fast-worktree/src/copy/worker.rs` — `issues`。

### Requirement: Worktree CoW and symlink copying
文件复制 SHALL 优先reflink并回退普通复制，设置permissions；Unix符号链接保留link target。

#### Scenario: Worktree CoW and symlink copying boundary
- **WHEN** 替换已有目标或跨平台复制
- **THEN** 删除旧目标是best-effort且非原子，无fsync/全部metadata保证；非Unix链接处理不提供Unix等价保证。

证据：`crates/codegen/fast-worktree/src/copy/cow.rs` — `reflink`。

### Requirement: Worktree git directory cloning
独立.git复制 SHALL 枚举目录及文件，跳过lock、fsmonitor和选定瞬态文件，较大集合按块并行。

#### Scenario: Worktree git directory cloning boundary
- **WHEN** 源.git为linked指针或复制失败
- **THEN** 指针文件被拒绝；并行线程全部join并返回错误，可能保留部分dest，不改写alternates等外部路径。

证据：`crates/codegen/fast-worktree/src/copy/gitdir.rs` — `fsmonitor`。

### Requirement: Worktree Git discovery and tracked count
Git辅助 SHALL 解析.git目录或gitdir指针、发现工作树根并读取HEAD；tracked计数读取gix index或HEAD。

#### Scenario: Worktree Git discovery and tracked count boundary
- **WHEN** gitdir相对路径或大量索引
- **THEN** 相对路径基于工作树解析并canonical fallback；计数入口包含索引加载，不能视为恒定成本全树统计。

证据：`crates/codegen/fast-worktree/src/git/discovery.rs` — `gitdir:`；`crates/codegen/fast-worktree/src/lib.rs` — `count_tracked_files`。

### Requirement: Worktree Git index copying
copy_git_index SHALL 复制源index并处理common/own目录下sharedindex文件；源index不存在返回false。

#### Scenario: Worktree Git index copying boundary
- **WHEN** split-index链接已损坏或源目的路径相同
- **THEN** 无统一原子替换/同路径保护；sharedindex未逐个验证hash，Unix链接使用原始源路径。

证据：`crates/codegen/fast-worktree/src/git/index.rs` — `copy_git_index`。

### Requirement: Worktree index stat refresh
update_index_stats SHALL 按复制metadata更新命中条目的stat字段并写索引。

#### Scenario: Worktree index stat refresh boundary
- **WHEN** 索引缺失、空、metadata为空或路径未命中
- **THEN** 可直接成功或忽略条目；解析固定SHA1，路径lossy，不提供SHA256仓库与非UTF8完整保证。

证据：`crates/codegen/fast-worktree/src/git/index.rs` — `update_index_stats`。

### Requirement: Worktree dirty status collection
get_modified_files SHALL 收集index与工作区差异、未跟踪项及事件计数。

#### Scenario: Worktree dirty status collection boundary
- **WHEN** 需要HEAD与index的暂存差异或索引缺失
- **THEN** 此入口不提供完整staged状态；缺失/空索引可返回空，计数与去重集合大小不必相等。

证据：`crates/codegen/fast-worktree/src/git/status.rs` — `get_modified_files`。

### Requirement: Worktree scoped stale registration cleanup
stale注册清理 SHALL 限定exact目标或路径前缀，保留locked、live及不匹配注册，并尝试canonical缺失路径祖先。

#### Scenario: Worktree scoped stale registration cleanup boundary
- **WHEN** 读取失败或路径在检查后变化
- **THEN** 可跳过错误且无原子再验证；调用方负责范围归属，不执行无限定全局prune。

证据：`crates/codegen/fast-worktree/src/git/worktree.rs` — `locked`。

### Requirement: Worktree Git subprocess environment
git_command SHALL 关闭交互stdin、detach并覆盖pager及选定认证/LFS环境，加入no-optional-locks。

#### Scenario: Worktree Git subprocess environment boundary
- **WHEN** 继承其他GIT环境或仓库filter配置
- **THEN** 仍可能影响命令；无统一deadline或取消，diff非零可视为有变化，at_ref解析失败返回false。

证据：`crates/codegen/fast-worktree/src/git/checkout.rs` — `git_command`。

### Requirement: Worktree scratch Git snapshots
Git快照 SHALL 用临时index从HEAD执行add -A/write-tree/commit-tree并更新指定ref，保留真实index作为独立文件。

#### Scenario: Worktree scratch Git snapshots boundary
- **WHEN** 源HEAD变化或原先有不同staged/worktree内容
- **THEN** 不保证同一时刻HEAD快照，也不保留原两阶段staging；update-ref无CAS，属性filter仍可参与。

证据：`crates/codegen/fast-worktree/src/git/checkout.rs` — `commit-tree`。

### Requirement: Worktree snapshot ref transfer
ref转移 SHALL 执行fetch no-tags force并验证目标ref可解析为commit。

#### Scenario: Worktree snapshot ref transfer boundary
- **WHEN** 源和目标共享对象存储或源ref并发改变
- **THEN** 仍会执行fetch；不比较转移前后源SHA，也不保证fsync持久化。

证据：`crates/codegen/fast-worktree/src/git/checkout.rs` — `--no-tags`。

### Requirement: Worktree snapshot rehydration
恢复 SHALL 尝试以snapshot父提交为HEAD，父不可用时回退snapshot，再read-tree reset-u恢复树。

#### Scenario: Worktree snapshot rehydration boundary
- **WHEN** 目标已存在或恢复中失败
- **THEN** 可能先删除目标；失败best-effort清理，恢复差异进入index，不还原原XY状态；metadata登记为Subagent。

证据：`crates/codegen/fast-worktree/src/git/checkout.rs` — `read-tree`。

### Requirement: Worktree sync HEAD and cleaning
WorktreeSync SHALL 仅在源与目的HEAD不同时hard reset，按skip_clean控制clean -fd，并可复制dirty状态。

#### Scenario: Worktree sync HEAD and cleaning boundary
- **WHEN** 两端HEAD相同且dest已有tracked修改
- **THEN** 不会先reset这些修改；skip_clean保留遗留untracked，普通clean也保留ignored，依赖调用方提供合适dest。

证据：`crates/codegen/fast-worktree/src/sync.rs` — `WorktreeSync`。

### Requirement: Worktree precomputed dirty state
SourceDirtyState SHALL 缓存porcelain v2 NUL状态Bytes并允许复用于多个同步目标。

#### Scenario: Worktree precomputed dirty state boundary
- **WHEN** collect后源文件或HEAD改变
- **THEN** apply仍读取当前内容，没有源身份/HEAD版本绑定；None或空预计算状态跳过dirty，预计算入口不提供普通timing值。

证据：`crates/codegen/fast-worktree/src/sync.rs` — `SourceDirtyState`。

### Requirement: Worktree dirty file and index replay
同步 SHALL 处理普通、rename、untracked、删除与Unixsymlink，并用index-info写源staged mode/blob以保留MM区别。

#### Scenario: Worktree dirty file and index replay boundary
- **WHEN** 源文件已消失、删除失败或staged命令非零
- **THEN** 部分计数仍可增加；staged非零仅warn，不保证报告意味着全部状态已复制，也无阶段回滚。

证据：`crates/codegen/fast-worktree/src/sync.rs` — `--index-info`。

### Requirement: Worktree sync parser and submodule limits
dirty解析 SHALL 跳过submodule条目并按porcelain类型提取路径。

#### Scenario: Worktree sync parser and submodule limits boundary
- **WHEN** 非UTF8、畸形记录、unmerged或copy类型rename记录
- **THEN** 不提供通用健壮解析保证；unmerged不还原三stage，type2可能按rename删除旧路径。

证据：`crates/codegen/fast-worktree/src/sync.rs` — `submodule`。

### Requirement: Worktree generic removal and batch scope
remove_worktree SHALL 先尝试Linux特定删除再普通symlink unlink或目录删除，成功后best-effort注销DB；batch扫描一至二层。

#### Scenario: Worktree generic removal and batch scope boundary
- **WHEN** 条目不是本项目工作树或lstat失败
- **THEN** 普通删除未统一验证ownership；lstat错误可按缺失成功，batch第二层不要求.git，注册清理错误可忽略。

证据：`crates/codegen/fast-worktree/src/api.rs` — `cleanup_worktrees_in`。

### Requirement: Worktree BTRFS removal and recovery
Linux删除 SHALL 处理snapshot symlink、direct/bind及metadata恢复，支持部分delegate回退。

#### Scenario: Worktree BTRFS removal and recovery boundary
- **WHEN** snapshot拒绝或umount失败
- **THEN** 某些路径仍清目标引用/返回成功；snapshot安全拒绝、实际引用清理与DB注销不是原子事务。

证据：`crates/codegen/fast-worktree/src/api.rs` — `remove_worktree`。

### Requirement: Worktree BTRFS orphan reclamation
BTRFS orphan扫描 SHALL 使用已知mount存储目录及metadata识别目标，检查活跃mount或symlink引用并尝试回收。

#### Scenario: Worktree BTRFS orphan reclamation boundary
- **WHEN** snapshot删除失败或目标父目录不可见
- **THEN** 删除失败保留metadata，父目录缺失可跳过；坏metadata可被清理，报告计数不保证所有best-effort动作成功。

证据：`crates/codegen/fast-worktree/src/api.rs` — `cleanup_orphaned_btrfs_snapshots`。

### Requirement: Worktree GC age policy
GC SHALL 依次采用skip kind、per-kind期限、global期限，按max(created,last_accessed)计算年龄。

#### Scenario: Worktree GC age policy boundary
- **WHEN** 期限None、force或未来时间
- **THEN** never不被force覆盖；未来age按0处理，超期使用严格比较，dead清理独立于年龄保护。

证据：`crates/codegen/fast-worktree/src/api.rs` — `age_path_enabled`。

### Requirement: Worktree GC liveness protection
非force年龄回收 SHALL 检查保护路径、creator PID与可用进程CWD，并在删除前重查。

#### Scenario: Worktree GC liveness protection boundary
- **WHEN** CWD扫描失败或不支持平台
- **THEN** Failed阻止非force年龄删除，Unsupported与非Unix PID策略不等价于完整保护；扫描自己可见不证明全部进程可见。

证据：`crates/codegen/fast-worktree/src/api.rs` — `LiveCwdScan`。

### Requirement: Worktree GC persistence boundaries
GC SHALL 对dead记录清理DB并对超期记录调用磁盘删除，dry-run只报告候选。

#### Scenario: Worktree GC persistence boundaries boundary
- **WHEN** 自定义DB或并发修改记录
- **THEN** 磁盘remove默认DB注销可能不对应传入DB；重查非原子且可使用原记录path，不构成事务一致性保证。

证据：`crates/codegen/fast-worktree/src/api.rs` — `gc_worktrees`。

### Requirement: Worktree database schema and journal
metadata DB SHALL 存储worktree记录与meta，id主键及path唯一，按sqlite-journal选本地WAL或network per-host TRUNCATE。

#### Scenario: Worktree database schema and journal boundary
- **WHEN** journal Busy/Locked或高版本schema
- **THEN** 约10秒预算重试后失败；schema v1初始化不提供完整升级迁移或高版本拒绝，network库不迁移legacy rows。

证据：`crates/codegen/fast-worktree/src/db/mod.rs` — `set_journal_mode`；`crates/codegen/fast-worktree/src/db/schema.rs` — `SCHEMA_VERSION`。

### Requirement: Worktree database replacement and decoding
登记 SHALL INSERT OR REPLACE整条记录，路径保存为lossy文本；未知kind退Manual、未知status退Dead。

#### Scenario: Worktree database replacement and decoding boundary
- **WHEN** id/path冲突或坏metadata
- **THEN** 可能替换原记录；坏JSON读为None，raw PID整数窄转不额外校验，登记不验磁盘存在。

证据：`crates/codegen/fast-worktree/src/db/queries.rs` — `row_to_record`。

### Requirement: Worktree database lookup and filtering
get SHALL 将含/输入按canonical fallback路径查询，否则先ID再label；list提供状态、kind、repo及source exact组合过滤。

#### Scenario: Worktree database lookup and filtering boundary
- **WHEN** label重复或status=Dead但include_dead=false
- **THEN** label按created_at最新取一且不排除dead；后者过滤相交为空；并列排序不保证稳定。

证据：`crates/codegen/fast-worktree/src/db/mod.rs` — `get_by_label`；`crates/codegen/fast-worktree/src/db/queries.rs` — `include_dead`。

### Requirement: Worktree database maintenance
DB SHALL 提供mark_dead、touch、stats和sweep_dead，touch不复活dead。

#### Scenario: Worktree database maintenance boundary
- **WHEN** 磁盘exists出错、并发替换或中途SQL失败
- **THEN** sweep可能把不可访问视为缺失且保留部分更新；stats容量为页数估计，不含WAL/SHM或事务快照保证。

证据：`crates/codegen/fast-worktree/src/db/queries.rs` — `sweep_dead`。

### Requirement: Worktree home and record identity
默认DB SHALL 优先GROW_HOME，否则canonical fallback HOME/.grow；ID由basename与完整原始路径hash形成。

#### Scenario: Worktree home and record identity boundary
- **WHEN** GROW_HOME为空、相对或路径别名不同
- **THEN** 不额外规范化或拒绝；hash不保证无碰撞，调用方的canonical时机决定记录身份。

证据：`crates/codegen/fast-worktree/src/db/mod.rs` — `resolve_grow_home`。

### Requirement: Worktree disk discovery
discovery SHALL 扫描worktrees与worktree_pool两层，分类Session/Pool，跳过点前缀和ready/claimed/claiming marker。

#### Scenario: Worktree disk discovery boundary
- **WHEN** 目录没有.git或扫描失败
- **THEN** 仍可发现unknown模式目录；读取错误略过，不保证完整候选列表，linked source仅解析gitdir三层parent。

证据：`crates/codegen/fast-worktree/src/discovery.rs` — `discover_worktrees`。

### Requirement: Worktree database rebuild
rebuild SHALL 检查canonical fallback目标在管理root内，按ID/path跳过已登记项，新项保存一次now为last_accessed。

#### Scenario: Worktree database rebuild boundary
- **WHEN** 已登记dead、symlink root或中途失败
- **THEN** 不复活或touch已有项；root本身可canonical到外部，逐项写入无事务，discovered包含拒绝项。

证据：`crates/codegen/fast-worktree/src/discovery.rs` — `rebuild_worktree_db`。

### Requirement: Worktree auto GC configuration
auto GC SHALL 默认开启、7天age、6小时间隔、Manual never，rebuild默认关闭且24小时间隔；local覆盖remote再覆盖默认。

#### Scenario: Worktree auto GC configuration boundary
- **WHEN** 环境设置kill/dry-run/rebuild/max-age
- **THEN** 只支持相应单向覆盖，age clamp为1小时至90天、间隔60秒至7天；raw options不重复全部clamp。

证据：`crates/codegen/fast-worktree/src/auto_gc.rs` — `resolve_worktree_auto_gc_from_layers`。

### Requirement: Worktree automatic GC sequencing
自动GC SHALL 先检查GC节流，再可选重建、收集source repo、GC、orphan及scoped prune，force恒false。

#### Scenario: Worktree automatic GC sequencing boundary
- **WHEN** GC命中节流或dry-run
- **THEN** 节流跳过整轮，dry-run跳过重建/prune/orphan但仍可写GC stamp；非扫描平台真实年龄回收关闭。

证据：`crates/codegen/fast-worktree/src/auto_gc.rs` — `maybe_auto_gc`。

### Requirement: Worktree auto GC failure and stamps
GC stamp读取失败 SHALL 返回Err，重建stamp读取失败只跳过重建；未来或不可解析stamp视到期。

#### Scenario: Worktree auto GC failure and stamps boundary
- **WHEN** 重建成功后GC失败或局部删除失败
- **THEN** 前者不写两stamp且保留已重建记录；后者仍可stamp，stamp写失败仅report false，节流不是跨进程互斥。

证据：`crates/codegen/fast-worktree/src/auto_gc.rs` — `rebuild_stamped`。

### Requirement: Worktree mount parsing
挂载工具 SHALL 解析mountinfo、匹配overlay/FUSE及取首lower层，并提供namespace状态与可读PID upper集合。

#### Scenario: Worktree mount parsing boundary
- **WHEN** 根挂载、非ASCII路径或受限proc可见性
- **THEN** 根前缀匹配与逐字节解码存在边界；集合不证明全部namespace完备，Host只相对可见PID1，Unknown可能仍启用overlay。

证据：`crates/codegen/fast-worktree/src/mount_info.rs` — `MountNsStatus`。

### Requirement: Worktree BTRFS detection
BTRFS检测 SHALL 使用statfs与btrfs subvolume show，再借findmnt和mountinfo解析bind来源及mount point。

#### Scenario: Worktree BTRFS detection boundary
- **WHEN** 工具不可用、转义路径或没有根mount
- **THEN** 可降为None或错误；解析按首个可用设备路径且不统一解码，subvol fallback不等于完整挂载拓扑解析。

证据：`crates/codegen/fast-worktree/src/btrfs/detect.rs` — `is_btrfs_subvolume`。

### Requirement: Worktree BTRFS snapshot layout
快照 SHALL 对无bind来源直接在dest创建；bind来源在mount下worktrees或.grow-snapshots以basename加完整dest hash创建并symlink暴露。

#### Scenario: Worktree BTRFS snapshot layout boundary
- **WHEN** mount路径词法等于source root或已有snapshot
- **THEN** 相等才选隐藏目录；已有项依据安全路径及metadata三态处理，悬空snapshot symlink可能绕过exists清理分支。

证据：`crates/codegen/fast-worktree/src/btrfs/snapshot.rs` — `snapshot_dest_path`。

### Requirement: Worktree BTRFS metadata and exposure
快照暴露 SHALL 允许替换symlink或空目录，拒绝非空目录；暴露失败尝试回收新snapshot。

#### Scenario: Worktree BTRFS metadata and exposure boundary
- **WHEN** 回收或metadata写入失败
- **THEN** 回收错误丢弃，metadata失败warn仍可成功；metadata直接写无原子持久化，Matches只比mount_target。

证据：`crates/codegen/fast-worktree/src/btrfs/snapshot.rs` — `snapshot_meta_state`。

### Requirement: Worktree BTRFS delete guard
safe predicate SHALL 拒绝ParentDir与候选symlink，要求canonical父目录名为指定存储名且直属BTRFS mount。

#### Scenario: Worktree BTRFS delete guard boundary
- **WHEN** 候选不存在或调用裸delete_snapshot
- **THEN** predicate可接受不存在候选，不证明subvolume或会话所有权；裸删除函数不自动执行guard。

证据：`crates/codegen/fast-worktree/src/btrfs/snapshot.rs` — `is_safe_snapshot_delete_target`。

### Requirement: Worktree overlay detection and layout
overlay SHALL 在FUSE lower与BTRFS upper条件下尝试snapshot整个upper父目录，并用root/upper及新overlay-work挂载。

#### Scenario: Worktree overlay detection and layout boundary
- **WHEN** upper不是固定名称或两个dest同basename
- **THEN** 检测不验证这些前提；backing仅按basename命名可共用路径，不提供BTRFS hash布局的同等隔离。

证据：`crates/codegen/fast-worktree/src/overlay/detect.rs` — `detect_fuse_overlay`；`crates/codegen/fast-worktree/src/overlay/snapshot.rs` — `create_overlay_worktree`。

### Requirement: Worktree overlay creation failures
overlay创建 SHALL 在mount前写base metadata，delegate可接管mount；直接mount使用index=on。

#### Scenario: Worktree overlay creation failures boundary
- **WHEN** 中间mkdir/metadata失败或mount失败
- **THEN** 前者可直接返回留下snapshot；后者best-effort清理且dest可保留，路径option未统一escape。

证据：`crates/codegen/fast-worktree/src/overlay/snapshot.rs` — `mount_overlay`。

### Requirement: Worktree overlay removal and orphan scan
overlay移除 SHALL 尝试unmount并拒绝可见active upper，随后清snapshot/work/metadata；恢复与orphan扫描限定/local/repo-fuse前缀。

#### Scenario: Worktree overlay removal and orphan scan boundary
- **WHEN** snapshot删除失败、不可见namespace或不可信metadata
- **THEN** 失败仍可清metadata；扫描不完备且未统一ownership/containment校验，removed及unmounted标志不证明所有动作成功。

证据：`crates/codegen/fast-worktree/src/overlay/snapshot.rs` — `cleanup_orphaned_overlay_snapshots`。

### Requirement: Worktree CLI behavior
fast-worktree CLI SHALL 提供create、ref、dirty、ignored、skip、并行度与standalone参数；默认CleanAll及buffer1024。

#### Scenario: Worktree CLI behavior boundary
- **WHEN** copy issues存在或文件复制计数为0
- **THEN** issues打印warning仍成功；0计数显示snapshot不证明实际策略，mode展示采用请求值。

证据：`crates/codegen/fast-worktree/src/bin/cli.rs` — `Commands`。

### Requirement: Worktree pool benchmark scope
pool-perf-bench SHALL 测量create/warm/sync/use/release/cleanup阶段并提供文本及JSON格式输出。

#### Scenario: Worktree pool benchmark scope boundary
- **WHEN** A/B模式或零iterations
- **THEN** A/B仍串行；启动会修改source Git性能配置且不恢复，部分Git非零不失败，JSON特殊字符串及stdout纯度无保证。

证据：`crates/codegen/fast-worktree/src/bin/pool_perf_bench.rs` — `run_ab_iteration`。

### Requirement: Worktree platform validation boundary
包验证记录 SHALL 区分实际执行、平台排除、环境跳过与仅代码审阅；Linux快照集成依赖真实受隔离stack。

#### Scenario: Worktree platform validation boundary boundary
- **WHEN** 仅观察测试通过计数或在macOS测试
- **THEN** 不证明Linux快照运行；现有overlay集成含旧布局和固定mnt basename夹具，BTRFS部分错误只打印，不能扩展为完整行为证明。

证据：`crates/codegen/fast-worktree/tests/overlay_integration.rs` — `detect_overlay_env`。


### Requirement: Shell worktree type and restore config precedence

WorktreeType SHALL 精确识别linked/standalone/git并分别映射Linked/Standalone/GitCheckout，默认Linked；本地cli.worktree_type非法字符串或错型warning后视缺失，resolver再试remote，返回local/remote/default来源。同步worktree_type只读取effective本地不接收remote，加载错Linked。restore_code仅有效本地bool优先remote，再默认false；错型不阻断remote。use_leader_sync加载错false，其余委托use_leader_from_toml，不在此连接leader。

#### Scenario: Invalid local type with valid remote
- **WHEN** 本地worktree_type未知且remote standalone
- **THEN** 返回Standalone及remote来源。

证据：`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn resolve_worktree_type`；`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn worktree_type`；`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn resolve_restore_code`；`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn use_leader_sync`。

### Requirement: Shell worktree GC settings adapter boundary

shell GC解析 SHALL 将worktree.auto_gc反序列化为共享settings，失败默认；本地与remote各自经workspace adapter转换，再委托fast-worktree层解析。adapter保留enabled/max_age/min_interval/dry_run/orphan/rebuild/rebuild_interval；逐kind键仅识别WorktreeKind，未知键跳过，Secs映射Some秒数、Never映射None。这里不执行删除、平台年龄策略或调度，不能以配置解析通过认定GC已安全完成。

#### Scenario: Unknown kind in settings
- **WHEN** max_age_by_kind含无法解析的kind
- **THEN** adapter忽略该项，其余合法项继续传给底层resolver。

证据：`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn worktree_auto_gc_from_toml`；`crates/codegen/shell/src/util/config/worktree.rs` — `pub fn resolve_worktree_auto_gc_from_settings`；`crates/codegen/workspace/src/worktree/mod.rs` — `pub fn worktree_auto_gc_layer_from_settings`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_defaults_enabled`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_env_disabled_wins_over_remote_and_local`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_local_wins_over_remote_ttl_and_env_dry_run_wins`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_remote_ttl_clamped`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_upper_clamp_via_resolve`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn worktree_auto_gc_toml_bad_field_keeps_enabled_false`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_defaults_manual_never`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn resolve_worktree_auto_gc_kind_map_local_wins_over_remote`；`crates/codegen/shell/src/util/config/worktree.rs` — `fn worktree_auto_gc_toml_kind_map_parses_never`。


### Requirement: Shell persistence worktree liveness refresh
Shell持久化构造与load_light SHALL 在存储初始化或加载成功后以spawn_blocking更新会话cwd所属worktree的活跃时间，最多等待2秒；超时仅debug记录并继续，后台任务不被取消，JoinError也不传播。new_child在初始Timeline构造成功后执行同样等待。此更新不保证后续会话打开步骤成功，也不提供与GC互斥的全程保护。持久化actor从启动时刻计时，在收到任意PersistenceMsg且距上次触发至少3600秒时先重置计时并脱离等待触发touch，再处理消息；不是定时心跳，空闲无消息不刷新，失败不立即重试。workspace touch仅在找到cwd所属记录后调用db.touch，错误debug记录，无法找到记录时无操作。

#### Scenario: Open touch timeout
- **WHEN** 存储加载成功但touch任务超过2秒
- **THEN** 会话继续后续打开步骤，touch仍可能后台完成。

#### Scenario: Idle actor
- **WHEN** actor超过一小时没有收到PersistenceMsg
- **THEN** 不因时间经过自行发起touch。

#### Scenario: Traffic after interval
- **WHEN** 距actor上次触发至少一小时后收到Flush消息
- **THEN** 先触发后台touch并重置间隔，再处理Flush，touch失败不阻断消息。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `async fn touch_worktree_for_session`。
- `crates/codegen/shell/src/session/persistence.rs` — `async fn run(mut self)`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) async fn new(`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) async fn new_child`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) async fn load_light`。
- `crates/codegen/workspace/src/worktree/mod.rs` — `pub fn touch_worktree_for_cwd`。
