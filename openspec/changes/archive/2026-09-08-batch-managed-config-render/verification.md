## Correctness
重构前新增两项批量语义回归2 passed：新旧混合、请求顺序与文件顺序不同、未知条目保留、LF/CRLF、末尾换行、空源和非空源。重构后相同冻结输出通过，并补充foreign.item与外层namespace作为条目时最终解析仍拒绝。

最终 `cargo test --locked --offline -p config --lib managed_text --quiet`：34 passed，2.88s。R17重复快照与断言未删。公开接口不变；RenderedUpdate内部返回原文条目状态，范围基于同一解析，排序后一次拼接，最终parse仍保留。

## Performance
复用measure-managed-plan-batches/probe.rs相同1 MiB源、3样本和release命令。
|条目|原中位μs|新样本μs|新中位μs|
|---|---|---|---|
|1|2758|1730,1927,2155|1927|
|16|21779|1729,1831,1892|1831|
|64|84515|1802,1803,1858|1803|

64条在该本机样本约46.9倍加速；不将跨时间的小样本当作跨平台保证。release探针1 passed、0.02s，临时测试接入已精确移除，保留原探针归档源码。

全量规范16项严格通过，归档后再次校验。target294 MiB/可用74 GiB，无编译进程待完成。
