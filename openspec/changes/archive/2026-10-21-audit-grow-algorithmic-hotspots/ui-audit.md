# UI 算法审计

本记录仅基于源码审计；没有构建、运行测试、启动真实会话或修改生产代码。行为契约参考 `openspec/specs/client-surfaces/spec.md:1`，该规范说明展示更新与持久化会话事实的职责边界。本次通过 Atlas 打开当前仓库，并对 scrollback layout、history search、dashboard、suggestion controller、shell completion 做了局部符号搜索，再核对源码和调用路径；没有启动全仓索引。以下记录优化候选，不改变行为契约。

## 发现

### U1 — Resume 和干净帧的 scrollback layout 仍随可见历史增长

**证据与触发。** `ScrollbackState::prepare_layout` 在缓存干净时每次渲染都会进入 clean-cache 分支，见 `crates/codegen/pager/src/scrollback/state/mod.rs:2084`。该分支无条件调用 `compute_total_height_from_cache`，对 `cache.entries[visible_entry_range()]` 求和，见 `crates/codegen/pager/src/scrollback/state/layout.rs:993`。AllTurns 模式下若可见范围覆盖整段会话，这就是每帧 O(N)，包括内容未变化的空闲帧。Resume/full-rebuild 路径也会在 `ensure_layout_cache` 后求和，见 `crates/codegen/pager/src/scrollback/state/mod.rs:2005`；随后 `settle_visible_measurements` 将屏幕内估算高度改为精确高度，并在每轮测量后重建虚拟偏移，见 `crates/codegen/pager/src/scrollback/state/layout.rs:758`。重建会重新计算相邻 gap、分组、全部 `virtual_y` 和 prompt descriptors，见 `crates/codegen/pager/src/scrollback/state/layout.rs:1083`，每轮为 O(N)。Streaming dirty 快路径 `patch_virtual_y_for_dirty` 从最早 dirty index 更新到尾部，然后扫描全部 prompt descriptors；第二轮还会对每个 dirty 项嵌套扫描 descriptors，见 `crates/codegen/pager/src/scrollback/state/layout.rs:1139`。其复杂度为 O(N-earliest + P + kP)，其中 N 是缓存项数、P 是 prompt descriptors 数、k 是实际高度变化数。以上属于同一布局维护链，合并为一个候选。

**最小优化方向。** 通过可见范围边界增量或范围求和结构维护 total height，使干净帧无需重扫；宽度、结构、分组或可见范围变化时保留完整重建。dirty height-only 路径可以按 prompt 索引更新 descriptors，或将排序后的 dirty indices 与 descriptor 做一次有序合并。保留当前惰性精确测量和 resume warm-page 策略；不能为了偏移维护而把整段历史都改成精确渲染。

**语义边界。** `visible_entry_range()` 会随 view mode 和 turn navigation 改变；末项 trailing gap 计入总高度。分组截断和 display mode 可能改变 gap 与可见性，宽度变化会使换行高度失效。Scroll anchor、sticky prompt 最小高度、follow/preserve 行为和 selection fixup 都须保持一致。多个 dirty prompt 和 prompt 自身高度变化都需要正确更新 descriptor。

**已有证据与后续验证矩阵。** 现有回归场景包括 `crates/codegen/pager/src/scrollback/state/layout.rs:2336` 的 `lazy_bulk_load_lays_out_viewport_plus_warm_pages_not_history`（保持惰性布局）、`:2986` 的 `lazy_second_prepare_layout_is_a_noop`（断言状态稳定，但未断言操作量）、`:3027` 的 `lazy_total_height_matches_independent_exact_oracle`、`:2950` 的 `lazy_measurement_window_boundaries_are_exact`、`:3169` 的 `lazy_dirty_case2_settle_measures_revealed_region`，以及 `crates/codegen/pager/src/scrollback/state/mod.rs:3754` 的 `batch_layout_matches_full_rebuild_for_mixed_history`。未来实现需在 AllTurns/SingleTurn 切换、尾部追加、较早 prompt 变更、多个 prompt 同时 dirty、fold/group truncation、宽度 resize、follow/manual anchor 下，对照现有 full-rebuild oracle 比较总高度及所有虚拟偏移；另需测量不同 N 与 dirty k/P 下 clean-frame 和更新路径的工作量。本次没有执行这些验证。

