## Why

用户确认中文冒号后的提前停止跨 LLM 端点存在，并指出 Timeline 的 end_turn 可能是 Grow 自行补写。必须分别核对 HTTP body、归一化响应和 Turn 终态，不能以内部标签反推 provider 原始输出，也不能把协议结束当成任务完成。

## What Changes

审计原会话与补充会话 `01a08b02-acf9-7c33-8b0b-cf474d521d09` 的原始采样 artifact、采集代码及结束映射；保留明确的证据边界，把整体提前停止问题继续登记为未解决。

## Capabilities

纯审计与 backlog 记录，`skip_specs: true`；不改变采样或 Turn 契约，不增加标点启发式，不把未定位的第二个停顿场景编造成回归。

## Impact

只增加本 change 下的审计材料并更新 backlog。沿用前一变更的源码修复，不安装或重编译大型二进制，不操作用户会话中的工具。
