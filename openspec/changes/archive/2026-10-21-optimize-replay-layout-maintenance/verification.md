# 验证记录

2026-10-21，本地 macOS；基线 HEAD 为 a1d1c2965075c87645eab22adce6d48b19410c59，优化结果包含当前工作树改动。所有数据均为合成历史、dev/test 未优化构建，不能当作 release 或真实用户会话的速度承诺。

构建统一使用 CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_BUILD_JOBS=2：

```sh
cargo test --locked --offline -p chat-state -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
```

先通过已编译产物的 --exact --list 确认每个 ignored 基准恰好一项，再以 RUST_MIN_STACK=16777216、--exact --ignored --nocapture --test-threads=1 串行运行三次。fixture 建立与正确性断言在计时窗口外，前后规模、输入及构建参数一致。性能测量时没有并行启动本任务的编译或测试；共享机器其他负载仍会带来噪声。表格为中位数（最小–最大），原始日志保留全部三轮结果。

## 布局测量

128/512/2048 entries，其中一半为 prompts。每组测 64 次干净 prepare_layout，及靠近开头的 8 次 height-only 更新；独立 renderer/full rebuild oracle 在计时外核对每项坐标、height、prompt descriptor 与总高度。

### 每个干净布局帧

| 规模 | 优化前（μs） | 优化后（μs） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 1.167（1.163–1.176） | 0.613（0.613–0.617） | 1.90× |
| 512 | 3.480（3.184–3.508） | 1.079（1.060–1.165） | 3.23× |
| 2048 | 11.501（11.255–11.512） | 2.938（2.859–3.264） | 3.91× |

### 8 次 height-only 更新总计

| 规模 | 优化前（ms） | 优化后（ms） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 2.896（2.743–3.011） | 2.847（2.804–2.900） | 1.02× |
| 512 | 2.903（2.741–2.979） | 2.877（2.785–2.887） | 1.01× |
| 2048 | 3.147（3.104–3.176） | 3.012（2.970–3.018） | 1.04× |

总高度查询改为 O(1)，并不等于整个 prepare_layout 都是 O(1)。height-only 场景包含渲染与需要移动的后缀，此组收益只有约 1–4%，不声称显著提速。完整 bounded replay 的 256 turns frame_total 中位数为 124.66→121.65 ms，约 2.4%，同样不足以声称整体 TUI 大幅加速；[前](baseline-bounded_replay_text_blocks_and_prompt_measurement.txt)/[后](after-bounded_replay_text_blocks_and_prompt_measurement.txt)保留 handler/frame 细项。加载输入从 draft queue 改为忽略，因此 input/load_finish 指标不作同语义性能比较。

## 正确性与对抗性复核

布局模块 285 passed、0 failed、1 ignored；加载派发 64 passed；事件循环 88 passed。见 [布局](correctness-4-pager.txt)、[加载](correctness-5-pager.txt)、[事件循环](correctness-6-pager.txt)。覆盖多处 dirty prompt/entry 的累计 delta、尾部更新保留 prompt 前缀、single-turn 可见范围、fold/group、resize、follow 和手动 scroll anchor。

新多 dirty 测试的独立 full rebuild 参照最初在 FULL_HEIGHT 下恢复 offset，被正确 clamp 到 0；修正参照先回到 VIEW_HEIGHT，再恢复相同 offset。坐标与总高度 oracle 未削弱。加载测试另修正 set_text 后 cursor 位于开头的 fixture 预期。

## 真实终端

CLI 为当前工作树 grow 2.4.3 (a1d1c296)，用 cargo build --locked --offline -p cli --features shell/test-support --bin grow 构建。隔离 Grow home、临时项目和本地 mock provider；PAGER_BINARY 指向 target/debug/grow。每项先 --exact --list 确认恰好一项，再实际运行 ignored PTY。

- [512 turns 慢写](pty-resume_history_progress_with_slow_writer.txt)：40 ms frame writer 注入；首条历史 1848.8 ms，最后历史标记 6003.6 ms，25 次可见推进，历史出现后的最大内容间隔 196.6 ms。
- [512 turns 密集工具慢写](pty-resume_dense_history_progress_with_slow_writer.txt)：首条历史 1747.8 ms，最后标记 1942.3 ms，2 次可见推进，最大内容间隔 194.2 ms。加载越快，观察到的中间帧可以越少。
- [加载中 Ctrl+D](pty-ctrl_d_exits_during_session_loading.txt)：实际确认退出、exit code 0，再次恢复同一会话见到末尾历史，随后正常退出。
- [ready 输入](pty-resume_input_latency_128_512_turns.txt)：128/512 turns 连续 20 次按键回显 p95 为 9.0/7.7 ms；慢写两项 ready p95 为 56.9/52.8 ms。这是加载完成后的单独测量。
- [恢复后新提交](pty-continue-resumes-history.txt)：历史只出现一次、ready 后提交第二轮并收到响应、正常 Ctrl+D 退出。旧用例把首条历史可见当作可输入边界，并用已过时 Ctrl+Q/强制 quit；现改为等待 ready 后发送，使用实际双 Ctrl+D 并断言正常退出，不改变生产加载 barrier。

五项实际 PTY 均 passed。前四项使用统一构建的测试产物，最后一项在最小修正旧 fixture 后重编译 PTY target 并明确枚举、运行。普通输入、Enter、paste、mouse 在 loading 时被忽略，旧 draft 不变；resize、全局 Quit 确认与原 suspension 路径保持可用。自动 paint 仍以既有至少 100 ms 间隔执行，不人为延缓历史处理，SessionLoaded 后恢复正常输入和最终绘制。

本次没有可比的旧版本 PTY 数据，因此这里只验证实际内容进度和正确性，不宣称首屏时间或用户长会话停顿已全部消除。首条历史前仍存在磁盘、解码和 shell 启动阶段；更大/真实 workload 应另测。

## 原始数据

[优化前](baseline-manual_layout_prepare_clean_and_height_only_scales_with_history.txt)、[优化后](after-manual_layout_prepare_clean_and_height_only_scales_with_history.txt)。

## 归档校验

四项 change 各自完成 tasks 后独立归档；只有加载交互修改 client-surfaces 主规范，三项内部优化使用 skip_specs。归档前全量 strict 为 20 passed；归档后全量 strict 为 16 passed、archive strict 为 671 passed，均 0 failed。修改 Rust 文件的 rustfmt 检查、git diff --check 和本组文档链接检查通过。原始 stdout/stderr 使用 .txt 保存，避免被仓库的 *.log 忽略规则排除。
