# pager-pty-harness 文件与契约覆盖

当前40个文件均完成源码阅读并登记哈希。下表区分直接需求来源与注册/测试/说明文件；文件有映射不等同于其所有分支已通过运行验证。

| 文件 | 契约或证据角色 |
| --- | --- |
| `Cargo.toml` | 包边界、依赖、bin/test/bench注册；行为映射见相应实现。 |
| `benches/paste_latency.rs` | Paste latency benchmark selection and global clipboard scope；Paste latency measurement start and screen observation；Paste latency surface readiness and clear fallback |
| `benches/pty_baselines/README.md` | 开发者基准使用说明；已纠正命令和未经CI证明的描述。 |
| `benches/pty_bench.rs` | PTY benchmark CLI orchestration and failure output |
| `src/bin/pty_scenario.rs` | Scripted PTY command report exit mapping |
| `src/bin/scroll_matrix.rs` | Scroll matrix command selection and report lifecycle；Scroll matrix command concurrency and ordering |
| `src/content.rs` | PTY content controller defaults and explicit config seed；PTY logical turn expectation endpoint alternatives；PTY foreground turn registration matching scope |
| `src/env.rs` | PTY harness binary resolution and implicit build |
| `src/flows.rs` | PTY flow label waiting and submit retries；PTY inference count path matching；PTY model waiting creates new sessions |
| `src/host_clipboard.rs` | Host clipboard text commands and restoration scope；Host clipboard roundtrip mutates global state；Host clipboard PNG fixtures and platform encoding |
| `src/leader.rs` | PTY leader cluster shared socket launch；PTY leader persisted update scan tolerance；PTY leader completion wait identity scope |
| `src/lib.rs` | PTY harness output fanout and reply forwarding；PTY condition stability and idle observation；PTY cast capture geometry and byte decoding；PTY exit drain preserves known code with separate budgets |
| `src/pty.rs` | PTY child environment layering；PTY reader channel and drain boundary；PTY child exit observation and cached status；PTY quit and drop bounded cleanup |
| `src/results.rs` | PTY benchmark aggregation and percentile semantics；PTY baseline persistence and regression scope |
| `src/scenarios/empty_enter_send_now.rs` | PTY queued send now regression assertions |
| `src/scenarios/idle_cost.rs` | PTY idle cost workload measures welcome frames |
| `src/scenarios/large_codeblock.rs` | PTY large codeblock workload content and timing |
| `src/scenarios/mixed_interaction.rs` | PTY mixed interaction workload overlap scope |
| `src/scenarios/mod.rs` | PTY benchmark scenario registry |
| `src/scenarios/plan_approval_resume.rs` | PTY plan approval resume fixture lifecycle；PTY plan control fixture ledger validation |
| `src/scenarios/resize_storm.rs` | PTY resize storm workload and crash checks |
| `src/scenarios/scroll_stress.rs` | PTY scroll stress workload and readiness proxy |
| `src/scenarios/streaming_render.rs` | PTY streaming render workload window |
| `src/screen.rs` | PTY screen visible and historical query separation；PTY emulator reply draining |
| `src/scripted.rs` | Scripted scenario decoding and validation boundary；Scripted run preparation and skip ordering；Scripted run failure capture and completion scope；Scripted ephemeral workspace materialization；Scripted input injection and observation windows；Scripted image clipboard simulation and drop payloads；Scripted locator character indexing and selection range；Scripted highlight assertions use dominant background；Scripted screenshot serialization boundary；Scripted image fixture bytes and name handling；Scripted request image discovery and dimension assertion scope；Scripted request text and temporary file assertion scope；Scripted OSC52 raw history decoding；Scripted Kitty graphics presence counting boundary；Scripted pointer protocol encoding；Scripted artifact naming and assertion defaults |
| `src/scroll_matrix/cells.rs` | Scroll matrix representative cell profiles and tiers；Scroll matrix declared invariant and fixture constraints |
| `src/scroll_matrix/gestures.rs` | Scroll matrix gesture dispatch and declared stream counts |
| `src/scroll_matrix/invariants.rs` | Scroll log invariant routing and absent finalize scope；Scroll log cadence and carry tolerances；Scroll log pricing bounds and coast measurement；Scroll log configuration echo validation |
| `src/scroll_matrix/log.rs` | Scroll matrix JSONL schema and stream grouping；Scroll matrix finalize wait raw marker scope |
| `src/scroll_matrix/mod.rs` | 公开模块导出；具体契约分属cells、gestures、log、runner、report、session、invariants。 |
| `src/scroll_matrix/report.rs` | Scroll matrix report encoding and exit policy |
| `src/scroll_matrix/runner.rs` | Scroll matrix blocking cell timeout and failure reports；Scroll matrix quiet window and teardown before verdict；Scroll matrix screen travel simulation boundary；Scroll matrix expected failure classification precedence |
| `src/scroll_matrix/session.rs` | Scroll matrix marker parsing boundary；Scroll matrix settled session preparation；Scroll matrix streaming session preparation boundary |
| `src/timing.rs` | PTY frame timing parser measurement boundary |
| `tests/empty_enter_send_now.rs` | ignored集成入口；对应 PTY queued send now regression assertions。 |
| `tests/env_op_compile.rs` | EnvOp四种构造形式的编译证据，不执行子进程环境修改。 |
| `tests/plan_approval_resume.rs` | 集成入口；对应 PTY plan approval resume fixture lifecycle。 |
| `tests/prompt_history_durable_quit.rs` | PTY prompt durability quit regression scope |
| `tests/scroll_correctness_ptyctl.rs` | PTY scroll correctness mixed input regression scope |
| `tests/scroll_matrix_curated.rs` | PTY curated matrix test selection and serialization |
