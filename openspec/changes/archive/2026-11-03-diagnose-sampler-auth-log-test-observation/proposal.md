## Why
历史并行suite曾记录auth日志test缺client_post或sampling_request，但没有保留原始失败日志。两轮当前完整并行suite与100k callsite探针均未复现；不能凭猜测加入serial/retry/全局cache刷新。

## What Changes
保留全部wire/positive log/无secret断言，为缺字段错误补实际capture日志。记录已执行调查、原历史证据缺口和无reproducer的候选结论；不宣称定位或修复tracing bug，不用进程隔离掩盖未证明的耦合。

## Impact
仅test失败诊断与调查文档，不改变采样、日志生产路径、tracing订阅或产品契约，skip_specs。
