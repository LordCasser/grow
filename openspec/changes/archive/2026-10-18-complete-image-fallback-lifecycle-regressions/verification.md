# Verification

## Source-only scope

没有安装、替换本机 Grow、重启或修改用户 live sessions，也没有调用 live provider。HTTP 使用本机 MockInferenceServer；OCR 使用 test-only task-local 程序，保留原有生产进程运行器、输入解码、超时、输出校验与 kill-on-drop；不修改全局 PATH。取消后 OCR 是否被调用通过专用测试线程的 thread-local 计数观测，避免并行用例共享全局变量。

## Isolated build

本轮先尝试主工作树 `cargo test --locked --offline -p shell --lib image --no-run`，因与本 change 无关的并行 planning 字段/导出缺失失败（例如 tool_types::planning、SubagentCompletedOutput.planning_summary）。未修补那些并发改动。

隔离快照由 `git archive 8cc20454da9e62874170fb3ed1cc532591954947` 构建，叠加同一任务此前的 typed transport identity、中文 text-only classifier、read_file/current/historical recovery 回归，以及本 change 的 OCR 测试 hook、storage 查询/provenance 差异、相关测试与持久化 harness。support 只叠加图片 persistence harness 与已有真实 input payload admission helper；不把 suspension/planning/skill loading 或临时 tracing 差异混入快照。

测试使用 Rust 1.93.1，`CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`，`--locked --offline`，共享 Cargo target 仅复用依赖。以下结果不等同于全工作树含所有并行改动的构建通过。

## Reproduction

有效修复前运行：`cargo test -p shell --lib image_fallback -- --test-threads=4 --nocapture`：**5 passed, 1 failed**。

- 失败为 `image_fallback_cold_restore_reuses_completed_description_before_projection`：恢复后总请求变为 4 而应为 3，说明已完成描述没有复用、额外发起辅助请求。
- 同轮 OCR 三分支、辅助前/运行中取消、已确认视觉投影冷恢复和已确认 OCR 投影冷恢复通过。
- 更早 fixture 编译/初始化失败已修正，不作为生产缺陷证据。

修复前查询与 projection ledger validator 都要求描述 manifest 的全局 Surface revision 等于当前 revision。修复只允许同一当前 branch 的 immutable image source 复用不晚于当前 revision 的已完成中性描述；保持 prompt、source/evidence ranges、完成记录和精确新投影的 identity/fingerprint 校验，并拒绝未来 revision。

第二条有效失败：修复 revision 条件后，新增无辅助配置冷恢复回归，运行 `cargo test -p shell --lib image_fallback -- --test-threads=4`：**7 passed, 1 failed**。失败为 `image_fallback_cold_restore_reuses_description_without_auxiliary_route`，恢复后的主请求没有已持久化的 E91 描述。原因是历史查询位于可用辅助路由分支内；已把 cache/durable reuse 移到该准入之外，未命中组才解析辅助模型。

存储反例扩展期间曾遇到 fixture 的 DirectUser 权限形状与继承 seed 不携带本地 rewind 坐标问题；按实际 Timeline 契约修正为本地 User/Turn 坐标，不删失败场景或放松 Timeline 验证。这些 fixture 错误不计为生产缺陷。

## Final regression results

| Command (with locked/offline and the environment above) | Result |
| --- | --- |
| `cargo test -p shell --lib image -- --test-threads=4` | **142 passed, 0 failed, 1 ignored**, 46.52 s |
| `cargo test -p shell --lib sideband -- --test-threads=4` | **35 passed, 0 failed**, 1.02 s |

这些过滤存在重叠，不合计为独立测试总数。Ignored 为既有 48 MP 手动内存测量，不将其计为通过。最终新增用例包含四个冷恢复分支、OCR 三种结果、辅助前/准备 ACK/运行中取消、OCR 运行中取消；扩展后的存储 source/prompt/revision/completion/rewind 反例也已通过。

`git diff --check` 通过。归档前 `openspec validate --all --strict --no-interactive`：**21 passed, 0 failed**。

## Adversarial boundaries

- OCR：成功非空、成功空结果、进程失败；使用原 PNG 输入；projection ACK 前没有重提交；成功保留图和描述、失败移除活动图但保留 Timeline 证据。
- 取消：primary 拒绝返回前、auxiliary Attempt ACK 挂起、首组 auxiliary HTTP 活跃；Stop 结束真实 producer，释放旧 response/ACK 也不再派发第二组或主请求；OCR 调用数为 0。
- OCR 中取消：实际 PID 的 OCR 进程终止并回收，无新投影或重请求。
- 冷恢复：原 actor/cache 销毁，资源状态从文件恢复；完成描述但未投影，以及已投影视觉/OCR 三种窗口及辅助配置被关闭的恢复；新输入推进全局 revision 后仍只新增一条文本主请求，原图证据保留。
- 存储反例：不同 prompt/source、不存在于父 ledger 的查询 revision、未来 manifest revision、未 Completed 的结果、rewind 丢弃的来源、其他损坏 Sideband；冷恢复完成后篡改成未来 manifest revision，必须拒绝再次加载已持久化投影。

覆盖范围与明确未覆盖组合见 [audit.md](audit.md)。OCR 识别准确率、非 Unix 进程行为、真实 provider、完整 CLI 子进程启动不在本轮测试保证内。

## Archive and cleanup

`openspec archive complete-image-fallback-lifecycle-regressions --yes` 已完成；新增场景与上表测试/反例逐项核对。归档后 `openspec validate --all --strict --no-interactive`：**20 passed, 0 failed**；`openspec validate --archived --no-interactive`：**581 passed, 0 failed**。

删除本轮临时源码快照、修改前源码备份、测试可执行文件 `shell-e13c2ed4401144f5` 及 `.d` 文件（约 519 MiB allocated）；保留共享 Cargo 依赖与其他并行任务的 target，不清空整个 target。最终 `git diff --check` 通过。
