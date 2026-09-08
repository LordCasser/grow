## Design
RenderedUpdate可同时返回requested item states，使plan不再逐条调用item_state解析原文。render_update保留一个initial ParsedBlock，根据原范围生成有序替换；不存在条目按请求顺序集中插入outer close之前。缺少outer时将新sections按原换行组合交给append_outer。保留未知条目原位置、无关正文、最终换行与CRLF规则，末尾仍parse_block校验完整输出。

## Verification
覆盖已有条目请求顺序不同于文件顺序、新旧混合、未知条目、空/非空源、CRLF与末尾换行、非法标记拒绝。对照旧算法输出（测试夹具或冻结基线），再复用measure-managed-plan-batches/probe.rs比较release数据。不能用更弱的最终校验交换性能。
