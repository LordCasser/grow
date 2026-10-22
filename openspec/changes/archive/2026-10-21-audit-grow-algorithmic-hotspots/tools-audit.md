# 底层算法优化审计：memory、索引、工具与文件系统

状态：只读源码审计；未运行构建、测试、基准或真实会话，也未修改生产代码。以下是限定范围的代表性热点，不是全仓穷举或实测性能结论。源行号以本次审计时的工作树为准。

## 覆盖范围

| 模块 | 核对内容 | 结论 |
| --- | --- | --- |
| memory | 混合检索候选数、合并去重、评分排序、MMR | 未列候选：默认 `max_results=6`，每类查询候选按 3 倍取回；FTS/向量与 evergreen 补充是契约要求。没有证据表明有限小候选的排序值得改。 |
| codebase-graph | 文件变更缓存、刷新扫描、解析器和查询缓存 | 未列候选：刷新时全树查找缓存外的新文件是发现新文件的必要线性扫描；解析器与查询按语言缓存。只看到路径比较时的 lossless-string 转换成本，缺少证据证明其值得单独优化。 |
| tools 与 catalog | 工具定义快照、notification schema catalog、工具查询 | 1 项待测候选（T4）。notification schema catalog 目前在本 crate 仅由单元测试调用；静态 catalog 不构成生产热点。 |
| workspace | `workspace.fs_list` / `workspace.client_fs_list` 共享遍历、元数据、排序及分页 | 1 项明确候选（T3）。列表最多收集 50,000 项，排序使用 `sort_by_cached_key`，文件读取上限 4 MiB；这些边界不消除每项重复元数据系统调用。 |
| hunk-tracker | 文件变更重算、patch 渲染、按 ID/turn 查询 | 1 项明确候选（T2）。文件 diff 已限制为 1 MiB 和 10 秒；未把必需 diff 算法本身列为缺陷。`GetTurnHunks` 的索引后逐 ID 全表查找虽有 O(T×H) 结构，但当前仓库没有生产调用点，故不列为生产候选。 |
| fsnotify | 初始监听目录发现、预算截断、动态子树监听 | 1 项明确候选（T1）。动态子树监听达到 watch budget 后会停止；初始 PerDir 监听仍会先收集完整目录集再排序和截断。 |

## 候选

### T1 — 初始 PerDir watch budget 没有约束发现阶段的内存与排序工作

- **证据 / 触发**：`crates/codegen/fsnotify/src/watcher.rs:657-669` 的 `select_per_dir_watch_dirs` 通过 `pruning_walker` 全量收集所有有效目录，再按深度稳定排序。启动 `FsEventSource::start_on` 进入 PerDir 分支时调用该函数，然后在 `:1064-1075` 截断到 `max_watch_budget()`。Linux 默认 watch budget 为 49,152，用户也可用 `GROW_FSNOTIFY_MAX_WATCHES` 改写（`:154-167`）。
- **复杂度变量**：D 为所有通过 ignore/custom glob 过滤的目录数，P 为路径深度和字节长度。当前发现需要 O(D) 路径存储、O(D log D) 深度排序，并保留完整目录路径；budget 只约束后续 watch 数量。
- **最小优化**：遍历时维护至多 B 个“最浅优先”目录的有界 max-heap，B 为 budget；仍访问目录以执行 gitignore 与自定义过滤，但选择阶段把额外内存降到 O(B)，排序成本降到 O(D log B)。若保持同深度 DFS 顺序可观察性，heap 项需要携带遍历序号作为稳定次级键；先确认这一顺序是否属于契约，再决定是否值得保留。
- **已有缓存 / 上限**：没有发现启动目录选择缓存；已有 49,152 默认 watch 上限以及后续分批 arming，但它们不限制本函数的收集峰值。动态 `add_subtree_watches` 在 budget 到达后停止 walk，边界不同。
- **语义边界**：必须继续按现有 walker 规则剪枝 ignored、`.git`、`.sl` 和 custom-ignore 目录；截断必须优先保留浅层目录，现有代码对同深度保持 DFS 原序。注意 root 与额外 VCS watches 目前不计入子目录 budget，优化不能改变此计数。
- **现有验证**：`watcher.rs:3293-3327` `select_per_dir_prunes_nested_gitignored_dirs`；`:3329-3342` `select_per_dir_orders_shallow_first`。建议验证方法是对预算截断前后的选择结果做差分：同一目录树与 glob 配置下，保留数量、浅层优先、同深度原序、剪枝结果逐项相同；再补一个远大于 budget 的合成目录树峰值内存/耗时基准。审计未运行这些验证。
- **置信度**：高（全量 collect/sort 与截断顺序均为直接源码事实；收益大小需测量）。

### T2 — 一个文件有多个 hunk 时重复扫描并分配整份文件行表

