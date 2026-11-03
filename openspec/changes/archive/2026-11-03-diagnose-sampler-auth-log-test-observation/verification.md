# Verification

- Added `captured logs: {logs}` as the assertion message for the five positive log-content checks in `sampling_auth_logs_omit_credentials`. HTTP wire-header checks and credential-absence checks are unchanged.
- Ran the full sampler library suite once with its default parallel test scheduling:
  `GROW_HOME=/tmp/grow-home-sampler-diagnostics-2026-11-03 RUST_MIN_STACK=16777216 cargo test -p sampler --lib`
- Result: **259 passed, 0 failed, 0 ignored**, 12.64 seconds. The target test passed; the suite did not reproduce the missing-log observation.
- Raw output: `/tmp/grow-backlog-sampler-diagnostics.txt`.

The historical failure's root cause remains unknown because its original raw logs were not retained. This run does not reproduce the issue and does not establish that the flake is fixed. The new assertion messages preserve the captured output if a required field is missing in a later run.

正式补跑：`RUST_MIN_STACK=16777216 GROW_HOME=$(mktemp -d ...) cargo test --locked -p sampler --lib`，259 passed / 0 failed / 0 ignored，12.62秒；原始日志 `/tmp/grow-backlog-sampler-diagnostics-locked.txt`。先前未带--locked的真实记录保留；Cargo.lock未修改。

实现前只读调查也运行默认平行259/259、32线程259/259（`/tmp/grow-backlog-sampler-full.txt`、`/tmp/grow-backlog-sampler-threads32.txt`）。[探针源码](tracing_probe.rs)与[结果](tracing_probe_result.txt)为std四线程/recording subscriber与NoSubscriber切换、共享callsite100k事件/span/enter/exit均未丢失；不覆盖fmt与全部test peers，不构成历史flakiness被修复的证据。

历史事实来自[2026-10-29兼容性验证](../2026-10-29-improve-cross-route-reasoning-compatibility/verification.md)和[2026-09-08 R32](../2026-09-08-clean-approved-candidates-r12-r32/verification.md)：分别记录client_post或sampling_request缺失，未保留原始失败capture；当前不能确认原根因。独立review `/root/rules_recovery_review`最终通过：仅5个失败消息、全部wire/log正向/无secret断言及producer不变，无sleep/retry/serial/global cache reset。按当前无reproducer关闭调查候选，保留历史观察与未解决原因的明确限制，不写成已修复产品日志缺陷。

归档前strict18/18；归档后全量strict17/17、archive tasks788/788通过。
