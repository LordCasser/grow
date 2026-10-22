# 验证记录

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

2026-10-21，HEAD a1d1c2965075c87645eab22adce6d48b19410c59，工作树包含此前四项优化及本轮fixture/诊断。没有实施本轮新发现的R5/R6算法修复。结论、样本限制和主阶段数据见 [分析](analysis.md)。

## 构建与运行

CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_BUILD_JOBS=2；测试RUST_MIN_STACK=16777216。先串行编译，再直接运行已编译产物，避免Cargo再次编译混入性能窗口：

```sh
cargo test --locked --offline -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
cargo build --locked --offline -p cli --features shell/test-support --bin grow
```

每个目标先以 --exact --list 枚举并断言恰好一项；实际运行使用 --exact --ignored --nocapture --test-threads=1。CLI为当前工作树 grow 2.4.3 (a1d1c296)。本任务不并行编译、跑性能用例或采样；普通定向回归允许与编译重叠，不作为性能数据。OS页缓存温热，未清缓存。

复跑正式现代基准，从仓库根目录设置环境并对每个profile独立启动进程三次：

```sh
GROW_PERF_SESSION_FORMAT=modern GROW_PERF_TURNS=512 GROW_PERF_AGENT_CHUNKS_PER_TURN=8 GROW_PERF_AGENT_CHUNK_LEN=4096 RUST_MIN_STACK=16777216 <session_load_perf-executable> full_session_load_e2e --exact --ignored --nocapture --test-threads=1
```

- turns取128、512、2048为32 KiB规模曲线。
- 512 turns、chunks=1、chunk_len=4096为4 KiB正文对照。
- 512 turns、GROW_PERF_MISSING_PROJECTION_EVERY=16为32项repair。
- 512 turns、GROW_PERF_OMIT_ROUTE_BASELINE=1显式重现路由基线缺失。
- legacy对照设置GROW_PERF_SESSION_FORMAT=legacy，512 turns，chunks/len取8/4096与1/32768。其原fixture没有route baseline，不作为现代格式的严格A/B。
- GROW_PERF_ASSERT_HISTORY=1单独跑正确性，不能把保留历史Vec的运行混入timing。

## 覆盖与结果

- 现代fixture的独立字符串oracle、projection补写后重放相同且不重复：1 passed。
- instrumentation窗口排除fixture/返回后工作且保留嵌套count：1 passed。
- 完整投影回归：19 passed，1 ignored。
- JSONL回归：179 passed，1 ignored。
- MvpAgent回归：148 passed，2 ignored。
- 上述正常定向用例合计348 passed、0 failed，4项ignored性能/重型用例另外按精确目标执行。
- 完整现代128/2048及512 repair的历史FIFO、全部文本、load响应边界和rewind文件指纹：128、512、2048，3项 passed。
- 正式现代18次timing均 passed，每次断言2×turns历史通知、物理projection最终恰好turns项、配置匹配的route不补写、rewind字节不变、旧catalog不回放且live catalog可达。legacy6次辅助timing亦通过。
- 首轮缺route现代探索12次与采样诊断1次均通过；不混合不同fixture口径统计。
- PTY两个精确目标各三次，9个规模/慢写场景全部通过：resume_input_latency::resume_modern_admission_projection_128_512_turns；resume_input_latency::resume_modern_history_progress_with_slow_writer。普通loading输入忽略、ready后输入回显与实际双Ctrl+D正常退出都保留。
- 首轮PTY的ready启发式失败与修正原因见 [分析](analysis.md)；没有修改恢复算法或放松输入断言来让测试通过。
- 原始数据为*.txt和JSON，不使用仓库忽略的*.log。

首次编译暴露diagnostic timer含Chrome entered guard而非Send，不能跨storage trait的await。现仅在同步块持有该timer；异步append段只持Instant并在Log模式成功后记录聚合elapsed，保留Send及原错误路径。最终构建和回归通过。

## 产物

正式现代15轮、本轮缺route3轮、含legacy的汇总24轮、首轮探索、采样。各轮对应profile-run-N.txt；采样命令和样本stdout单独保存。

## 收尾

21个相关Rust文件的rustfmt检查、git diff --check、归档报告链接和证据未被Git忽略的检查均通过。归档前全量strict为17 passed；独立skip_specs归档后，全量strict为16 passed、archive strict为672 passed，均0 failed。R5/R6只登记在backlog，未混入本次实现。

原始证据保存后清理了本次测试可执行文件/dep-info与已失败编译诊断残留，回收1.51 GiB；保留当前grow和可复用依赖缓存。构建均禁用incremental。复跑先重新no-run构建并精确枚举实际产物。