- **证据 / 触发**：文件变化或用户操作触发 `HunkTrackerActor::recompute_hunks`，在 Full/Full 状态调用 `compute_hunks`（`crates/codegen/hunk-tracker/src/actor/hunks.rs:45-51`）。Shell hunk-tracker extension 请求文件 diff 数据时调用 handle（`crates/codegen/shell/src/extensions/hunk_tracker.rs:305`）；`get_file_hunk_data` 对每个 hunk 调用 `generate_hunk_patch`（`crates/codegen/hunk-tracker/src/actor/queries.rs:38-67`），而 patch 函数每次都对 baseline 和 current 分别执行完整 `.lines().collect()`（`crates/codegen/hunk-tracker/src/diff.rs:81-84`）。
- **复杂度变量**：B、C 是 baseline/current 行数，K 是该文件 hunk 数；patch 构造阶段的行扫描/引用向量分配约为 O(K×(B+C))，patch 输出本身的大小另计。单 hunk 文件为线性工作，多 hunk 且接近 1 MiB 上限时重复成本增大。
- **最小优化**：在 `get_file_hunk_data` 为两份文本各建一次行切片，再将 `&[&str]` 传给内部 patch helper；保留现有单 hunk 公共 helper 作为薄包装，复用同一格式化逻辑。只消除重复行扫描和临时 Vec，不改 patch 内容或 diff 算法。
- **已有缓存 / 上限**：`MAX_TRACKED_TEXT_BYTES` 与 `MAX_DIFF_FILE_SIZE` 均为 1 MiB；`compute_hunks` 设 10 秒 timeout。没有发现文件级行切片缓存；diff 结果保存在 actor 状态中，但渲染每次仍重建行表。
- **语义边界**：行号仍按当前 1-based hunk 坐标解释；before-context 来自 baseline，after-context 的新内容仍来自 current；添加、删除文件和空文本状态仍按原入口处理。避免借此改写已有 patch header 或末尾换行规则。
- **现有验证**：`diff.rs:777-831` 的 patch context 与多 hunk add/delete 测试；`actor/tests.rs:2950-3200` 覆盖 Full/Missing/Binary/TooLarge 状态的 `GetFileHunkData`。建议将同一多 hunk 样例同时送入旧/新 helper 比较完整 patch 字节，另加接近 1 MiB 且 K 较大的测量样例。审计未运行验证。
- **置信度**：高（重复构造直接可见；实际 K 分布与端到端收益需测量）。

### T3 — workspace 列表对每个条目执行两次 metadata syscall

- **证据 / 触发**：共享 `walk_fs_entries` 为每个保留条目先调用 `symlink_metadata` 读取是否为链接，再调用 `metadata` 跟随链接读取大小、类型和修改时间（`crates/codegen/workspace/src/file_system/walk.rs:104-115`）。调用方包括 shell-facing `workspace.fs_list`（`ext_fs.rs:161-182`）及 client-facing `workspace.client_fs_list`（`client_fs.rs:146-169`），同一核心还服务本地文件系统入口（`walk.rs` 模块注释）。
- **复杂度变量**：N 为实际收集的条目数，最多 50,000。当前至少有 2N 次 metadata 相关调用；confined symlink 路径还可能在 walker filter 中先执行一次 `symlink_metadata`（`walk.rs:89-90,125-132`）。
- **最小优化**：用一次 `symlink_metadata` 取得入口类型；普通文件/目录直接复用该 metadata，仅当 symlink 且现有 follow-links 语义要求跟随时再调用 `metadata`。这样普通条目每项减少一次 syscall，同时保留 symlink target size/type 行为。
- **已有缓存 / 上限**：共享列表上限 50,000 项；结果排序缓存每个条目的排序 key（`:252`）。未发现 metadata 缓存，跨请求缓存还会引入文件系统变更失效问题，不建议作为最小改动。
- **语义边界**：`is_symlink` 描述目录项本身；`is_dir`、size 和 modified 当前来自 follow-link 的 metadata。不可把入口 lstat 的 symlink 类型直接当作目标类型，也不能改变删除竞态下条目被跳过的规则。根约束仍须在遍历期间有效。
- **现有验证**：`client_fs.rs:382-424` 覆盖 dirs-first、稳定排序分页和 cap 行为；`:449-491` 覆盖根约束和 symlink 结果。建议测试普通条目只需一份 lstat 路径的抽象 helper，以及真实 symlink 仍报告 `is_symlink=true`、并沿用目标元数据。审计未运行验证。
- **置信度**：中高（syscall 重复由实现直接确认；实际列表规模与 I/O 延迟决定收益）。

### T4 — 每个采样步骤重新深拷贝完整内置工具定义