**置信度：高。** 扫描存在且位于生产布局路径；实际成本随可见历史长度和帧频增长。

### U2 — Session picker 每次绘制都会重建全部匹配项的展示数据

**证据与触发。** Welcome picker 每次绘制都会过滤传入的 session slice，见 `crates/codegen/pager/src/views/welcome/mod.rs:660`，并为全部匹配项构造 owned 展示数据，见 `crates/codegen/pager/src/views/welcome/mod.rs:668`。Resume modal 的渲染路径也重复过滤和构造，见 `crates/codegen/pager/src/app/agent_view/modal_routing.rs:2108`。`filter_session_entries` 每次扫描都会对每条目的 ID 和 summary 做小写转换，见 `crates/codegen/pager/src/views/session_picker.rs:333`；`build_session_entry_data_at` 会复制每个 summary，并给每个匹配项格式化 time-ago 文本，见 `crates/codegen/pager/src/views/session_picker.rs:583`。通用 picker 只绘制视口中的行，因此这些前置工作仍是每次重绘 O(S)，即使视口仅显示 V 项。输入和 selection reanchoring 路径也会重建完整 entry map；capture/restore 会调用 `build_entry_map`，见 `crates/codegen/pager/src/views/session_picker.rs:200`。

**上游集合限制与收益范围。** `shell/session/listing.rs` 的 `list_from_summaries` 会排序、去重后按传入 `limit` 截断，见 `crates/codegen/shell/src/session/listing.rs:98`。已核实的 unified-list 入口默认 limit 为 30，见 `crates/codegen/shell/src/session/unified_list/mod.rs:21`、`:131`；并使用 `over_fetch(limit)`，见 `crates/codegen/shell/src/session/unified_list/mod.rs:134`，其公式是 `(limit * 3).max(100)`，见 `crates/codegen/shell/src/session/listing.rs:12`。这是一个上游入口的事实；本次没有核定 pager 所有入口是否都经过该截断。因而候选的潜在收益只按实际传入 picker 的 S 计算，不据此暗示 pager 输入无界增长。

**更新频率与失效条件。** 展示数据构造位于绘制函数内，因此每次 picker paint 都运行，而不只在搜索文本变化时运行。输入过滤变化也会重建 map 和 rows；异步 content-search 结果会改变去重结果和 header 位置。宽度、选中项、展开项、时间标签、分组、当前 repo 以及远端 `entries_query` 至少会使部分结果失效。

**最小优化方向。** 保留键盘/鼠标语义所需的完整逻辑 row/index map，减少每次绘制重建 owned display strings：可以按 entries generation、effective query、width、grouping、expanded state 和粗粒度时间刷新周期缓存派生数据；或者先算完整索引映射，再只为可见行构造展示数据。是否引入缓存应由实际 S 和测量决定，避免缓存失效复杂度超过收益。

**语义边界。** `effective_filter_query` 在 server-fetched entries 与 live query 一致时有意跳过本地过滤，以免隐藏仅由内容命中的结果。分组 header、当前 repo 置顶、fuzzy/content 去重、loading header 位置、selection anchor 及 input/render 两边 `entry_map` 的索引必须一致。picker 打开期间 time-ago 标签会变化；expanded metadata 按需显示。当前可推断的收益主要是减少重复字符串处理和分配，不能推断小 S 时每次绘制都显著变慢。

**已有证据与后续验证矩阵。** `crates/codegen/pager/src/views/session_picker.rs:883` 起的测试覆盖 substring filter、effective-query、flat/grouped index map、content 去重/header、空列表/loading 和当前 repo 置顶；未覆盖不变帧是否复用或省略展示数据构造。未来变更应检查不变帧、query 编辑、width 变化、expanded/selected 变化、content-result 到达、当前 repo 变化和 time-label 跨周期，并以小/大 S 对比；同时断言行序、header 位置和 selection mapping 完全一致。本次没有执行这些验证。

**置信度：中高。** 每次 paint 的 O(S) 过滤与展示数据构造有源码证据。收益受实际传入 S 和终端重绘频率限制；已核实的一个上游入口确有 limit，但 pager 的所有入口尚未确认。

### U3 — Dashboard 为最多展示八个子 agent 而对全部子 agent 排序

