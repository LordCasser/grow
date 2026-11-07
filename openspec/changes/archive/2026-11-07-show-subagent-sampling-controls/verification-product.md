# Product verification — actual CLI PTY

状态：v10 真实 CLI 产品链已通过并经父审查。父从真实屏幕核对唯一 parent Queued 行、child 旧选型下的 Pending、closed-Step 后唯一 Applied notice，并核对正式 replay receipt、下一目标 provider 请求和 process exit 0。v9 的生产故障及旧红保留如下；SessionActor publication receipt 和 child bootstrap 顺序已分别修复验收，freshbuild 后同一原 exact PTY compile/enumerate/execute/runner 均 exit 0。

## 用例、产物与隔离

- 唯一用例：`subagent_model_control::parent_set_subagent_model_queues_then_child_applies_at_its_closed_step`，位于 `crates/codegen/pager/tests/pty_e2e/subagent_model_control.rs`，注册仍为既有 `pty_e2e_shell_tools.rs` 条目，本轮无须修改注册。
- 使用真实 `/Users/lordcasser/workspace/projects/grow/target/debug/grow`，bytes `424578920`、mtime_ns `1793994920533794682`、SHA256 `52dce97918b50136391395c61372a309ca6427c46298b1e4e1f5efc784f89463`、BLAKE3 `614ab7a231adfd9547330adbf7b43c9da9caf9c23f0a50dc5c8976b53aed1b28`。父构建日志 `/tmp/grow-subagent-control-cli-final-build.log`（actualexit 0）及 identity `/tmp/grow-subagent-control-cli-final-identity.json` 保留。本轮只编译 test，未重建 CLI。
- `ContentController` / `PtyHarness::spawn_with_content_in_dir` 提供临时 HOME、GROW_HOME、loopback mock provider 和临时 git project，不写真实用户配置。
- 只修改此测试及本记录；未改生产、公共 harness、manifest、其他测试或注册，未 commit/push/archive。共享工作区已有改动保留。

## 实际发现与夹具修正

first–v7 的夹具/验收入口失败保留原日志，不充当 production old-red；v8 是尚未闭合的实际 child pending 显示链，须据运行时证据定位：

| 轮次 | 实际结果 | 定位与修正 |
| --- | --- | --- |
| first | compile 0 / 唯一枚举 0 / execute 101 / runner actualexit 1 | root sessions 含合法非目录索引，旧 helper 对索引 `read_dir` 报 ENOTDIR；两个目录层都先 `file_type().expect(...).is_dir()`，仅跳过非目录，真实 IO 错误保留。日志 `/tmp/grow-subagent-control-product-first.log`。 |
| v2 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | `actual child journal` 失败。此前静态结论“生产 child 一定 nested parent/subagents/task-id”错误：`handle_request` 实际以 spawned child 的 SessionInfo 调用标准 `session_dir`；Explicit adapter 仅是通用能力。修正唯一标准 cwd/session-id 路径，并通过正式 typed summary 校验 child session id、spawn.child_cwd 和隔离 project 的 canonical 路径。日志 `/tmp/grow-subagent-control-product-v2.log`。 |
| v3 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | 新 project 校验发现 macOS `/var` 与 `/private/var` 同目录别名；改比较 canonical 路径，保留 typed summary 的实际 id/cwd 校验。日志 `/tmp/grow-subagent-control-product-v3.log`。 |
| v4 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | parent 屏幕 `Ran 1 tool · 1 failed`，无目标行；未用 mock 正文 `PARENT_CONTROL_QUEUED` 充当准入成功。保存 `/tmp/grow-subagent-control-product-v4.log`、`/tmp/grow-subagent-control-product-parent-v4.txt` 后报父。 |
| v5 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | canonical exact SET_CALL 为 `Model control requires an active directly owned non-Workflow subagent with open execution`。root/child journal 证明控制前 child 已完成：既有 ScriptedResponse 仅挡最后 SSE event，旧 script 的 finish_reason 已先于尾 `[DONE]` 关闭 Step。仅本 test 显式移除并校验尾 `[DONE]`，使屏障挡住真正 finish_reason/usage 终态。日志 `/tmp/grow-subagent-control-product-v5.log`；原始屏幕、typed result/events、严格解码 journal 保存在 `/tmp/grow-subagent-control-product-evidence-v5/`。 |
| v6 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | child 真正保持 running 后，公共全屏 idle helper 被 child `Waiting for response` 行阻塞 60s；parent 实际已 Worked 且输入区可用。仅 test 改按各自精确 parent prompt 关联 typed Started.id → Ended(completed)，并核对输入区状态，不改公共 helper。日志 `/tmp/grow-subagent-control-product-v6.log`。 |
| v7 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | 两次 parent 精确 TurnEnded 及 typed `status: queued` 通过；默认组显示 `Ran 1 tool`。父核对已归档 activity-group contract 后确认应通过正常用户展开成员，不能改 group 配置或生产造例外。原始证据 `/tmp/grow-subagent-control-product-evidence-v7/`，日志 `/tmp/grow-subagent-control-product-v7.log`。 |
| v8 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | 正常组展开后的单一 Queued 行及所有 parent 显示断言通过；Tasks 唯一 child 的查看按钮打开了正确 fullscreen，保持 mock/child、first Step open。但等待 60s 未出现 `model child→target (high)`，尚未释放 Step、未验证 applied/next request/clean exit。日志 `/tmp/grow-subagent-control-product-v8.log`，独立证据 `/tmp/grow-subagent-control-product-evidence-v8/` 含 `parent-group-collapsed.txt`、`parent-queued.txt`、canonical exact Queued result/events、root/child journal、从 timeout 原日志提取的 `child-pending-failed.txt`。 |
| v9 | compile 0 / 唯一枚举 0 / execute 101 / actualexit 1 | 仅新增证据模式的既有 per-process 日志，所有产品断言未变；再次在 child pending 等待 60s 失败，实际执行 67.74s。日志 `/tmp/grow-subagent-control-product-v9.log`；runtime 日志证明 view 注册前通知丢失及后续 unknown-epoch 拒绝，见下节。 |

