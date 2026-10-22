# P0 release verification and P1 gate

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

## Scope and method

Diagnostics/test tooling only; no storage, protocol, replay order, readiness, input policy or rendering algorithm changed. Normal release/default CLI features, Rust 1.99.1 (Homebrew; actual compiler recorded in build/run context, despite repository toolchain file pinning1.93.1), incremental off, four build jobs. Own target `target/resume-attribution-release`; shared target is untouched. Executable identities were checked during the original comparison. No own compilation or correctness run overlaps formal sampling; external host load is uncontrolled.

Three alternating AB/BA pairs, fresh processes with warm executable/filesystem cache. `smoke/` retains excluded warmups: both newly built CLI executables initially spent about1.2s before their first instrumented frame; this interval is unassigned and must not be attributed to Markdown. Formal samples follow warmup. N=3 is a local comparison, not a latency guarantee.

Paired CLI fixtures use identical modern admitted history, 128/512/2048 turns ×32KiB assistant text; actual typed projector counts and bytes are retained. The extra final PTY visibility sentinel is one extra notification merged into the last assistant block. Counting RPC and no-draw controls use the same canonical body pattern but different startup/transport/finalization boundaries (and no extra PTY sentinel); never subtract them to claim exclusive rendering cost. Real CLI uses direct typed channels, not leader IPC; RPC control uses JSON ACP duplex.

## Measured attribution

All values below are medians in milliseconds, n=3; per-run records have been removed from the archive.

| Turns | Counting RPC | No-draw decode/project | No-draw handler | TUI loading batch | TUI Markdown finish | TUI frame construction | TUI reducer finalize |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
|128|92.108|14.497|58.030|61.883|48.512|7.710|15.418|
|512|158.722|59.192|226.373|237.376|193.801|24.070|56.870|
|2048|407.350|233.669|914.834|952.119|774.469|77.246|223.635|

These intervals overlap or nest; do not add columns. No-draw finish_reload is0.440/0.489/0.814ms, and deliberately excludes the full SessionLoaded dispatcher/descendant restore. Markdown finish accounts for about81% of loading batch service work at512/2048 (the final body also finishes in reducer finalization). It ran exactly once per assistant block, not repeatedly on the same ordinary replay body. All12 detailed P0 observations have matching finish counts and source-byte sums; see `source-count-checks.json`.

Images are absent from these text performance fixtures; reference probes therefore provide no image-heavy latency claim. Image correctness is covered separately. `wrap` includes renderer work and wrapping, not pure line wrapping. Markdown finish, image scan/probes and Mermaid work nest in message.finish; frame.render nests in present; wait/write nest in delivery.

The remaining reducer tail is real work: even the root-only fixtures synchronously call `restore_descendant_state` → `durable_child_spawns` → `load_timeline_by_id_at` (`pager/src/app/subagent.rs:314,338,125`). The second root Timeline decode is visible on the UI thread inside finalize. At2048, reducer finalize is223.6ms, of which about another root decode dominates; this is separate from the774.5ms Markdown cost. Keep any validated child-ownership reuse in a future independent change; do not skip it based on untrusted or incomplete UI rows.

## Diagnostic observer effect and default-mode control

Detailed logs are opt-in. At512, log-mode replay median is254.1→257.0ms; at2048,1007.2→1035.2ms. Small service changes can move the final body across a100ms paint deadline; the n=3 data does not establish how much is diagnostic cost versus scheduling noise. Slow log-mode last visible median436.5→570.9ms, with before2–3 versus after3 observed advances. This is not an optimization result, proof of a uniform diagnostic penalty, or proof of runtime neutrality. Never multiply whole-test26/27frames (includes typing) by40ms.

The slow-writer wait timer now exposes roughly40–45ms per relevant frame, although actual write/flush remains tiny. Writer ACK observation is usually immediate/few milliseconds; concurrent event emission can produce a tiny negative timestamp delta, which is not negative physical latency. Source sequence, writer delivery and simulated terminal visibility remain separate clocks/proofs.

Default diagnostics-off comparison (pre-P0→P0), medians [range] ms:

| Profile | Last history visible before | Last history visible after |
| --- | ---: | ---: |
|128|182.4 [172.2,240.1]|180.2 [166.7,238.6]|
|512|412.5 [410.6,417.5]|447.7 [417.1,497.7]|
|2048|1391.0 [1390.4,1753.0]|1403.4 [1401.1,1410.2]|
|slow512|542.3 [421.0,547.9]|544.2 [543.6,584.4]|

Default slow first visible284.8→283.4ms; maximum observed gap136.2→143.1ms; each run made2–3 visible advances. No uniform P0 speedup or zero-overhead guarantee is claimed. Defaults-off `ready_ms` is explicitly unavailable; input echo is checked after the stable ready prompt. The old detailed baseline uses early end_batch as a labelled proxy and cannot be compared to the new full-finalize ready timestamp.

## Correctness and failures retained

- Scoped Pager release:1733 passed,0 remaining failures,3 ignored; writer+image tests:171 passed. Total1904 passed. Golden32 Grow/7ACP families, per-prefix ordered history/render equality, root/child routing, tools, image-recovery, Markdown/raw/pretty copy, Mermaid, source maps, selection/layout, resize/load/fork, loading input and writer success/failure/drain are covered by the scoped groups. Counts/logs: `checks.json`.
- Existing release-only mismatch retained: the broad scrollback run had1004pass/1fail/1ignore because `insert_block_before_rejects_an_already_committed_anchor` expects a disabled `debug_assert!` panic. Only that case is explicitly excluded from release; no production behavior/test body was changed. All neighboring insertion/selection/commit-frontier checks pass; backlog records it.
- No-draw:9 real ignored-test executions across3 sizes/3 processes, exact full assistant-source and order comparisons; actual handler ACK and complete AgentView reload finish, no frames.
- Counting RPC:9 formal runs; full-history correctness oracle at128/2048 passes. Actual follow-up input after quit/resume passes for pre-P0 and P0 CLI.
- Detailed PTY:3 pairs ×normal128/512/2048,3 pairs ×slow512, loading Ctrl+D quit/re-resume pair all pass. Defaults-off:3 pairs ×normal128/512/2048 andslow512 pass. Normal/slow captures also assert zero inference from restore/ignored loading input; ready key echo and history progress gates pass.
- Initial test compilation failed on private Shell file constants; fixture-local literals fixed it. First RPC build omitted required test-support; runner corrected it. Both failed attempts were excluded from executed test counts. Scoped rustfmt and git diff checks pass.

## P1 decision

Proceed to an independent P1 change: historical assistant/thinking source completion remains eager, while pure final Markdown presentation caches materialize only when needed. The measured removable work is material (about194/774ms at512/2048), substantially larger than current frame construction. This is an upper bound on work, not a promised end-to-end reduction: visible blocks still render, source scans stay, producer/consumer work overlaps,100ms cadence and40ms writer delay create floors/cliffs, and root/descendant validation remains.

Keep images probed at the existing completion boundary, keep Mermaid source/affordance geometry correct, preserve final normalizer flush before raw copy, full-render before pretty copy/source-map/link access, deterministic source-based offscreen estimates and exact visible layout. No background history, early ready, pagination, new persistent cache, new actor, SQLite or storage changes. P1 must demonstrate benefit in diagnostics-off release pairs and independently rerun rich-message/media/copy/anchor gates. A poor end-to-end result is grounds to narrow or reject it.

OpenSpec strict validation/archive results follow in completion.json. The dedicated target is still a necessary P1 baseline input at P0 archive; it will be removed after dependent comparisons, not confused with shared build residue.
