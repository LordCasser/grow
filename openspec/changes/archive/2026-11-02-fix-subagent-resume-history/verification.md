## Reproduction

- 用户确认：运行中的 child 随主会话退出挂起，再 `grow resume` 自动接续，使用原 session identity。
- 新回归 `subagent_resume_restores_history_before_first_live_update` 在修复前失败：首条 Thinking 后任务 prompt 数量为 0，期望为 1。
- 新回归 `subagent_resume_deduplicates_persisted_live_overlap` 在初版修复后暴露重复工具记录：同一已持久通知随后实时投递时得到 2 行，期望为 1；已以既有 ACP 高水位消除。

## Final validation

- `RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib app::acp_handler::tests -- --test-threads=4`：370 passed，3 ignored，0 failed。覆盖六种首次实时更新/完成时序、加载期重复 spawn、首次打开及重开、五种相同持久记录的实时重叠、新通知继续展示，以及既有纯 replay 延迟加载和控制恢复。
- `RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib app::subagent::tests -- --test-threads=4`：47 passed，0 failed。包括真实 canonical response journal 的 Reasoning + Assistant 重放、同 request/attempt 迟到预览拒绝，以及后续不同 request 正常显示。
- 较早的 `cargo test --locked -p pager --lib subagent -- --test-threads=4`：193 passed，0 failed。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- 默认 `cargo check --locked -p cli --bin grow` 失败于 `tikv-jemalloc-sys` 的 C 构建：缺少生成头文件 `jemalloc_internal_defs.h`；`cargo clean -p tikv-jemalloc-sys` 后重试相同失败。未修改依赖或 allocator 配置源码。
- `cargo check --locked -p cli --bin grow --no-default-features --features sandbox-enforce`：通过，验证保留 sandbox 的 CLI 生产依赖图；不将其描述为默认 jemalloc 构建通过。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed（归档前）。
- `openspec archive fix-subagent-resume-history --yes`：完成，新增 client-surfaces Requirement 已合入主规范；归档后全量 strict 与 `openspec validate --archived --no-interactive` 均通过。
- 验证后 `cargo clean -p pager -p tikv-jemalloc-sys` 清理本次相关编译残留，释放约 5.0 GiB。

## Adversarial review

- 收到实时内容前先恢复历史，不移除既有“已有正文不重复加载”的保护，也不将全部 child transcript 提前加载。
- root 尚在加载时若 child 已完成历史恢复，live terminal 仍完成一次 footer；未恢复的 child 沿原 deferred open 路径收束。历史 replay 不被误当成新的 live terminal。
- ACP 与 Grow 水位分离。`JsonlStorageAdapter::load_light_data` 的 writer 恢复调用 `HistorySnapshot::reseed_event_counter`，新 transport event ID 高于旧 presentation IDs。canonical response 不含 transport eventId，按已验证的 request/attempt 使用 tracker 原 Accepted 状态处理，未混用 domain sequence。
- User message 沿 messageId 保持唯一；工具/普通文本按 transport event 水位处理；已接纳模型文本按 attempt 处理。新事件不被重放水位永久阻断。
- 缺少 child journal 的既有 best-effort 行为不变；相关 no-op 用例通过。未运行真实终端进程退出/自动续跑的 PTY 场景，本次验证覆盖真实磁盘 journal 与生产 Pager 通知路由。
- 审计另发现 Task `resume_from` 的派生展示继承缺口，已单独登记 backlog，不混入当前修复。