按父确认的既有分组契约，测试先保默认 collapsed 帧，再聚焦 scrollback、等待其 focus hint，双击精确 control 活动组，检查真实 Queued 成员行。child 从顶部 Tasks 唯一真实 child 的 `[↗]` 查看按钮打开，并核对按钮与该 child 描述同一行；无需回到上一 Turn 的折叠 spawn。未关闭分组，未加盲导航、任意 sleep 或公共 API fallback。

## 断言与证据保存

1. 真实 `todo_write` 后后台 `spawn_subagent`；root 由实际 ToolEvent::Started.call_id 定位，child/task id 只取正式 SubagentSpawned；校验 parent step、初始 model 和 typed child summary。
2. child 首 Step 在真实 finish_reason 之前保持 open；parent 两个 Turn 按自身 canonical identity 收尾。exact SET_CALL 的正式 `SetSubagentModelOutput` 必须是同 child 的 Queued，先验证准入再判断展示，mock sentinel 仅是脚本文本。
3. 正常展开的 parent 行只有一个 `set_subagent_model`，保持请求目标及 `Queued · model mock/target · effort high`；无 Applied 或 Sampling switched。canonical parent 仅一个 ToolEvent::Completed 和一个匹配 ToolResult，root selection 不变，所有 parent wire requests 保持 parent-wire-model。
4. child pending 帧同时保原 `mock/child` 与 `model child→target (high)`；canonical model 仍是 child、effort 为空、target transition 为 0。
5. 释放首 Step 后 child 请求序列严格为 child-wire-model、target-wire-model；下一目标请求的正式 Chat Completions `reasoning_effort` 为 high。child 普通 Sampling switched notice 精确一次。
6. journal 通过 `decode_timeline_fixture` 的 checksum/typed Timeline 解码。目标 model.changed 唯一，host ControlIntent generation=0、sequence=0、namespace 为 native-sampling；正式 `stream_replay_grow_notifications_at` 投影唯一 Applied Sampling receipt，其 current/desired/message/intent 与 model.changed 一致、非 hydration snapshot。无退役 updates.jsonl、JSON 字段猜测或默认空结果。
7. child 视图通过既有全局 quit 两次退出，必须真实 process exit 0，无 kill fallback 成功。

可选环境变量 `GROW_SUBAGENT_CONTROL_EVIDENCE_DIR` 只接受显式绝对 `/tmp` 目录。保存失败前屏幕、严格解码的 root/child journal、exact typed control events/result；成功另保存 parent Queued、child pending/applied 三帧、canonical 前后 journal、正式 replay receipts、仅 path/model/reasoning_effort 的 wire 摘要、CLI identity 和 process exit。写出失败会直接失败；不保存配置或凭据。当前目标目录：`/tmp/grow-subagent-control-product-evidence/`。

## v9 实际 authority 丢失边界

v9 仅在本 test 的证据模式通过现有 `spawn_with_content_env_ops_in_dir` + `EnvOp` 启用 `GROW_LOG_FILE` 与目标模块 `RUST_LOG`，复用已有 advisory fixture 惯例，不改生产或公共 harness，也不修改未知 epoch 校验。日志直接写 `/tmp/grow-subagent-control-product-evidence/cli-diagnostics.log`，没有复制配置或凭据。

该日志的实际次序为：

