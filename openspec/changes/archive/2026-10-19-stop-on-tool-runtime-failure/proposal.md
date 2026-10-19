## Why

工具执行管线的 Err 当前被普通继续分支吞掉，随后再次开启 Step 或 completion recovery，掩盖原始错误并可能越过未确认的工具边界。需要在已有 turn terminal owner 下失败收尾，保留真实失败与未确认状态，避免另造恢复框架。

## What Changes

- 工具运行时基础设施错误停止当前采样循环与 completion recovery。
- 复用现有 terminal transaction 按 Tool、Step、Turn 顺序收尾，不先提交跨越未完成工具的 StepEnded。
- 持久失败诊断；terminal 再失败时保留原错误及二次失败，不伪造确认。
- 增加真实模型工具响应的 admission、result publication、terminal ACK 故障回归及冷历史校验。

## Impact

仅影响 Shell 工具运行时错误传播、相关 terminal 错误证据和定向测试。普通工具失败结果、权限拒绝、MCP 行为与其他 backlog 不扩展。
