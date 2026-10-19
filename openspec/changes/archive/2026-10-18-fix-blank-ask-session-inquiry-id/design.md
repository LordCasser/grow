## Context

协调询问的 backend 接受可选 inquiry ID：缺失时由 backend 生成 UUIDv7；显式值必须是 UUIDv7。模型工具 schema 允许可选字符串，但实际模型可能传空字符串，而空字符串不是 UUIDv7。null 正常解码为 `None`，由 backend 按缺失 ID 处理；问题限于模型工具入口的空白字符串归一化。

## Decision

在 `AskSessionTool::run` 转交 backend 前，仅将 `inquiry_id` 的空串或仅空白串归一化为 `None`。非空字符串保持逐字原样，backend 继续执行 UUIDv7 校验；因此周围带空格的 UUID、非 UUID 和非 v7 UUID 均被拒绝。工具参数文案建议省略该字段，并说明 null 与空白字符串都会请求新 ID；非空 UUIDv7 在重试时复用同一询问身份。

归一化只位于模型工具入口。外部协调协议/ACP 对显式 ID 的验证与 `get_inquiry` 查询保持严格，不把空 ID 转换成新询问，也不触及其他 session。

## Validation

回归覆盖缺失/null/空白新建、合法 UUIDv7 重试复用、非法非空值拒绝，以及协议显式空 ID 仍拒绝。任务验证完成前保持未勾选。
