# 验证记录

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

2026-10-21，本地 macOS；基线 HEAD 为 a1d1c2965075c87645eab22adce6d48b19410c59，优化结果包含当前工作树改动。所有数据均为合成历史、dev/test 未优化构建，不能当作 release 或真实用户会话的速度承诺。

构建统一使用 CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_BUILD_JOBS=2：

```sh
cargo test --locked --offline -p chat-state -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
```

先通过已编译产物的 --exact --list 确认每个 ignored 基准恰好一项，再以 RUST_MIN_STACK=16777216、--exact --ignored --nocapture --test-threads=1 串行运行三次。fixture 建立与正确性断言在计时窗口外，前后规模、输入及构建参数一致。性能测量时没有并行启动本任务的编译或测试；共享机器其他负载仍会带来噪声。表格为中位数（最小–最大），原始日志保留全部三轮结果。

## 测量

计时恢复写路径的规划决策：全部 existing projections 已 exact 存在，旧路径为 reconcile + 对每项重新扫描 existing，新路径直接取得 missing canonical append plan。两者都必须判断无追加。

| 规模 | 优化前（ms） | 优化后（ms） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 2.517（2.403–13.303） | 1.914（1.839–1.979） | 1.31× |
| 512 | 10.361（10.225–17.849） | 7.645（7.608–7.728） | 1.36× |
| 2048 | 55.171（53.701–62.164） | 31.896（31.689–32.323） | 1.73× |

优化后探针还在同一二进制中测了旧等价路径：2048 条时中位数 48.839 ms，新 append plan 为 31.897 ms，因此收益不只依赖跨轮机器负载差异。该路径仍须构造 canonical 并完成 branch/planner 校验；本 change 不缓存 Timeline branch。

## 正确性与对抗性复核

Projection 模块 19 passed、0 failed、1 ignored，见 日志。覆盖 exact conflict、candidate anchor、duplicate physical exact records、suppression、尾部排序安全、resident delta、rewind 与 inherited branch。特别区分“planner 需要 materialize canonical”和“物理 exact 缺失”：已有 exact 即使被 canonical materialization 替换，也不得再次追加。

JSONL 与持久化集成回归分别 179 和 82 passed，见 [存储 change](../2026-10-21-index-exact-update-commits/verification.md)。调用方直接追加 planner 明确指出缺失的 canonical，保留 durable append 及失败处理。

## 端到端恢复

fixture 使用合法 legacy Timeline，没有现代 response admission 身份，主要检验实际 load、通知顺序与持久历史完整性；它不能充分代表上述 identity-heavy 优化的总体收益。

## 原始数据

优化前、优化后。

## 端到端实测数据

同样的 8 chunks/turn、4096 bytes/chunk，优化前后各三次 timing；另以 GROW_PERF_ASSERT_HISTORY=1 单独执行 128/512 两项完整历史断言，不把断言开销纳入 timing。

| turns | 优化前 load round-trip 中位数（范围，ms） | 优化后（ms） |
| --- | ---: | ---: |
| 128 | 748.8（719.5–793.5） | 710.8（691.6–766.8） |
| 512 | 1659.1（1654.5–4966.7） | 1681.3（1590.6–1817.8） |

这个 legacy fixture 的端到端基本不变，不能宣称整体恢复加速。所有六次 timing 与两次 history 验证通过；history 检查 load response 前文本完整顺序，所有运行还检查 rewind_points.jsonl 的指纹不变。

128 前、128 后、512 前、512 后、128 完整历史、512 完整历史。

## 收尾

受影响正常回归共 1286 passed、0 failed；被忽略的性能探针另行精确运行。四项优化各有独立 change，不扩大到审计中的其他候选。完成格式、diff 与 strict OpenSpec 校验后归档。

## 归档校验

四项 change 各自完成 tasks 后独立归档；只有加载交互修改 client-surfaces 主规范，三项内部优化使用 skip_specs。归档前全量 strict 为 20 passed；归档后全量 strict 为 16 passed、archive strict 为 671 passed，均 0 failed。修改 Rust 文件的 rustfmt 检查、git diff --check 和本组文档链接检查通过。原始 stdout/stderr 使用 .txt 保存，避免被仓库的 *.log 忽略规则排除。

验证证据保存后，已删除本次不再需要的测试可执行文件及对应 dep-info，共 1.59 GiB；保留当前 grow 和可复用依赖缓存。所有构建均禁用 incremental，未增加 incremental 残留。复跑请先执行上面的 no-run 构建并重新枚举产物。
