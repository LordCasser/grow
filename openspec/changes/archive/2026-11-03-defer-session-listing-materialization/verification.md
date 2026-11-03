# 验证

2026-11-03；独立临时GROW_HOME、RUST_MIN_STACK=16777216、Cargo --locked。

- 改生产前：`cargo test --locked -p shell --lib session::listing::tests -- --include-ignored --nocapture --test-threads=1`，3 passed。对旧reference和原生产入口做全行serde差分，建立计时基线。
- 实现后相同命令：3 passed；旧reference与新入口同进程对照。每规模2 warmups+7 samples、limit30、input clone和output drop在计时外，所有row有stored worktree label。全部原始样本和临时probe源码见[measurement](measurement.md)。

| Summary数 | 实现后同进程旧reference中位(ms) | 新入口中位(ms) |
| --- | --- | --- |
| 128 | 0.307541 | 0.093291 |
| 512 | 1.103500 | 0.223584 |
| 2048 | 3.345042 | 0.585584 |
| 9864 | 14.937208 | 2.782458 |

这是本机debug纯计算/行构建阶段，输入已在内存，排除磁盘/DB/响应序列化/ACP/Pager。实施前单独进程原生产9864中位15.015084ms，old reference15.144625ms；不把跨进程/CPU频率偏差当额外收益，不声称RSS或生产吞吐。

- 移除临时probe并补leap秒后最终 `cargo test --locked -p shell --lib session::listing::tests -- --test-threads=4`：2 passed，0.01秒。全行差分保留query大小写/Unicode/ID/title、零/边界/最大limit、空cwd与非空同cwd、纳秒、完全同key稳定顺序、缺last-active fallback、MIN/MAX UTC及扩展年、leap second；字段仍由原summary_to_session构建。
- `cargo test --locked -p shell --lib unified_list -- --test-threads=4`：3 passed；现有request filters/facets/cursor稳定与无重复回归。该套不是9,864summary磁盘benchmark，未虚报真实IO基准。

独立review最终通过；rustfmt与git diff --check通过。没有新增缓存、索引、分页或timestamp输入约束，不变契约，skip_specs。临时probe移到measurement记录，不加入默认测试/发布构建。

归档前strict18/18；归档后全量strict17/17、archive tasks789/789通过。
