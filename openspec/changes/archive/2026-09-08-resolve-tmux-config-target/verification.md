## Stage 1: Explicit target (2026-09-08)
CLI FixArgs新增--config，要求fix ID；TUI用既有shlex处理引号并将Option<PathBuf>传入DoctorRequest、后台effect和FixRequest。FixRequest验证tmux类型及安全绝对文件路径；显式目标优先于普通默认和Byobu目录，进入同一预览/apply/verify。SSH不得忽略该参数。

`cargo test --locked --offline -p pager --lib doctor --quiet`：56 passed，0.07s；`cargo test --locked --offline -p pager --lib diagnostics::fix --quiet`：34 passed，0.32s。包含带空格路径、缺参数/重复参数/未闭合引号、SSH拒绝、Byobu无目录但显式目标可用、实际写入与后置验证、默认路径未创建。全量规范16项通过。现有compact-unwind warning，不影响exit0。

## Remaining
自动候选探测、歧义时拒绝默认猜测及诊断路径统一尚未实现。当前无--config仍保留原选择逻辑，不能宣称整个来源修复完成；本change保持active。

## Stage 2: Candidate facts (2026-09-08, partial)
Bounded tmux config_files query preserves raw startup candidate text without splitting unescaped commas. Query protocol tests: 8 passed. Diagnostics collection forwards candidate facts into reports and skips the query for SSH-only fixes. Re-observed `cargo test --locked --offline -p pager --lib diagnostics:: --quiet`: 176 passed, 0 failed (0.30s); existing compact-unwind linker warning. Automatic target selection and diagnostic path unification remain unfinished; change stays active.

## Stage 3: Automatic selection (2026-09-08)
Automatic tmux planning uses explicit config first, then the effective Byobu directory for Byobu, otherwise one nonempty comma-free safe absolute startup candidate. Missing, ambiguous and unsafe candidate evidence now returns actionable --config guidance; no HOME fallback. Dedicated regression cases cover missing/empty, comma list or filename, relative/control/traversal/root paths, preserved trailing space, actual custom-target apply, and explicit comma target precedence. Transaction/scanner fixtures now select their isolated path explicitly rather than depending on the removed default assumption.

`cargo test --locked --offline -p pager --lib diagnostics::fix --quiet`: 35 passed, 0 failed, 0.31s; existing compact-unwind warning. Subsequent fix-list NeedsConfig presentation change is under validation and is not included in that count. Diagnostic manual paths remain unfinished.

Fix-list follow-through: target-source errors remain listed as NeedsConfig, with CLI and TUI --config commands. Fix tests: 36 passed (0.31s); complete diagnostics module: 178 passed (0.31s). Strict validation: 16 passed. No real tmux server test on this host. Remaining work: unify diagnostic manual paths and reload notes, update user guide, final scenario audit and archive. Disk after validation: target 3.6 GiB, free 70 GiB; retain this cache for the immediately following same-module implementation, then clean at batch completion.

## Stage 4: Registered Doctor fix guidance (2026-09-08, partial)
Both CLI configured reports (including SSH sessions) and TUI reports after runtime merge now resolve registered tmux fix guidance through the same selector as planning. Tests compare selected report path against plan for all three fix IDs and ordinary/Byobu targets, preserve notification suffixes, verify shell quoting, and reject guessed-file/one-off rendering when target evidence is absent.

`cargo test --locked --offline -p pager --lib doctor --quiet`: 58 passed, 0.10s. `cargo test --locked --offline -p pager --lib diagnostics:: --quiet`: 180 passed, 0.27s. Existing compact-unwind warning only. Startup banners and truecolor/manual-only default candidate guidance remain to be completed; task 3 is intentionally still unchecked. Cache 3.6 GiB, free 70 GiB; no compiler remains live after these runs.

## Final audit (2026-09-08)
This section supersedes the incomplete-stage notes above.

- Explicit custom target: CLI clap and TUI shlex tests cover path transport and invalid syntax; FixRequest validates it; explicit_tmux_target_drives_preview_apply_and_verification covers preview, real isolated apply and verification (plain and Byobu). Managed transaction and conflict tests remain green.
- Ambiguous evidence: bounded runner tests preserve unsplit raw text, reject invalid encoding, failed exit and timeout; collector tests preserve facts. tmux_automatic_target_requires_unique_safe_server_evidence covers missing/empty/comma/unsafe inputs, actual custom target apply, and explicit comma precedence. NeedsConfig listings preserve the CLI/TUI selection entry.
- Consistent Byobu target: tmux_doctor_guidance_and_plan_share_the_selected_target compares all three registered fix report paths against the plan, including custom directories with quotes and spaces, preserving notification notes. Both real report entry points call configure_tmux_report after collection/merging.
- Manual/startup guidance: tmux_reload_note and truecolor warnings label legacy defaults as candidates, requiring confirmation. Doctor manual-only truecolor guidance adopts a known selected path and quoted source-file command; unknown targets keep the explicit candidate label. manual_tmux_guidance_uses_known_target_or_labels_default_candidate verifies both paths and keeps shell/reattachment advice.
- User guide no longer claims plain tmux always edits HOME. It explains explicit precedence, config_files ambiguity and limitations, Byobu policy and reload behavior.

Final runs: diagnostics module 181 passed (0.35s); doctor filter 58 passed (0.07s); pager-render tmux_probe 8 passed (1.34s). These filters overlap and are not a combined distinct-test count. git diff --check clean; strict all 16 passed. No tmux executable exists on this host, so no real tmux server integration was run. Existing macOS compact-unwind linker warning remains, with test exit 0. No actual user config or live server was changed.
