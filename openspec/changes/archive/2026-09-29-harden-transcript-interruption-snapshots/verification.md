# 验证记录

环境：macOS arm64，Rust 1.98.1，本地工作区。为避免和同目录的另一项 usage/interruption 工作争用或清理构建产物，本次后续编译使用独立 `CARGO_TARGET_DIR=/tmp/grow-transcript-audit-target`，`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4`。没有提交、覆盖或回滚其他任务的修改。

## 自动验证

| 命令 / 等效 libtest filter | 结果 |
| --- | --- |
| `cargo test --locked -p shell --lib session::storage::transcript::tests` | 16 passed，包含 11 个本次追加的对抗性用例 |
| Shell libtest `committed_jsonl_reader` | 3 passed，涵盖未设置捕获上界的原读源行为 |
| Shell libtest `response_projection::tests` | 17 passed |
| `cargo test --locked -p pager --lib transcript_projection` | 最终版本 7 passed（含可见/隐藏重复 echo） |
| Pager libtest `replay_cmd::tests` | 14 passed |
| Pager libtest `export_cmd::tests` | 6 passed，含 CLI 目录与 trajectory tar 的内容 parity |
| Pager libtest `acp::tracker::tests` | 161 passed |
| Pager libtest `app::acp_handler::tests` | 337 passed，1 ignored（已有性能探针） |
| `cargo build --locked -p cli --bin grow` | 最终版本通过 |
| 本次修改 Rust 文件的 `rustfmt --edition 2024 --config skip_children=true --check ...` | 通过 |
| `git diff --check` | 通过 |
| `openspec validate --all --strict --no-interactive` | 归档前 16 items passed |

libtest filter 在上述 Cargo 生成的可执行文件上直接运行，避免重复构建无关 feature graph。未运行整个 workspace 全量测试。链接器报告现有大型 Rust 可执行文件的 `__eh_frame` 超过 16 MiB 警告；编译及测试退出码均为 0。新增测试初次编译中的 non-exhaustive struct 构造与 unwrap_err 的 Debug bound 错误已修复，失败日志不计入通过数。

## 真实 CLI / PTY

运行本 change 内的可重现脚本：

```sh
python3 openspec/changes/harden-transcript-interruption-snapshots/verify_cli.py /tmp/grow-transcript-audit-target/debug/grow
```

初次与最终版本均通过。脚本创建隔离 Grow home 与合法 updates fixture，不使用 provider。归档后可使用本目录下的 [verify_cli.py](verify_cli.py)，参数指向重新构建的 grow 可执行文件。覆盖：

- 默认输出目录、取消后直接新输入、旧 terminal 晚到并重复、未闭合工具的 Markdown 标记。
- 已存在目录再次导出失败，已发布内容不变。
- 真实终端以 100× 播放，完成后仍可阅读。
- bracketed paste 中的 `q /help + -` 不退出；独立 q 正常退出，raw mode、alternate screen 和 paste mode 恢复。
- session 文件前后 SHA-256 完全一致；记录中的 `touch <sentinel>` 从未执行。

这一 fixture 的 Timeline 为空，只用于验证真实 CLI/终端整条展示链路；canonical admission、rewind、真实 Hook 与 receipt Timeline 使用 Rust 存储 fixtures 单独验证，不将合成 PTY 等同于真实 provider 运行。

## Delta 场景核对

| OpenSpec 场景 | 证据 |
| --- | --- |
| User cancels and continues | cancelled_tool / cross_turn_tool + CLI fixture |
| Earlier terminal arrives during another turn | late_old_terminal 的 EntryId、running 与 AB 连续正文断言 |
| Background work finishes after a turn | background_task 的原 EntryId、开始/结束分离及晚到前台更新抑制 |
| Persisted user echo is repeated during its response | duplicate_user_echo，包含可见/隐藏两种输入 |
| Passive history requires an existing fact projection | Hook / captured child spawn / parent receipt source tests；通信正文 projection test |
| A retained message receipt has no readable body | parent receipt 测试删除 artifact 后仍保留 unavailable 回执 |
| Writer appends after capture | 半行补齐、之后完整坏行、新消息及新 child 不进入捕获 |
| Committed prefix is invalid | JSONL 原回归、foreign session、updates/Timeline 截断、missing/empty display |
| Canonical response replaces a preview anchor | admitted attempt 文本、promptId 与原始时间均保留，其他 attempt 排除 |
| Cancellation arrives during reveal | cancellation/new_user/tool boundary 的到期 tick 断言；pause/speed 交互 |
| Paste contains playback keys | Paste 单事件单元用例 + 实际 PTY |

## 限制

详见 [audit.md](audit.md) 的未闭合边界。没有执行真实远端模型、多进程文件故障注入、跨平台 Windows/Linux 终端、最大深度/节点/条数的极限负载，也不承诺部分丢失 cache 的完整恢复。测试支持自然中断场景的已记录事实展示，不支持“所有并发历史可精确重演”的结论。

## 收尾

最终 CLI/PTY 复验通过。已逐项核对上述 delta 场景；归档前全量严格校验通过。`CARGO_TARGET_DIR=/tmp/grow-transcript-audit-target cargo clean` 已完成，删除 15451 个文件、8.2 GiB。本工作区默认 target 正被另一项任务使用，未清理它。

已归档为 `2026-09-29-harden-transcript-interruption-snapshots`，3 条新增 requirement 合入 client-surfaces。归档后 `openspec validate --all --strict --no-interactive` 为 15 passed，`openspec validate --archived --no-interactive` 为 554 passed，均无失败；`git diff --check` 通过。