**证据与触发。** `build_local_rows` 为每个本地父会话收集全部非 workflow 子 agent，按运行中优先、再按 `started_at` 倒序排序，统计总数后才截取 `MAX_VISIBLE_SUBAGENTS == 8`，见 `crates/codegen/pager/src/views/dashboard/row.rs:97`、`:228`。该逻辑经 `build_rows` 的 dashboard row 构造路径执行，见 `crates/codegen/pager/src/views/dashboard/row.rs:117`。因此每次构造行列表时，每个父会话有 M 个符合条件的子 agent 就会付出 O(M log M) 时间和 O(M) 临时引用空间，最终最多生成 8 个子 agent 行。

**最小优化方向。** 在统计所有符合条件子 agent 的同时，用有界 top-k 结构选择前八名，或用 partial-selection 再排序选中的前缀。必须检查全部符合条件的 child 后再选择前八；先截取插入顺序是错误的。

**语义边界。** 保留原优先级：未完成项在前，其后按 `started_at` 最新优先；排除带有 `workflow_run_id` 的项。当前稳定 `sort_by` 对 comparator 相等的项保留来源 `IndexMap` 顺序。top-k 替代实现须将来源 ordinal 作为显式最终 tie-break，避免相同时间戳时可见顺序变化。还需保留用于 “… N more” 的符合条件总数及当前可见子 agent 的 pinned ID。

**已有证据与后续验证矩阵。** 现有 dashboard 测试覆盖子 agent 状态分类、排除 workflow-owned 子 agent、父子 cluster，以及 filter 后保留 “more” placeholder；没有专门测试八项截断、排序后截断的选择结果或 equal-time 稳定顺序。未来优化应针对运行中/已完成混合、相同时间戳且不同插入顺序、workflow-owned 项、M 小于/等于/大于 8、可见子 agent pinned 等场景，将 top rows 和 placeholder 总数与完整稳定排序结果比较。本次没有执行这些验证。

**置信度：中。** 算法开销明确，但只有父会话累计大量历史子 agent 时才可能明显；源码定义了展示上限，未发现存储上限。应结合真实子 agent 数量或基准决定优先级。

## 已检查但未发现额外增长候选的路径

- **Prompt history search：** 搜索在后台线程执行，结果堆固定最多 100 项；dispatch 路径将 prompt history 截断为 200 项，例如 `crates/codegen/pager/src/app/root/dispatch/prompt.rs:1015`。因此完整扫描和 `HistoryEntry` 复制受现有限制，不列为可随历史增长的候选。
- **Input completion dropdown：** `SHELL_SUGGEST_WIRE_LIMIT = 50`，见 `crates/codegen/pager/src/views/suggestion_controller/mod.rs:164`；候选比较及展示受响应大小限制。本次局部审查未发现 completion 输入路径上其他随历史增长的算法。
- **Markdown streaming/replay：** 已有 deferred chunk 插入 `MarkdownContent::push_chunk_deferred`，见 `crates/codegen/pager/src/scrollback/blocks/markdown_content.rs:166`，以及 frozen-prefix 增量换行，见 `crates/codegen/pager/src/scrollback/blocks/markdown_content.rs:316`。Resume 使用 deferred scrollback mutation API，见 `crates/codegen/pager/src/scrollback/state/mod.rs:1259`、`:1315`，避免每个 replay chunk 都重新解析 markdown。宽度或主题失效时仍会全量换行，这是现有设计路径。
- **Pager-render：** 本次检查的 buffer/viewport renderer 和 terminal utility，其工作量随显示区域、选中行或固定协议数据变化。未发现超出上述三项、且输入会随 session history 增长的候选。本项是局部源码审计，不表示 `pager-render` 每个函数都经过 profiling。

## 覆盖范围与限制

覆盖 resume/scrollback layout、Markdown replay/wrapping、pager-render 表面路径、session picker 本地及内容搜索展示/index mapping、prompt-history search、shell completion dropdown、dashboard row 构造和子 agent 展示。审计重点是算法扫描、排序和每帧派生分配；未检查每个 UI widget 或工具执行路径。没有运行 build、test、benchmark 或真实会话，也没有性能遥测。session picker 的已知上游 limit 只适用于已核定的 unified-list 入口；本次没有核定 pager 其他输入入口。主审已复核 U1 的 settle/dirty 路径与 U2 的列表上游边界。
