## Setup
macOS 当前main工作树；1 MiB无托管块正文，1/16/64个短条目，每组3次实际ManagedConfig::plan，不含写源文件时间。隔离tempdir；probe.rs保存源码，仅测量期间追加tests.rs，完成后精确移除。

## Results
|条目|test未优化 samples μs|中位 μs|release samples μs|中位 μs|
|---|---|---|---|---|
|1|50881,51161,51559|51161|2591,2758,3224|2758|
|16|344563,344835,345106|344835|21641,21779,22461|21779|
|64|1288409,1290120,1292472|1290120|69529,84515,85654|84515|

命令：`cargo test --locked --offline -p config --lib measure_managed_plan_batches -- --ignored --nocapture`；第二轮加`--release`。禁用incremental，jobs2/debug0。两轮各1项通过；release编译15.45s，测量测试0.31s。

## Decision and limits
确认数量增长带来显著重复扫描成本，单条release约2.8ms，不声称常见doctor单条卡顿。独立立项batch-managed-config-render，保留格式和未知条目行为，用同一探针比较。这里是少量本机样本，不是release-dist、全TUI或跨平台性能保证。
