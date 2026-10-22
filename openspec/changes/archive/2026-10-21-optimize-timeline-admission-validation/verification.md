# 验证记录

2026-10-21，本地 macOS；基线 HEAD 为 a1d1c2965075c87645eab22adce6d48b19410c59，优化结果包含当前工作树改动。所有数据均为合成历史、dev/test 未优化构建，不能当作 release 或真实用户会话的速度承诺。

构建统一使用 CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_BUILD_JOBS=2：

```sh
cargo test --locked --offline -p chat-state -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
```

先通过已编译产物的 --exact --list 确认每个 ignored 基准恰好一项，再以 RUST_MIN_STACK=16777216、--exact --ignored --nocapture --test-threads=1 串行运行三次。fixture 建立与正确性断言在计时窗口外，前后规模、输入及构建参数一致。性能测量时没有并行启动本任务的编译或测试；共享机器其他负载仍会带来噪声。表格为中位数（最小–最大），原始日志保留全部三轮结果。

## 测量

计时范围仅为 cold Timeline::from_events。每个 response 带合法 completed attempt 与 admission identity，128/512/2048 对应 896/3584/14336 个事件；没有工具调用的健康追加是这项算法的主要重复扫描路径。

| 规模 | 优化前（ms） | 优化后（ms） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 1.471（1.469–1.488） | 0.607（0.584–0.704） | 2.42× |
| 512 | 16.675（16.573–16.880） | 2.591（2.534–2.592） | 6.44× |
| 2048 | 337.225（250.356–594.941） | 10.199（10.071–10.583） | 33.06× |

2048 基线波动较大，250–595 ms；新实现为 10.1–10.6 ms。这里只声称该健康冷折叠路径的改进，不声称 live prepare 全流程或所有 rewind/compaction 变成线性。

## 正确性与对抗性复核

完整 chat-state 回归 569 passed、0 failed、3 ignored，见 [日志](correctness-0-chat_state.txt)。每次基准另外比较恢复 Surface、派生状态及 admission 失败结果。新增覆盖拒绝事件不污染索引、attempt supersession、重复 admission、空 ID/空名称/重复 tool ID、compaction 与 rewind 后的 active Surface 摘要。

新 compaction/rewind 测试最初误将 post-rewind quarantine count 设为 0；旧 quarantine 算法独立 oracle 得到 2。rewind 恢复未压缩分支及原有 malformed assistant，因此只修正 fixture 的预期，保留旧 oracle 与摘要的逐步比对。完整回归与三次基准随后均通过。

索引只在 accepted apply 后更新；completed attempts/admission identities 归不可变事件历史，tool identity 摘要归当前 Surface，两者不混用。替换、compaction、rewind、image projection 后重建 active 摘要。未更改持久格式与契约。

## 原始数据

[优化前](baseline-benchmark_timeline_response_admission_validation.txt)、[优化后](after-benchmark_timeline_response_admission_validation.txt)。

## 归档校验

四项 change 各自完成 tasks 后独立归档；只有加载交互修改 client-surfaces 主规范，三项内部优化使用 skip_specs。归档前全量 strict 为 20 passed；归档后全量 strict 为 16 passed、archive strict 为 671 passed，均 0 failed。修改 Rust 文件的 rustfmt 检查、git diff --check 和本组文档链接检查通过。原始 stdout/stderr 使用 .txt 保存，避免被仓库的 *.log 忽略规则排除。