- 第 25–27 行，`20:26:39.025173/223/248Z`，child session `01a1b28a-bbbd-7712-88e7-09e10fa5b940` 的三条 Grow 通知均被 `load-race: grow/session_notification DROPPED — no agent matches session_id` 丢弃。
- 第 29 行，`20:26:39.035982Z`，相同 child 的 `Subagent spawned` 才被 Pager 处理，比第一条被丢弃的通知晚约 10.8ms。
- 第 42 行，`20:26:39.988448Z`，`ignoring live control update from unknown epoch update_epoch=01a1b28a-bc18-7720-aa14-4c64a3b70ed3 active_epoch=None`，证实本轮 live control 在 authority 尚未建立时被拒。

第一个实际丢失边界是 child session 尚未注册时的 Grow 路由丢弃，随后新 epoch 的 live control 被现有安全规则拒绝。被丢弃包的日志没有 payload，不能逐包断言其 domain；结合生产创建路径，`handle_request.rs:1927` 发 parent `SubagentSpawned`，`subagent/mod.rs:2537` 仅入 parent actor mailbox 即返回，`handle_request.rs:1968` 随后向 child actor 要求三域控制 snapshot。parent 真正向 gateway 转发与 child 直接发布 snapshot 没有跨 actor 的送达屏障；当前实际到达次序已经证明该窗口发生。不能把 `PublishControlState` 的本地 ACK 当成 Pager 已注册 child 的确认。

已排除“打开 child 后换了新 Box 清掉 Pending”这一具体解释：`open_subagent_fullscreen` 复用既有 child view；`replay_inherited_updates` 不清 shell_controls。Pager child ControlState 路由会交给该 view 的 `apply_control_state_update`，但 `session/mod.rs:1956` 明确仅 snapshot/load window 能建立新 epoch。保留该过滤；把任意首 live 包视为合法会削弱旧 actor 迟到包隔离。

父另行决定并实现生产修复边界。现有模块证据验证 actor 发出 Pending 和消费者已知 authority 后展示；本实际链暴露跨 actor 首次 child authority publication 的整合缺口。当前测试继续保留 pending 显示、旧 selection、next request、唯一 terminal receipt/notice、root route 和 clean exit 的原验收断言。

## v10 freshbuild 后的完整通过

SessionActor publication receipt 和 bootstrap gate 的模块证据分别见 [回执验证](verification-publication-receipt.md) 与 [启动顺序验证](verification-startup-order.md)。父完成源码审查后 freshbuild CLI：`cargo +1.93.1 build -p cli --bin grow --locked --offline`（固定环境前缀同上），task `01a1b2b0-c1ca-7f33-baa0-c7d3b6ff8b35`，actual exit 0，日志 `/tmp/grow-subagent-control-startup-cli-build.log`。

新 CLI 为 424843624 bytes，mtime_ns `1793999428699276719`，SHA256 `c5d28154e428a411a8bdd2bddee85cc871a8e3daf9e129b22ac2a601b8f49e7c`，BLAKE3 `7462fc4951fdd7605e3a9cac20d0b1afa6e7370439a3c77a35600627cf91f552`；完整身份 `/tmp/grow-subagent-control-startup-cli-identity.json`。v9 故障基线已保存在 `/tmp/grow-subagent-control-pre-startup-fix`，其原SHA256保持，不与新产物混用。

父使用原唯一 exact 用例，仅将证据目录设为 `/tmp/grow-subagent-control-product-evidence-v10`。task `01a1b2b3-950f-7b33-b4ff-d9718763a7fe` 实际完成 exit 0；compile 0、唯一枚举 0、execute 0，1 passed，实际 PTY 11.32秒；日志 `/tmp/grow-subagent-control-product-v10.log`。没有修改测试或产品断言。

父亲读了下列实际产物，接受本里程碑：

- `parent-queued.txt`：正常展开既有活动组后，仅一条 `set_subagent_model → subagent「01a1b2b5-c160-7b33-a0bb-58313b76cac9」`，状态为 `Queued · model mock/target · effort high`，无 Applied/switch 宣称；同一 call 的正式 completion 仅一个，parent model.changed 前后均为1。
- `child-pending.txt`：实际 child fullscreen 保留 `mock/child`，running footer 同时显示 `model child→target (high)`；释放首 Step 前 canonical 无目标选型变更。
- `child-applied.txt`：实际 child 更新为 `mock/target (high)`，一次 `CONTROL Sampling switched to mock/target (high)`，正文继续为 `CHILD_AFTER_SWITCH`。
- `verified-summary.json` 与严格 typed journal/replay：child wire 请求依次 `child-wire-model`、`target-wire-model`，后者 `reasoning_effort=high`；所有 parent 请求仍为 `parent-wire-model`。目标 model.changed=1、Applied receipt=1、host namespace=`native-sampling:01a1b2b5-c1c4-78b3-8fec-5edf223d5673`、generation=0、sequence=0；`control-replay.json` 的 current/desired/message/intent 与 canonical 一致。
- `process-exit.txt` 为0，原测试经正常全局 quit 获得实际 CLI exit 0，没有 kill fallback 通过。

