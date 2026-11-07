# Design

## Facts

- `end_prompt`（workspace `session/file_state.rs`）先短锁收集 `file_snapshots` keys，`buffer_unordered(8)` 读全部触及路径，末尾再短锁写入；`.unwrap_or(None)` 把 read 错误与 UTF-8 失败都折叠成 `None`（确认不存在），且整轮无任何期限。调用方 `tasks_cancel.rs`×2、`turn/admission.rs`×2 都不加期限——期限必须在 owner 内。
- `tokio::time::timeout_at` 接受共享绝对 `Instant`：每个读取 future 包一层 `timeout_at(deadline, read)`，到期即 `Err(Elapsed)`；`buffer_unordered(8)` 消费所有已 ready 项后自然结束。相比 `take_until`（deadline 触发时可能丢失已产出未消费的项），`timeout_at` 让每条路径都解析为三态结果，不丢预算内的成功项。
- `AsyncFsWrapper` 是 `Clone`（内部 `Arc<dyn AsyncFileSystem>`）；`try_read_file` 的 `Ok(None)` 仅表示确认不存在；`bytes_to_string` 可区分 UTF-8 失败。
- shell 预览（`session/actor/rewind.rs`）：`rev().find_map(|p| p.after_snapshots.get(path))` 跳过缺失条目的 point 落到更早 snapshot；`and_then(content)` 再把 missing 折叠成 `None` 与 current `None` 误判 clean。`files_to_revert` 由 `file_snapshots` 构建，故"最新触及 point"必存在（缺失属防御分支）。
- pager `ConflictDisplay::from_conflict` 对未知 `conflict_type` 已有 `_ => "conflict"` 回退，新增 `snapshot_unavailable` 不需要改 UI（也在本项范围外）。
- `merge_rewind_points_from` 是纯函数：before `or_insert`（earliest wins）、after `insert`（latest wins）；无 predecessor 时 merged 效果整体丢弃（既有行为，不变）。
- 未知即 after 条目缺失：serde（HashMap 序列化）、persistence JSONL、lazy 读取与 metadata 扫描天然保留，无 schema 变化。

## Decision

- **捕获（workspace）**：`end_prompt` 公共签名不变，委托 `#[cfg(test)] end_prompt_with_budget(fs, index, END_PROMPT_AFTER_CAPTURE_BUDGET)`；固定常量 5 秒，不加用户配置。预算覆盖整轮：`Instant::now() + budget` 一次计算，`map(读).buffer_unordered(8).filter_map(三态).collect`，deadline 包含排队与读取等待；8 路并发不变。到期后已 ready 项照常写入，in-flight future 被 drop——注释明确：drop 不终止底层操作（如 ACP 读可能在远端完成），只是不再观察、绝无后台晚写。
- **三态映射**：`Ok(Some(bytes))`+UTF-8 成功 → `Some(content)`；`Ok(None)` → `None`（仅此含义表示确认不存在）；read 错误 / UTF-8 失败 / `Elapsed` → 不产出条目（未知）。
- **重试清旧**：收集路径的同一短锁内先 `after_snapshots.clear()`（after ⊆ 触及路径，全清安全），新捕获只在最终短锁写入成功项；这样重试与中断轮次都不会残留旧 after。
- **锁沿现 owner**：收集/清理短锁 + 写入短锁，不跨 await 持全 map。预算约束的只是读取观察阶段（deadline 后 collect 不再等待任何 read）；随后的写入短锁只操作内存 map、不含 I/O，但其耗时不在本预算的声明范围内，也不声称任何锁或底层同步 I/O 可被强行终止。
- **消费（shell）**：`latest = all_points.iter().rev().find(|p| p.file_snapshots.contains_key(path))`，只看 `latest.after_snapshots.get(path)`：`Some(snapshot)` → 既有 clean/冲突判定（`None==None` 仍 clean）；`None` → `conflict_type "snapshot_unavailable"`（常量放 rewind.rs），不进 clean；防御分支（无触及 point）同 unknown。preview `error` 文案按冲突构成区分 unknown / 外部修改 / 两者并存，不把 unknown 说成外部修改。
- **merge**：折叠每轮先对触及 path `previous.after_snapshots.remove(&path)` 再 `or_insert` before，后插该轮 after。多轮按序折叠后，每 path 的 after 由最后一个触及轮决定：未知轮清空、已知轮覆盖——前轮陈旧已知 after 不得穿透未知轮。
- **force 不变**：`force=true` 仍走既有提交（含恢复 before 内容）；unknown 只影响 preview 呈现，不改提交流程与审批。

## Alternatives rejected

- 每 read 单独 timeout 常量：总期限不可组合，逐个限时仍可能整体超预算；总 deadline 是需求本身。
- `take_until` 包整条流：deadline 竞态可能丢弃已产出未消费的成功项；`timeout_at` 共享绝对 deadline 语义更精确。
- 读错误记错误串或新增 conflict 字段 / version：schema 已能表达 unknown（条目缺失），不加持久实体。
- 把超时/错误当文件不存在（现状 `unwrap_or(None)`）：backlog 明确禁止。
- 到期后 spawn 后台补写：违反"超时后不得后台晚写点"。
- shell 侧对 unknown 路径静默跳过冲突或自动判 clean：违反消费端契约；恢复 before 属 force 既有语义，preview 必须如实呈现 unknown。

## Tests

workspace `file_state.rs` 内嵌 tests：受控 `AsyncFileSystem`（成功 / 确认不存在 / read 错误 / 非 UTF-8 / 永不返回但 sender 存活）验证小预算整体 return、成功项保留、未知不插条目、sender 存活计数；公共路径 `end_prompt` + 外层 watchdog 证明固定预算（对旧实现为 red）；重试清旧 after；merge 未知轮删除前轮 after（含多轮顺序）；unknown 的 serde 与 lazy 往返。shell 新增 `actor/tests/rewind_unknown_snapshot_tests.rs`（注册于 `tests/mod.rs`）：经真实 `handle_rewind` 预览断言 `snapshot_unavailable` 不进 clean、文件存在与否无关、提示文案；`force=true` 执行与文件恢复不变；ConversationOnly 合并后同 path unknown 且 preview 不用旧 after。本机不运行 Cargo；测试由父在共享 target 窗口执行，旧行为 red 证据随窗口记录。