- **证据 / 触发**：每个新 Step 在 `crates/codegen/shell/src/session/actor/turn/sampling.rs:760-788` 调用 `prepare_tool_definitions_inner`；它从 bridge 取得 builtins-only definitions（`:796-797`），再按压缩状态、Goal、Behavior 和 Plan 状态投影可见目录（`:798-857`）。registry 的 `tool_definitions_builtins_only` 对每个工具深拷贝 `ToolDefinition`（`crates/codegen/tools/src/registry/types.rs:1397-1404`）。已解析列表之后还会由 `turn_base_tool_specs` 克隆成 ToolSpec（`sampling.rs:780-784`）。
- **复杂度变量**：每次 Step 的复制量与工具数及 schema/description 总字节数成正比；当前也会执行多轮 retain 和按名称查 ToolKind。此审计没有统计实际工具数、schema 字节数或采样频率下的耗时。
- **最小优化**：先为一次稳定 tool-registry generation 统计定义深拷贝字节量和耗时；若有可测成本，令底层不可变 definition 快照共享，再只在过滤或输出序列化处物化所需对象。需保留每 Step 的动态投影和最终调用方 owned API，避免过早改变协议或权限边界。
- **已有缓存 / 上限**：`FinalizedToolset` 持有一个已终结 registry；未发现定义快照缓存。MCP 更新会重建/刷新 catalog；工具可见性还依赖 Timeline compaction、Goal 与行为状态，不能跨 Step 缓存过滤后的最终结果。
- **语义边界**：Tool manifest 必须在每个新 Step 反映 compaction rewind、实时 Goal/Behavior 变化及 MCP catalog 刷新。隐藏可见性与 dispatch authorization 是两个不同边界；优化不得通过减少或滞后 tool identity 检查改变授权语义。
- **现有验证**：`sampling.rs` 相关 tool-definition projection/lifecycle 测试，以及 `registry/types.rs` 中 `tool_definitions_builtins_only` 和 MCP catalog lifecycle 测试。建议先增加只读计量/criterion 样例对比一次 Step 的定义字节和构造时间，再用既有状态切换测试确保快照更新及时；审计未运行验证。
- **置信度**：中（重复 clone 有直接证据，但 catalog 小、拷贝可能不是请求主成本，必须先测量）。

## 对抗性排除与边界

- `memory::hybrid_search` 默认 `max_results=6`（`config-types/src/memory.rs:55-80`），FTS/vector 候选各自按 `3×max_results` 请求；排序、内容克隆、MMR 都受候选数约束。用户可配置结果数，且没有在本次范围内发现硬上限，但没有证据表明默认有限候选是值得优化的热点。FTS 与向量独立候选、evergreen 补充、完整 chunk 内容、MMR 前排序都是契约定义的行为（`openspec/specs/memory-search/spec.md`）；不建议为降低拷贝改掉这些行为。
- memory `get_chunk` 对候选逐个读 SQLite 确有多次查询形态，但默认最多几十个候选，且调用建立了混合评分所需完整 chunk 输出；只有测得数据库往返主导后再评估批量读取。
- codebase-graph 的背景刷新扫描工作区以发现 cache 外新增源码文件；即使已缓存已有路径，仍须枚举 workspace。它使用路径 HashSet，parser/query 有按语言缓存，因此扫描的 O(F) I/O 本身不作为缺陷。单次 `to_string_lossy()` 比较分配在 `index_manager.rs:1390-1403` 可被借用路径比较消除，但没有数据支持将其提升为候选。
- `hunk-tracker::get_hunk` 对所有 tracked hunk 按 ID 线性搜索，而 `get_hunks_for_turn` 对 turn index 中每个 ID 再调用它（`actor/queries.rs:106-121`）。然而本次全仓调用点搜索只找到 actor 分发和 handle 定义，没有生产 crate 调用 `get_turn_hunks`；只记录为未启用 API 的结构性债务，未列入候选。
- workspace 目录分页必须遍历到 `min(depth, max_collect)` 以维持全局 dirs-first、case-insensitive 且稳定的 offset/page 结果；当前 50,000 项上限及 `sort_by_cached_key` 已限制其峰值与重复 key 计算。本审计未把全量 walk 或 O(N log N) 排序本身列为问题。
- fsnotify 每个初始 PerDir watcher 只需建立一次监听，但遍历目录以确定 watch 集合是必要工作；T1 仅指出在 budget 已知时仍保留全部目录并全量排序的额外选择成本。Fanout selector 已按 top-level fanout 上限提前停走；动态 PerDir 子树添加也会按预算停走，不能推广为所有 fsnotify 路径都有无界成本。
- tools notification `notification_schema_catalog()` 以 BTreeMap 构造所有 schema（`tools/src/notification/types.rs:438-447`），但全仓引用只有定义/导出与本地 catalog 单元测试；没有证据说明它位于生产请求路径，故不列候选。
- 本次 Atlas 查询先执行 `project(action="open")`，再限定到 memory、codebase-graph、workspace、hunk-tracker 和 tools crate 的符号名查询；未启动全库索引。Atlas scoped search 仅用于结构入口定位，不据其 Focus 范围推断仓库全局无调用；生产调用缺失结论来自限定目录的 `rg` 调用点核对。

## 本范围内的规范证据

- `openspec/specs/memory-search/spec.md` 是 memory 目录下检索、候选、排序和 MMR 的行为权威。
- `openspec/README.md` 说明规范目录不是全内部函数穷举。对本次其他模块，未找到匹配的已归档算法行为契约；这里只报告源码行为与现有测试，不把旧审计或 archive 中草案当作规范。
- 历史模块审核 `openspec/changes/archive/2026-09-23-inventory-all-crate-features/reviews/{memory,codebase-graph,tools,workspace,hunk-tracker,fsnotify}.md` 仅作追溯材料。本次候选由当前实现行与当前调用关系重新核对。
