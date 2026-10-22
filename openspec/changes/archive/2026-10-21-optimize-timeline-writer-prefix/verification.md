# R5 验证与基准

环境：macOS ARM64，HEAD a1d1c2965075c87645eab22adce6d48b19410c59 加此前四项已授权优化。本次 before 为同一工作树的R5/R6前产物；r5只含R5，不含R6。所有构建禁用incremental，dev/test debug=0，jobs=2。性能口径为Homebrew Rust 1.99.1 debug，先编译再直接运行二进制，不与本任务构建/测试重叠。宿主其他项目可能有负载，各组保留三次原始运行，不能视作隔离环境或release数据。仓库固定1.93.1的额外验证见后续收尾记录。

## 现代长会话

沿用生产projector、合法request completed/admission、payload-free candidate anchor和当前mock model route fixture。每轮一个user和一个canonical assistant；主矩阵正文8×4096字节，短正文1×4096。setup/fixture生成不计入RPC和phase窗口。128/512/2048、512每16轮缺一个projection、512短正文各三次。每次检查历史数量、物理投影repair数量、rewind字节不变、旧catalog抑制与live catalog到达。完整FIFO内容断言另项运行，不混入timing。

| profile | before RPC ms | R5 RPC ms | before actor call ms | R5 actor call ms |
|---|---:|---:|---:|---:|
| modern-128 | 1267.9 | 1201.6 | 179.4 | 146.1 |
| modern-512 | 4401.0 | 3700.1 | 854.7 | 495.3 |
| modern-2048 | 18765.8 | 13544.7 | 6311.3 | 1731.2 |
| modern-repair-512 | 4485.4 | 3881.8 | 784.6 | 477.5 |
| modern-short-512 | 1694.5 | 1229.1 | 490.6 | 187.8 |

三次中位数；phase为嵌套inclusive计时，不能与RPC相加。2048轮总恢复降低27.8%，actor call约6.31→1.73秒。R5消除了累计LifecycleFold复制；前缀读取、JSON解析、合法性验证和durable写入仍存在。512短正文的固定生命周期成本也下降。首条历史在R5前后约11.10→10.39秒，但此路径发生在回放之后，不能把这部分TTF波动归因于R5算法；下一项R6处理前置正文成本。

[原始before15次](before-measurements.json)、[R5后15次](r5-measurements.json)、[摘要](comparison.json)、[构建身份](builds.json)。每条JSON的source对应完整stdout/stderr文本。

复跑：
```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
GROW_PERF_SESSION_FORMAT=modern GROW_PERF_TURNS=2048 GROW_PERF_AGENT_CHUNKS_PER_TURN=8 GROW_PERF_AGENT_CHUNK_LEN=4096 RUST_MIN_STACK=16777216 <session_load_perf-executable> full_session_load_e2e --exact --ignored --nocapture --test-threads=1
```
目标先--exact --list确认只有1项；每组启动三个独立进程。repair加GROW_PERF_MISSING_PROJECTION_EVERY=16。

## 正确性与TUI

- JSONL：180 passed、0 failed、1 ignored。包含原始hash/CRLF/空格、read上界、超限、语义非法JSON完整前缀及不修改文件、同长篡改、实体替换、torn tail、同步失败与exact retry。见[r5-correctness-session-storage-jsonl.txt](r5-correctness-session-storage-jsonl.txt)。
- persistence：82 passed、0 failed。见[r5-correctness-session-persistence.txt](r5-correctness-session-persistence.txt)。
- 现代PTY正常128/512与慢写512均通过：ready后输入p95约7.5/52.6ms，慢写历史11次推进、最大可见间隔346.9ms；普通loading输入忽略，实际双Ctrl+D退出code0。512正常ready从7757.8→7086.1ms，历史可见6582.7→6405.6ms。
- 正常128首轮有约4秒进程启动外部空档，进程ready6423.5ms，但load内部1841.6ms；保留该异常样本。追加复跑128历史2231.7ms、ready2566.3ms；512历史6406.7ms、ready7094.4ms。不能把启动空档误归因于前缀算法。
- TUI原始记录：before-pty-*.txt、r5-pty-*.txt及[r5-pty-normal-repeat.txt](r5-pty-normal-repeat.txt)。PTY catalog负载大于Shell矩阵，不能把二者时间差全称为TUI开销。

最终现代完整文本FIFO、model route fallback、Ctrl+D中途退出后重开和固定工具链校验均通过，见下文及R6记录；它们验证最终组合产物包含的R5路径，R5独立性能样本不受R6影响。

固定Rust1.93.1补充：默认CLI check及测试构建通过；最终组合产物的JSONL180项、persistence82项与其余受影响回归（共539项）再次通过，包含本项非法lifecycle前缀及durable失败路径。完整固定工具链证据随R6保存，性能A/B仍使用统一1.99.1口径。

最终组合产物的固定1.93.1真实CLI及现代Ctrl+D中途退出后恢复、慢写历史推进均通过。首进程退出code0，第二进程等待自己的SessionLoaded完成后退出code0；与本项前缀/耐久回归共同核对退出后可恢复性，证据见R6固定工具链记录。

收尾：两项独立skip_specs change均已归档；全量strict 16 passed、archive strict 674 passed，均0 failed；Rust1.93.1的28个相关Rust文件格式检查、git diff --check、change链接及证据未忽略检查均通过。清理本次独占固定工具链target、测试可执行文件/dep-info与临时脚本，合计回收约11.15 GiB；保留当前grow和可复用依赖缓存，不触碰既有incremental。复跑需按记录重新no-run构建。
