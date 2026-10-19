## Implementation

- [x] 1.1 检查 Chat usage wire 类型、streaming/non-streaming 调用点与 `TokenUsage` 映射，确认可空字段边界。
- [x] 1.2 将可空明细建模为 `Option<u32>`，保留现有 reasoning 聚合和账本边界。
- [x] 1.3 添加 streaming 与 non-streaming 的 nullable/缺失/零/正整数及严格拒绝测试。
- [x] 1.4 添加 HTTP SSE tool-call finish、usage 尾帧与完整结算回归覆盖。
- [x] 1.5 从本机 session 的只读原始响应副本离线复验；不修改原始 session 文件。

## Validation

- [x] 2.1 运行相关 sampling types、Chat stream 与 HTTP 集成验证。
- [x] 2.2 记录验证结果并核对可信总量、缓存字段及 settlement 未受影响。

- [x] 2.3 归档 change，复核全量规范与 archive 校验；独立 change 的失败单独记录。
