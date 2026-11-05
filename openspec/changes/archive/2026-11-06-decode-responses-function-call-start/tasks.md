## 1. Evidence and contract

- [x] 1.1 核对指定会话两次失败及同会话成功对照、锁定 SDK 和 client/L2 的字段与完成校验边界。
- [x] 1.2 以合成无副作用 SSE 重现初始 function_call 缺 arguments 的原始失败。

## 2. Implementation and acceptance

- [x] 2.1 仅规范化显式 in_progress 的 function_call added 缺省空前缀，保留原 evidence 和所有完成/身份校验。
- [x] 2.2 通过实际 client→L2 red/green、错误类型/done/terminal/EOF 控制及相邻回归，固定 1.93.1 fmt 和 warning 检查。
- [x] 2.3 父检查最终 diff 与验证证据，同步说明，strict 校验并归档。
