# 验证与性能结果

## 保留的实现

Grow-only 恢复通过 RawLinePeek 识别方法，只反序列化 Grow payload；两个读取入口共用同一个恢复扫描。保留 pinned 文件、committed line、rewind、通知 sessionId/eventId/meta、损坏缓存行跳过规则。初始通知队列、tool-call 合并和最终 response barrier 保持。

## 性能证据

完整参数与原始输出见 measurements.json 和 measurements/。debug build、同机同 target、没有同时编译；OS 文件缓存未清空。128 turns 每阶段三次，512 turns 各一次探索。统计只计 session/load，不含生成/fsync fixture 的时间。

| 阶段 | 128 turns 完整加载中位数 | 128 turns actor 中位数 | 512 turns 完整加载（单次） |
| --- | ---: | ---: | ---: |
| 修正 fixture 后的 before | 503.8 ms | 115.2 ms | 1465.7 ms |
| A1 Timeline 所有权复用 | 491.3 ms | 110.2 ms | 1423.4 ms |
| 128 行分批 drain 实验 | 533.4 ms | 89.0 ms | 1692.2 ms |
| 撤回分批，保留 Grow-only decode | 493.7 ms | 93.1 ms | 1481.1 ms |

actor 阶段约减少 19%，但 128 turns 完整恢复仅约 2% 的中位数变化，512 turns 单次没有改善；不足以声称端到端问题已解决。分批 drain 虽让部分通知更早到达，却显著拖慢完整加载，已撤回。不能为了缩短 loading 标志而提前放开接纳屏障。

## 正确性

shell 全量中 Grow projection 的混合方法、64 KiB ACP 内容、rewind、未知/坏 payload、非完整尾行、通知身份参考等价测试通过；既有 rewind/cursor、pinned 文件、Workflow 与 load 恢复测试通过。

Pager 全量：7198 passed、10 ignored。新增 loading_replay_preserves_typed_prompt_until_session_loaded 从真实 LoadSession Action 建立恢复窗口，字符逐个通过正常 handle_input，断言要求 redraw 且 composer 保留；Enter 走正常输入 Action，load 前不发 SendPrompt，SessionLoaded 后原文被发送。它检查状态与 redraw 请求，不能证明真实终端帧耗时。

历史完整顺序的独立 ACP 校验与 CLI/PTY 已通过，见后文。PTY 没有可控 loading 完成钩子，因此终端输入 p95 ≤ 100 ms 尚未测得。长文本/密集工具/多子会话、resident/cursor 性能矩阵仍需后续采样，不能从本样本外推。


## 实际 CLI 恢复

`CARGO_BUILD_JOBS=2 cargo build --locked -p cli --bin grow`：通过。debug 链接器报告 __eh_frame 大小警告，与此前 perf 构建相同，退出码为 0。

`PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --test pty_e2e_persistence continue_resumes_session_with_history -- --ignored --nocapture --test-threads=1`：1 passed，8 filtered。首次停在无 LLM 配置的欢迎前检查；原 fixture 直接 spawn，没有调用现有 seed_llm_config。现只在该测试补齐隔离 mock 配置，重跑进入真正交互并通过：同 cwd 退出后 --continue，历史恰好一次，后续 turn 正常完成。没有修改生产配置门槛或扩大到整个 PTY 家族。


## 最终历史与磁盘检查

perf 与 test_session_load_memory 两个 harness 的 --no-run 编译通过。单独设置 GROW_PERF_ASSERT_HISTORY=1，12 turns 默认小样本与 128 turns（8 chunks、4096 bytes）分别 1 passed；全部历史文本在 load response 前与 typed reference 内容、顺序完全一致。大型 RSS/dhat 场景未运行。

本轮新建增量缓存已按创建时间清单清理：244 个新目录回收，36 个原有目录及所有可运行二进制保留，可用磁盘约 43.2 → 61.0 GiB；清单归入 reuse-validated-resume-timeline/disk-cleanup.json。无新 worktree 或真实会话复制。

归档前 `openspec validate --all --strict --no-interactive`：19 passed，0 failed。

归档完成后：`openspec validate --all --strict --no-interactive` 为 17 passed、0 failed；`openspec validate --archived --no-interactive` 为 327 passed、0 failed。`git diff --check` 通过。原有三个进行中的 change 保持不动。
