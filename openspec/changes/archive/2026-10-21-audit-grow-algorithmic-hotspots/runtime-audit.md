# 存储、搜索和运行时算法审计

本次只读分析，以 HEAD `a1d1c2965075c87645eab22adce6d48b19410c59` 为代码基线。范围包括 session 存储、列表、搜索更新，以及 workflow、prompt queue、coordination、config/MCP/hooks 的代表性入口。没有运行真实会话、构建或性能测试。

## S1：exact commit 每次重新读取并解码整份 updates

`raw_acp_event_status` 与 `raw_projection_status`（`crates/codegen/shell/src/session/storage/jsonl/mod.rs:2902,2948`）每次调用 `read_updates_from_directory`，再扫描全部记录判断 Missing / Exact / Conflict。`append_acp_event_exact`、`commit_response_projection`（`:5405,5463`）在追加前检查一次，新追加成功后再检查一次，错误恢复也会重查。

这是生产路径：`PersistenceActor::commit_response_projection`（`crates/codegen/shell/src/session/persistence.rs:2194`）在 response projection 提交时调用；sampling window 中的独立 ACP 更新也走 exact append（`:2287-2303`）。不是所有普通 ACP chunk 都走它，普通通知已有合并路径。

设 B_i 为第 i 次提交前的 updates 字节数，K 为 exact 提交次数。读取/解码成本为 O(ΣB_i)，等长记录逐步增长时可能表现为平方累计成本；每次还有完整 typed Vec 的峰值内存。这里是同步读取，虽位于 async 函数中也不会自动让出 executor。证据置信度高，实际耗时占比待测。

最小方向是让持有 writer lease 的存储 owner 保留已验证 physical prefix 的身份索引：ACP eventId 与 projection 的 `(request_id, attempt)` 映射到比较所需信息和记录位置；首次读取建立索引，后续只处理确认后的新 suffix。已有 exact body 比较仍保留，不能用缓存命中代替 file/directory sync。追加结果不明时在固定目录能力下核对本次 suffix，必要时全量重建。

需要先明确所有 append 入口、目录/文件身份、长度与更改戳、尾部截断、外部替换、writer 切换和回收时如何使索引失效。只缓存“见过 id”会漏掉同 identity 的冲突；只检查长度也会漏掉同长度替换。已有 Timeline prefix cache 是参考，不能直接把它当 updates 的安全证明。

验证入口：`storage/jsonl/durable_tests.rs` 的 `exact_projection_is_physically_idempotent_and_conflicts_on_payload`、`exact_acp_event_is_physically_idempotent_and_conflicts_on_payload`、post-commit bookkeeping failure 以及 file/directory sync failure retry。增加普通 append 与 exact append 混排、旧前缀冲突、torn tail、文件替换、重开 writer、同步失败后 exact 重试。契约见 `session-timeline` 的 Exact replay commits acknowledge durable barriers 和 physical identity seed。

## S2：搜索增量更新仍然全量构建 Timeline

`upsert_session`（`crates/codegen/shell/src/session/storage/search.rs:1047`）先打开 Timeline reader；`collect_timeline_indexable_content`（`:1466`）读完整 events 并 `Timeline::from_events`，再生成搜索内容。只有此后才查询 existing content hash，决定是否写 FTS。因此“内容没变”只省下索引写入，没有省下 ledger 读取、验证、fold、branch transcript 构造。

调用来自 durable Messages/Consumed/Turn Started/Title/Subagent/Result 等变更（`crates/codegen/shell/src/session/persistence.rs:2531-2554`）。worker 已有按 session 去重、500 ms debounce，bootstrap 也有限并发/超时/大小策略；不能描述成每条流式 token 都全量索引。设 K 为实际 flush 次数，每次成本约为 O(B_i + F(E_i) + 输出构造)，F 是当前 Timeline fold 成本。后台处理可能争用 CPU/I/O，是否影响前台需测量。

分两步考虑。先区分只变 title/summary 与搜索正文相关的提交，复用同一已验证内容投影，避免无关事件引发重建。再考虑在现有 live owner 内按 accepted event 更新搜索投影；普通追加只处理新增相关事实，rewind、分支替换、compaction 等走显式重建。是否复用 live actor，还是在 bounded worker 中保存单个活跃会话状态，需要根据所有权和 RSS 测量选择，不能给每个历史 session 永久缓存完整 Timeline。

搜索内容来自 selected branch 的 uncompressed transcript 与 prompt identities，不是 ACP replay 或当前压缩 Surface；还要保留 assistant/tool 截取、用户文本后缀、title、目录实体绑定、schema/cache epoch、跨进程 lease 和删除语义。只有文件长度/mtime 没变不够证明内容未变，也不能因先前索引成功而跳过冷 bootstrap 的必要校验。改进必须是可重建的派生索引。

