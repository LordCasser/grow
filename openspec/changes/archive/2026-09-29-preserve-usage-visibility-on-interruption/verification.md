## Planning verification

立项阶段（2026-09-29）的证据。以下运行验证另记于本文件，不把立项时的源码推断当作运行结论。

- 已核对现行 `client-surfaces`、`model-sampling` 与 `session-timeline` 契约；明确普通状态栏 N/A 符合旧契约，本项需要修改该要求。
- 已核对会话计数与两次 steered/unknown settlement；未把 provider 返回未知与本地可能丢失已确认 usage 混为同一原因。
- 已核对采样取消、Chat 尾部等待、Messages/Responses 完整性边界、usage sink、ledger 恢复及 Pager 投影/测试。
- `openspec list --json` 在立项前返回无 active change；工作区已有的其他未提交修改不属于本项。
- `openspec validate --all --strict --no-interactive`：通过，16 项通过、0 失败（包含本 change 与当时工作区内另一项并行 change）。
- `openspec status --change preserve-usage-visibility-on-interruption`：4/4 planning artifacts 完整；仅表示提案、delta、设计和任务齐备，不表示实现完成。
- 文档空白检查通过，8 个文件中没有行尾空白；实施任务已勾选数为 0。
- MODIFIED delta 保留原要求的全部 5 个场景标题，并在相应场景内明确修改比例展示规则。
- Luna 只读复核通过：确认 known/unknown 与预算边界、Messages 完整性、measured 分母与窄屏/恢复语义、任务未虚假完成；未发现需修订的具体问题。

## Runtime verification

实施阶段采用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216` 限制 Rust 构建并发。测试读取当前工作区；许多其他未提交改动不属于本 change。

- Sampler 的受控流回归 `chat_usage_confirmed_before_cancel_survives_pending_tail` 通过：finish 后 usage 已解析但尾部不结束时可取消；只有 finish 或只有中间 usage 时 slot 保持未知。`final_chunk_usage_is_confirmed_before_its_preview_yields` 通过：同帧的文本、finish 和完整 usage 在第一次 preview yield 前确认。旧版 `AttemptOutcome::Cancelled → None` 的失败路径见本 change 的 `analysis.md`，未声称运行过改动前的红测。
- 假 HTTP/SSE 到真实 `AttemptUsageSink` 的 `confirmed_chat_usage_settles_once_when_request_is_cancelled` 和 `...when_logical_deadline_expires` 均通过：确认槽作为同步屏障，不依赖任意 sleep；原 attempt 只结算一次 Known，保留 120 tokens、read=90 与 cost=42，请求不接纳候选、不重试，尾部保持 pending。sink 暂停 ACK 时请求仍 pending，放行后才结束，也未追加 Unknown。命令：`cargo test --locked -p sampler --lib confirmed_chat_usage_settles_once -- --test-threads=1`，2 passed。
- 最终 `cargo test --locked -p sampler --lib --quiet -- --test-threads=4`：254 passed、0 failed。覆盖 Buffered terminal 与 cancel 同时 ready、未准入取消、Messages/Responses 终态完整性、attempt 隔离和未知 output grant 限制。
- `cargo test --locked -p chat-state --lib known_cache_samples_survive_unknown_attempt_and_cold_restore -- --test-threads=1`：1 passed。A=80/100、连续两个未知 attempt、B=810/900 后，lifetime 账本为 read=890、measured input=1000、incomplete=true；重复身份与冷恢复不重计。
- `cargo test --locked -p chat-state --lib --quiet -- --test-threads=4`：527 passed、2 ignored、0 failed。覆盖原有 child fold、ACK、resume 和完整性回归。
- 最终 `cargo test --locked -p shell --lib sampling_usage_sink -- --test-threads=2`：2 passed，Known 结算去重与 output grant 仅扣一次，Unknown 耗尽 grant。直接运行该次构建的 Shell 测试二进制，`session_usage_projection_is_transient_and_matches_usage_query` 与 `late_unknown_usage_preserves_stopped_goal_status`：各 1 passed。直接运行复用大体积二进制。
- Pager 首次 `cargo test --locked -p pager --lib views::agent_status::tests` 被其他未提交 `transcript_projection/tests.rs:31` 的 non-exhaustive struct 字面量编译错误阻断；该文件随后由工作区其他修改修正，本 change 未改动它。最终同命令重跑：4 passed、0 failed，包括 89.00% 快照、缺失/零值、非法计数与窄屏。直接运行本次 Pager 测试二进制，累计快照替换 1 passed、部分 cache coverage 1 passed、Usage/Goal 点击 3 passed；详情格式与费用未知测试在此前构建中通过，相关生产代码其后未改。
- 四个本 change 修改的 Rust 文件的 `rustfmt --check --edition 2024` 与相关文件的 `git diff --check`：均通过。
- `openspec validate --all --strict --no-interactive`：16 passed、0 failed，包含本 change 与并行存在的其他 change。场景逐项对照：Chat pending 尾部、同帧 preview、deadline、未知/中间 usage、ACK、attempt 隔离，及状态栏完整/部分/缺失、恢复、窄屏与点击均有上述测试或既有组件回归；没有更改既有持久化格式或预算语义。
- Rust 测试结束且无 Cargo/rustc 进程后执行 `cargo clean`：移除 132,839 个编译文件、37.5 GiB；可用磁盘从约 21 GiB 恢复到 53 GiB。随后只执行 OpenSpec 文档操作，不再生成 Cargo 产物。

## Archive verification

- `openspec archive preserve-usage-visibility-on-interruption --yes` 将本 change 归档为 `2026-09-29-preserve-usage-visibility-on-interruption`，更新 `client-surfaces` 的原要求并新增 `model-sampling` 的取消用量要求。归档当时 4.3 尚未勾选，CLI 在 `--yes` 下提示 10/11；待规范合并和开发者链接更新完成后已勾选。
- 开发指南与 `/usage` 用户说明现链接正式 `openspec/specs/` 契约；检查到两个 requirement 标题与新增同帧场景均在主规范中。
- 归档后 `openspec validate --all --strict --no-interactive`：14 passed、0 failed。`openspec validate --archived --no-interactive`：555 passed、0 failed（含本 change）。相关文件的 `git diff --check` 再次通过。
- 上述验证针对本次实现与当时共享工作区的并行修改；没有把别的 change 的文件纳入本 change 任务或测试结论。
