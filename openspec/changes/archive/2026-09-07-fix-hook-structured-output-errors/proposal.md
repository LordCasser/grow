# Why
Hook 的严格 JSON schema 错误在部分 runner 中被丢弃并按普通日志允许，unknown field、类型错误和截断对象均可能绕过 on_failure=block。命令/HTTP 的 Stop 与 Prompt/Tool 行为也不一致。

# What Changes
共享结构化错误判定：对象/数组前缀的解析失败或 serde data 错误为 Failed，普通非 JSON 文本仍按既有日志容忍。Prompt/Tool 的命令 exit 2 不被解析错误或未知决策覆盖。

# Impact
保留空输出与普通日志行为、成功决策及失败策略。Stop 有效 JSON 的原优先级不变。
