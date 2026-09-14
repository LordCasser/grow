# 验证记录

## 所有权与失败顺序

storage 的 ValidatedTimeline 不再同时持有 events 副本；light load 将已验证 Timeline 移交 persistence/bootstrap。full load 的既有 events 输出保留，子会话失败恢复所需副本也保留。Workflow 只复制待恢复 run 的生命周期；from_restored 的修复副作用没有移动到 ChatState 校验之前。

已逐一核对 blob、sideband、提交输入、System、control/model receipt、hook generation、usage 的校验与 actor 发布顺序；保留 writer lease、pinned entity、replay cutoff、cursor 和冷恢复 usage boundary。

## 测量

合法 fixture 修复完成后取基线，参数与限制见 measurement-notes.md。128 turns 的完整 load 三次中位数：before 503.8 ms，Timeline 复用后 491.3 ms；actor 阶段 115.2 → 110.2 ms。512 turns 单次探索为 1465.7 → 1423.4 ms。这个收益有限，不作为真实终端已流畅的证据。

原始对照输出与后续 Grow-only decode 数据归入相邻 optimize-session-history-replay 的 measurements/ 与 measurements.json。首次失败 fixture 和测试 ACU 体量假设修复均有记录，不混进 before 数据。

## 回归

shell 全量运行 3802 passed、1 failed、3 ignored；唯一失败与用量 fixture 身份重复有关，另在 validate-restored-usage-settlements 记录修复及定向复验。storage light/full、损坏与 symlink/partial tail 拒绝、actor/bootstrap、子会话、Workflow 全部相关现有测试通过。

首次 test 编译发现 repair.rs 的两个 cfg(test) 调用仍使用旧 light 字段，已更新为已验证 Timeline；不是省略校验。Pager、CLI 与 PTY 已通过，见本记录后文。


## 实际 CLI 恢复

`CARGO_BUILD_JOBS=2 cargo build --locked -p cli --bin grow`：通过。debug 链接器报告 __eh_frame 大小警告，与此前 perf 构建相同，退出码为 0。

`PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --test pty_e2e_persistence continue_resumes_session_with_history -- --ignored --nocapture --test-threads=1`：1 passed，8 filtered。首次停在无 LLM 配置的欢迎前检查；原 fixture 直接 spawn，没有调用现有 seed_llm_config。现只在该测试补齐隔离 mock 配置，重跑进入真正交互并通过：同 cwd 退出后 --continue，历史恰好一次，后续 turn 正常完成。没有修改生产配置门槛或扩大到整个 PTY 家族。


## 最终历史与磁盘检查

perf 与 test_session_load_memory 两个 harness 的 --no-run 编译通过。单独设置 GROW_PERF_ASSERT_HISTORY=1，12 turns 默认小样本与 128 turns（8 chunks、4096 bytes）分别 1 passed；全部历史文本在 load response 前与 typed reference 内容、顺序完全一致。大型 RSS/dhat 场景未运行。

本轮新建增量缓存已按创建时间清单清理：244 个新目录回收，36 个原有目录及所有可运行二进制保留，可用磁盘约 43.2 → 61.0 GiB；清单归入 reuse-validated-resume-timeline/disk-cleanup.json。无新 worktree 或真实会话复制。

归档前 `openspec validate --all --strict --no-interactive`：19 passed，0 failed。

归档完成后：`openspec validate --all --strict --no-interactive` 为 17 passed、0 failed；`openspec validate --archived --no-interactive` 为 327 passed、0 failed。`git diff --check` 通过。原有三个进行中的 change 保持不动。
