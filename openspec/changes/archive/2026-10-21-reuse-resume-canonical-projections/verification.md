# R6 验证与现代恢复对比

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

## 口径

R6在R5验证及独立计时之后实施。before与R5原始样本见[独立R5记录](../2026-10-21-optimize-timeline-writer-prefix/verification.md)；R6同样5组各3次独立进程，现代合法admission/projection、空candidate anchor、匹配当前mock route，主矩阵每轮32KiB，setup不计入load RPC。性能统一为macOS ARM64、Homebrew Rust 1.99.1、unoptimized debug=0/incremental=0/jobs=2；未与本任务编译/正确性用例并行。主机有其他项目负载，保留全部样本和范围，不视为release承诺。额外固定1.93.1门禁见收尾。

## 完整 RPC

| profile | before ms | R5后 ms | R6后 ms | 总耗时减少 |
|---|---:|---:|---:|---:|
| modern-128 | 1267.9 | 1201.6 | 997.5 | 21.3% |
| modern-512 | 4401.0 | 3700.1 | 2871.8 | 34.7% |
| modern-2048 | 18765.8 | 13544.7 | 9862.0 | 47.4% |
| modern-repair-512 | 4485.4 | 3881.8 | 3080.6 | 31.3% |
| modern-short-512 | 1694.5 | 1229.1 | 1104.9 | 34.8% |

三次中位数，2048轮R6范围9.793–11.118秒（before18.188–19.534、R5后13.446–13.597）。原始R6十五次、三阶段摘要、产物身份。每条source指向完整stdout/stderr。R5前后的样本保存在另一个独立change，不混用最早四项优化前的HEAD样本。

## 收益来源

2048轮首条历史约11.102→10.386→6.692秒。R5移除writer前缀生命周期累计复制，主要缩短回放后的actor启动；R6消除同次load的第二遍canonical构造，并减少exact投影JSON重编码，主要提前历史开始。

R5后raw response reconcile中位数约4.080秒，R6后0.414秒；typed canonical仍需约2.728秒，raw canonical在匹配route的所有正常样本中为0次（构造次数有断言）。这是消除重复工作，不是省略physical projection解码与exact payload校验。缺route时追加Timeline事件后，raw canonical重新构造一次（亦有断言），完整历史验收通过。

R6后2048代表性中位总量运行：load_light约4.252秒，main replay3.247秒，spawn_actor_call1.744秒；这些层级有嵌套，不能直接与其子phase重复相加。replay含forward约1.576秒和completion约0.926秒。剩余成本包括一次正文digest、JSON解析、独立writer前缀验证和外发链；未扩展实施其他优化。

## 正确性

- JSONL180、persistence82、response projection19、raw storage57、MvpAgent148、communication history10、transcript42、现代fixture1，合计539 passed、0 failed，5项重型/性能用例默认ignored且另行精确运行。文件为r6-correctness-*.txt。
- 新回归覆盖：带原始空白/键顺序的exact行借用到candidate anchor；重复exact抑制且cursor强制完整回放；identity/digest相同但正文不同的后续副本仍InvalidData；missing exact仍构造canonical。借用的canonical仍是Synthesized来源，保留离线simulated_source及时间优先级。
- 现代128/2048、512每16轮缺投影、512缺模型路由基线四次完整FIFO验收全部通过。核对所有文本、顺序、恰好一次、load响应前完成及rewind字节不变。见r6-correctness-history-*.txt；这些保留完整oracle，不纳入timing。
- 旧回归覆盖前缀hash/长度/语义、同长篡改/替换、torn tail、file/directory sync失败重试、rewind/fork继承、resident snapshot frontier、迟到delta及cursor，不因缓存命中跳过。

## 真实CLI/PTY

现代正常128/512精确目标运行3次；慢写512运行1次。512正常SessionLoaded ready中位数：before7757.8ms（1次）→R5后约7090ms（2次）→R6后6226.1ms（3次）；历史末尾首次可见约6582.7→6406→5501.1ms。PTY catalog负载比Shell矩阵大，不将二者时间差称作纯TUI成本。

R6慢写512：首条可见2251.2ms、末尾5608.2ms、ready6236.9ms，12次可见历史推进，最大间隔349.2ms；ready输入p95 52.8ms，普通loading输入忽略，实际双Ctrl+D退出code0。正常512三次ready输入p95均低于8ms。

128首轮偶发CLI进程启动空档，R6初轮history6640.4ms/ready6876.4ms，复跑history1959.2/2164.9ms、ready2178.0/2505.8ms；全部保留，不归因于R6算法或删除异常样本。R5亦出现类似空档并独立复跑，故性能结论以RPC矩阵及512稳定场景为主。

记录：r6-pty-*.txt。中途Ctrl+D再恢复的新现代目标，以及固定工具链门禁结果见下文。

复跑参数、精确枚举和构建方法沿用[R5](../2026-10-21-optimize-timeline-writer-prefix/verification.md)，R6无需新性能入口。默认route应typed_canonical=1/raw_canonical=0；GROW_PERF_OMIT_ROUTE_BASELINE=1时raw=1。所有phase为inclusive。

## 固定工具链门禁命令

显式将 `~/.cargo/bin` 放在PATH最前，实际cargo/rustc均1.93.1，目标目录为本任务独占临时目录（见pinned-build-context.json），与正式A/B产物隔离。依次执行：

```sh
cargo check --locked --offline -p cli --bin grow
cargo test --locked --offline -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
cargo build --locked --offline -p cli --features shell/test-support --bin grow
```

所有命令仍使用incremental=0、dev/test debug=0、jobs=2；固定工具链的回归结果单独保存，不混入1.99.1性能中位数。

固定1.93.1结果：默认CLI check通过；Shell/Pager library、session_load_perf及PTY target构建通过；同一539项定向回归再次全部通过（0 failed，5 ignored），原始输出为r6-pinned-correctness-*.txt。这是两个工具链上的同一组用例，不把重复运行计作更多独立覆盖。CLI/PTY门禁结果见下文。

固定1.93.1收尾：真实CLI构建通过；现代512轮加载中双Ctrl+D退出code0，再次恢复必须经过新的SessionLoaded、最后历史标记及非Loading稳定画面，随后退出code0，精确目标通过。慢写现代512亦通过，ready输入p95 86.5ms、max96.6ms（阈值100ms）；该组为固定工具链正确性探针，不并入1.99.1性能对比。见pinned现代退出恢复、pinned慢写历史。

收尾：两项独立skip_specs change均已归档；全量strict 16 passed、archive strict 674 passed，均0 failed；Rust1.93.1的28个相关Rust文件格式检查、git diff --check、change链接及证据未忽略检查均通过。清理本次独占固定工具链target、测试可执行文件/dep-info与临时脚本，合计回收约11.15 GiB；保留当前grow和可复用依赖缓存，不触碰既有incremental。复跑需按记录重新no-run构建。