现有验证入口：`timeline_collector_reports_bytes_and_validates_lifecycle`、`timeline_collector_reads_the_pinned_entity_after_namespace_replacement`、`timeline_failure_preserves_row_and_recheck_indexes_repaired_content`、`test_recheck_bootstrap_reruns_reindex_when_marker_missing`，以及 busy eviction/lease 测试。后续做同一事件序列的全量/增量文档差分，覆盖 rewind、title-only、超过 200 kB 内容上限、bootstrap 与实时更新交错。置信度：全量重复工作高；方案复杂度和收益中，排在 resume 和 S1 之后。

## S3：session 列表先转换、排序全部 summary，再截取少量结果

`list_from_summaries`（`crates/codegen/shell/src/session/listing.rs:99`）过滤后为所有 summary 生成 `SessionListing`，把 typed timestamp 格式化成字符串；`effective_sort_time`（`:158`）又把字符串解析为时间排序。随后按 cwd 去重空 session，最后 truncate。`build_unified_list`（`unified_list/mod.rs:123`）默认 limit 30、over-fetch 至少 100，返回数量小不代表前面的扫描与转换小。

S 为收集的 summary 数，L 为 over-fetch limit：当前有 O(S log S) 排序及 O(S) 字符串生成，summary 磁盘发现仍然必要。可以先对 typed Summary 计算稳定排序键、为每个 normalized cwd 选出排序最前的空 session，再用标准库 partial selection/top-L，最后只转换选中项。选择后排序约 O(S + L log L)（或 heap O(S log L)），format/parse 往返可以移除。先做 typed key + 延后转换是更小的改动，未必需要同时换选择算法。

必须先按 query 过滤再做相同 empty-session 去重，保持 effective time 降序、session id 次序、重复相等键的稳定来源顺序、cwd normalize、worktree label、facet 与 cursor 的既有 over-fetch 行为。不能先截 L 再去重，否则数量和内容会变。最终分页正确性和目录发现并不因 top-L 而自动改善，本条不顺带修改列表契约。

测试入口：`listing_filters_and_keeps_only_newest_empty_session_per_cwd` 和 `unified_list` 的过滤/分页用例；增加 query 后同 cwd 多空 session、同时间 tie、无 last_active、limit 0、relaxed repo scope。置信度高，但是否值得 top-L 取决于实际 S，优先级低于 R1/R2/U1/S1。

## 其他路径与排除理由

| 路径 | 本次核对与结论 |
| --- | --- |
| Timeline append | `storage/jsonl/mod.rs:1698-1794` 已有 prefix hasher、长度和更改戳缓存；正常追加无需重新 hash 全历史，stamp 不一致才校验前缀。不能把 S1 推广成所有持久写都全量读取。 |
| Prompt blobs / sampling evidence | `crates/codegen/shell/src/session/persistence.rs:949` 与 `crates/codegen/shell/src/session/sampling_evidence.rs:126` 已去重引用。被引用 blob 的完整性读取是现有契约，不为省 I/O 删除；重开 artifact 目录等常数优化暂不立项。 |
| Session FTS 查询 | `storage/search_fts.rs:537-600` 已用 SQLite FTS，LIMIT/OFFSET 深分页或 count 成本要有实际查询分布才值得改。本轮不建议另建搜索引擎。 |
| Workflow journal / restore | `workflow/src/journal.rs:226-250` 以 seq 索引 Vec，校验 kind/hash；restore 读取有字节上限。run restore 最多 128 个有效 run；不能在过滤缺失/损坏候选前直接截断 128。冻结配置和 journal hash 关系到确定性，不因 clone 就认定多余。 |
| Prompt queue | actor 使用 VecDeque，队列编辑为保序重建；wire snapshot 的线性成本与输出大小相符。未建立高频大队列证据，不预建第二份索引。只核对队列容器/编辑入口，未穷举所有控制流。 |
| Local coordination | session/inquiry 以 HashMap 按身份查找；查询前 TTL retain 是线性清理。现有队列边界和 terminal TTL 不等于全进程固定记录数上限，但没有大规模 inquiry 频率证据，不先增加 expiry heap。握手和 exactly-once 边界保留。 |
| Config / MCP / hooks | config watcher 已 debounce 并过滤 access 事件；MCP dispatcher 以 server/episode 的 HashMap/HashSet 合并事件、撤销被替代的 catalog task；hook matcher 在 registry 中编译，dispatch 复用。没有确认新增增长热点。仅覆盖 watcher、dispatcher、hook planning，未 profile 网络初始化/插件进程。 |

核对规范包括 `session-timeline`、`workflow-execution`、`local-coordination`、`configuration-rules`、`extension-runtime`。未深入 sandbox/OS 进程隔离、第三方 SQLite/向量库内部、provider SDK、release/build toolchain、所有图片/终端协议实现；这些不在本轮算法候选结论之内。
