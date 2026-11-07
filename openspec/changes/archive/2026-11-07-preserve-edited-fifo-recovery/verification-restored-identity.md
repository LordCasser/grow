# Task S · Shell restored row/runtime identity

## 状态与实现边界

本记录承接 Task T 的 canonical pending 顺序，记录 Shell 冷恢复身份修复的实际实施与验证。Task T、Task S 已分别由父核实际diff与日志后接受；两侧组合恢复链见父verification。本记录不表示整个campaign完成，无 commit/release/archive。

本轮 Rust 写入只涉及 `crates/codegen/shell/src/session/actor/input_admission.rs`：`restore_pending_human_inputs` 的 FIFO 分支在构造 `queue_meta` 后，以 `queue_meta.id`（即 `StoredQueueEntry.id`）作为恢复的 `InputItem.prompt_id`。该文件此前已有同一 owner 的 F1/F3 改动；本轮的净变化是这处恢复身份映射和同文件新增的 cold-restore 测试，未改 `prompt_queue.rs`、ChatState、Pager、schema 或 manifest。

契约保持不变：

- 有 `StoredQueueEntry` 时，runtime/展示行身份取持久队列行 id；编辑请求的 `payload.prompt_id`（`original:edit:N`）、`input_id`、`payload_ref` 与哈希仍是不可变接纳身份，不参与运行行身份。
- 正文、图片、client、screen、verbatim、json_schema、版本仍从 canonical pending 指向的最新 immutable payload 读取；Shell 不扫描 raw 日志、不排序。
- 无 queue 的既有普通输入继续以 `payload.prompt_id` 作为默认行 id（fallback 构造的 `queue_meta.id` 与其一致）。
- F1 `preservedTurn` 的 Steer 恢复分支在该映射之前，保持不变；Timeline `RouteSuperseded`、Hook/成员过滤与 validator 未改。

## 实际旧红

环境为 macOS aarch64，Rust `+1.93.1`，`--locked --offline`；命令在仓库根执行：

```sh
env -u CARGO_TARGET_DIR \
  PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 test --locked --offline -p shell --lib \
  edited_queue_rows_restore_row_identity_from_latest_immutable_payload
```

未修改生产代码时实际失败（`/tmp/grow-fifo-identity-oldred.log`，exit 101）：只经过真实 `queue_input` 与 `handle_queue_control(Hold→Save)` 产生 A/B/C（A 连续编辑两次、B 一次），销毁 live actor 与 adapter 后从真实 JSONL 冷重建并 `restore_pending_human_inputs()`，canonical 顺序、`queue_meta.id`、`input_ids` 均正确，但恢复出的 runtime prompt id 是 `queue-a:edit:3` / `queue-b:edit:2`，期望 `queue-a` / `queue-b`。失败断言位于最终行身份比较，不是编译错误或 fixture 前提错误。

fixture 断言按现 producer 实际输出使用 `queue-a:edit:3`、`queue-b:edit:2`（`replace_queued_prompt_with_admitted_input` 在 `apply_queued_prompt_edit` 递增版本后再拼 `version + 1`）。这是测试对既有不可变身份规则的适配，本轮不改变编辑编号政策，也不把该 off-by-one 当作产品缺陷。

## 新绿与回归

最终源码实际结果：

| 命令 | 结果 |
| --- | --- |
| `cargo +1.93.1 test --locked --offline -p shell --lib edited_queue_rows_restore_row_identity_from_latest_immutable_payload` | exit 0；1 passed、0 failed。 |
| `cargo +1.93.1 test --locked --offline -p shell --lib session::actor::input_admission::tests::` | exit 0；19 passed、0 failed，含 F3 稳定重试/冲突/观察、interrupted admission、physical writer 子进程切点、large input recovery、缺失附件失效与 queue retry。 |
| `cargo +1.93.1 test --locked --offline -p shell --lib restore_steer_preserved_tests::` | exit 0；2 passed，F1 preserved/closed Steer 恢复未回归。 |
| `cargo +1.93.1 test --locked --offline -p shell --lib follow_up_admission_tests::` | exit 0；16 passed，queue hold/save/remove、拒绝替换保留原行、合并与提升未回归。 |
| `rustfmt +1.93.1 --edition 2024 --check --config skip_children=true crates/codegen/shell/src/session/actor/input_admission.rs` | exit 0。 |
| `git diff --check -- crates/codegen/shell/src/session/actor/input_admission.rs` | exit 0。 |
| `openspec validate --all --strict --no-interactive` | exit 0；22 passed、0 failed。 |

证据：`/tmp/grow-fifo-identity-oldred.log`（有效旧红）、`/tmp/grow-fifo-identity-green.log`（最终源码新绿）、`/tmp/grow-fifo-identity-regress.log`（input admission 全组）、`/tmp/grow-fifo-identity-check.log`（格式、diff、F1/queueedit 与 strict spec）。green 在最终格式化后的源码上重跑，oldred 日志保留未覆盖。

## 覆盖与限制

新增测试走实际入口：`queue_input` 做三次真实 durable admission，`handle_queue_control` 的 Hold→Save 做 A 两次、B 一次编辑；每次提交都经真实 `JsonlStorageAdapter::append_timeline_event_durable` 与 ACK。随后销毁整个 live actor（含其 persistence clone），释放 adapter writer lease，用 `load_session_for_write_without_updates` + `ChatStateActor::spawn_from_validated_timeline` 重建 owner，再调用真实 `restore_pending_human_inputs`。

断言覆盖：cold query 与队列顺序 A/B/C；运行/展示 `prompt_id`、`queue_meta.id`、版本 2/1/0、最新正文；A 的图片在编辑与冷恢复后保留；client_identifier、screen_mode、verbatim、json_schema 保留；canonical pending 与物化 payload 读取最新版 `queue-a:edit:3` / `queue-b:edit:2` / `queue-c`，`StoredQueueEntry.id/version/text` 与行一致；A 与 B 的旧输入为 `Dismissed(RouteSuperseded)`；恢复不新增任何 Timeline 事件，每个输入 `UserPromptSubmit` Hook 恰好一次；随后用真实 `TurnEvent::Started` 预留并经 `consume_fifo_inputs` 只消费恢复行一次，pending 缩为 B/C 且 A′ 为 `Consumed(Fifo)`。

未覆盖/限制：

- 未新增 unknown-ACK 子进程测试；按父范围复用既有 `physical_writer_replacement_replays_stable_input_identity_once` 与 F3 stable retry 回归，不把 failed ACK 当未写。
- 冷启动为同进程内真实 JSONL 文件重建 + 新 writer load，不是独立 OS 进程；生产 spawn 在 restore 前还会执行 `recover_interrupted_durably`，本测试直接调用 restore，未重放该步骤。
- 未验证手动 reorder 的重启持久化（独立政策，本项不承诺）、真实 Pager 交互、PTY、其他平台或全 workspace 全量测试。
- 图片保真使用现 support 测试 PNG 经真实 payload manifest 往返；未使用其他图片格式或外部附件。
