# Review fix: typed-invalid `allow_non_blocking` policy resolution — 2026-11-06

Fixes `review-admission-policy.md` P2: the live resolver previously turned a
present-but-invalid `allow_non_blocking` value (or a non-table `toolset` /
`ask_user_question` node) into `None` → default `true`, silently reopening the
advisory gate.

## Contract implemented (`shell/src/util/config/resolve/toolset.rs`, only file changed in production)

- `ask_user_question_allow_non_blocking_from_toml` now returns
  `Result<Option<bool>, String>`: `Ok(None)` = genuinely absent node/key (may
  default `true`), `Ok(Some(b))` = legal boolean, `Err` = present-but-invalid
  value type or non-table intermediate node.
- `resolve_ask_user_question_allow_non_blocking_from_disk`:
  1. `load_effective_config()` first — file **read/syntax failure is `Err`
     unconditionally**, even when a valid env override exists (per parent
     steering; not relaxed to env-pass-through on read failure).
  2. Valid env (`GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING` = true/false, via
     existing `config::env_bool`) short-circuits and **overrides field-type /
     structure errors** in an otherwise-legal file.
  3. Typed extraction: absent → `DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING`
     (true); present-invalid → `Err` — the tool fails before publication, the
     error never becomes a recommendation or a user answer.
- No new public error type/framework; timeout/search/login parsers and Factory
  finalized-timeout snapshots untouched.

## Red → Green evidence

**Red (old resolver), `/tmp/grow-ask-policy-type-oldred.log`:**
- Integration `test_advisory_question_admission` (extended with scenario D:
  `allow_non_blocking = "false"` string, real model ask tool call): FAILED at
  `typed-invalid policy must not publish an ask reverse request` — the old
  resolver defaulted the gate open and published the card. Genuine, valid red.
- Unit child test first pass did not compile (fixture typo in the new
  assertions — `.expect_err` chained on `bool`); recorded accurately as a
  fixture compile error, **not** counted as red evidence. No production
  rollback/re-run cycles were spent on it.

**Green (fixed resolver), `/tmp/grow-ask-policy-type-green.log`:**
- `cargo +1.93.1 test --locked --offline -p shell --lib -- --nocapture
  allow_non_blocking` → `3 passed; 0 failed`, exit 0. The isolated child
  (tempdir `GROW_HOME`, no real user state) now discriminates:
  default true; legal false; string/int/array value, scalar `toolset`, scalar
  `ask_user_question` → `Err`; malformed TOML + env `1`/`0` → still `Err`
  (file-level failure not env-overridable); typed-invalid + env `1`/`0` →
  `Ok(true)`/`Ok(false)`; env removed → `Err` restored; missing key (only
  `timeout_secs`) → default true.
- `cargo +1.93.1 test --locked --offline -p shell --test
  test_advisory_question_admission` → `1 passed; 0 failed`, exit 0. Scenario D
  now passes through the real chain (mock provider ask tool call → Factory
  toolset `Tool::run`): no ask reverse request reaches the client within a
  bounded 2 s window (fail happens before enqueue/publication), the turn
  continues only after the model receives the failed tool result, and the
  settled result contains the policy error and does **not** contain the
  advisory `the user has NOT answered` presentation text. In the final provider
  request, all four call ids have one projected tool result each. This is model
  history evidence, not an independent count of persisted Timeline events.

**Regressions, `/tmp/grow-ask-policy-type-regress.log` (all exit 0):**
`question_worker` 3, `question_deadline` 1, `ask_policy_resolver` 1 (Factory
finalized-timeout snapshot preserved), `ask_allow_non_blocking_write` 1,
`ask_user_question_merge_writes` 1 — all `ok`.

## Parent review

父亲读最终load-before-env顺序、typed抽取与隔离unitcases，核有效旧红确为实际ask被错误publication；首轮unit编译错不计行为反例。最终green日志为policy3项与runtime1项四场景全部exit0；回归日志含Factory timeout及worker持久错误语义控制。确认该P2已在新增字段owner消除；原Shell17项worker证据仍来自修复前全filter，当前类型修复没有修改worker。Pager/Settings/真实PTY仍待最终集成，不能从此声称整feature已通过。

## Scope / release

Write scope: `shell/src/util/config/resolve/toolset.rs` (resolver + its
tests) and a negative-control scenario appended to
`shell/tests/test_advisory_question_admission.rs` (owned from the prior round).
No Tools/Pager/other production changes; no fmt/commit; no real user
state/config/session/Goal touched. Command env: `RUST_MIN_STACK=16777216
CARGO_BUILD_JOBS=2 PATH=~/.cargo/bin`, Rust 1.93.1 `--locked --offline`. One
stale pre-steering background run (`01a1b07b`) was killed before the final
run; the final background run (`01a1b07c`) exited normally. Cargo window and
Shell scope are released; the original three-scenario integration remains
accepted and unrolled-back.