`cli-diagnostics.log` 中本 child 的 Spawned 已建立路由，随后没有未知 child 路由丢弃或 unknown-epoch 拒绝。日志另有 parent session 的一条 dedup highwater 丢弃记录，不能把它称为本 child 启动故障，也不声称日志中完全没有 DROPPED。

v10 前后2402项仓库 Rust/manifest/toolchain/Python/shell文件的内容、大小和mtime均相同；CLI bytes/mtime/SHA256也未变。清单 `/tmp/grow-subagent-control-product-v10-source-{before,after}.json`、核对 `/tmp/grow-subagent-control-product-v10-verification-state.json`；独立证据目录含三帧、parent折叠帧、canonical journals、正式 replay、wire摘要和 `artifact-sha256.json`。与v9的2172项旧清单选择范围不同，两个轮次各自前后比较，不冒充同一清单。

本链使用真实 CLI、实际 TODO/spawn/tool/coordinator/actor/view 与隔离 loopback provider，不代表真实远端供应商部署或 Windows 现场验证。模块中的瞬时 Dispatching、effort-only/no-op/supersede/failure/hydration 矩阵不在 PTY 重复；它们的已接受证据继续有效。

## 指定命令与验证边界

```sh
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC PATH="$HOME/.cargo/bin:$PATH" RUSTUP_TOOLCHAIN=1.93.1 RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 CARGO_NET_OFFLINE=true PAGER_BINARY="$PWD/target/debug/grow" GROW_SUBAGENT_CONTROL_EVIDENCE_DIR=/tmp/grow-subagent-control-product-evidence python3 scripts/run_exact_cargo_test.py --package pager --test pty_e2e_shell_tools --ignored subagent_model_control::parent_set_subagent_model_queues_then_child_applies_at_its_closed_step
```

最终验证：

- `rustup run 1.93.1 rustfmt --edition 2024 --check crates/codegen/pager/tests/pty_e2e/subagent_model_control.rs`：exit 0。
- 两个本轮 untracked 文件使用 `git diff --no-index --check /dev/null <file>` 核对：无 whitespace 诊断；该模式对有内容的新文件返回 1 仅表示有 diff，已明确检查输出为空。
- `openspec validate --all --strict --no-interactive`：21 passed / 0 failed，actualexit 0；日志 `/tmp/grow-subagent-control-product-openspec.log`。
- v9 前后 2172 个 Rust/manifest/script 文件哈希全同。快照 `/tmp/grow-subagent-control-product-v9-source-before.json`、`/tmp/grow-subagent-control-product-v9-source-after.json`；清单 SHA256 为 `850f9345250bba1589b6265f2c5cbc6bc16d1a169d45c98a2befe6f98b107ca4`。
- CLI bytes/mtime_ns/SHA256 与父指定 identity 完全相同，测试内 BLAKE3 亦为同一值；完整核对状态 `/tmp/grow-subagent-control-product-v9-verification-state.json`。
- v9 独立 artifact `/tmp/grow-subagent-control-product-evidence-v9/` 保存 collapsed/expanded parent 帧、child pending timeout 帧、精确 canonical tool result/events、root/child journal、原 debug 日志、binary identity 与 artifact SHA256 清单。`child-pending.txt`、`child-applied.txt`、正式 terminal replay 和 switched request 摘要尚未产生，不能称三帧成功或 applied 已验证。
- v9 libtest execute exit 101、runner actualexit 1。测试失败时 harness 退役进程；这不同于用例最终 clean exit 0 断言通过。运行后 `ps` 确认本委派的 CLI/test/Cargo/rustc/sample 无在途进程，所有自建 monitor 已结束。
- 未运行 broad `--tests`、full workspace Cargo 回归或其他权限矩阵。没有 production/manifest/shared-harness 修改，未重建 CLI，未 commit/push/archive。

v9 委派当时明确释放测试源码、注册检查、验证文档和 Cargo 窗口，父另建生产发布回执和 bootstrap 顺序两个增量。上述历史记录不声称 v9 feature 或产品验收通过；后续父已完成两项修复审查、freshbuild 和 v10 原断言完整通过，证据见 v10 节。

瞬时 Dispatching、失败矩阵、effort-only/no-op/supersede/durable-failure 和 cold/manual 历史行为沿父已接受的模块证据；本 PTY 只验首条实际显示链，不声称覆盖所有交互/平台或真实远端 provider。独立 natural fixture maintenance 已另行归档。父在v10通过后亲读屏幕与canonical/replay，完成本change验收与归档；最终全量和archived严格校验见[验收索引](verification.md)。
