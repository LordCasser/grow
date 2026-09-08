## Why
用户在空会话切换 model/agent 后无法选择 Goal/Workflow，resume 补入项目指令时报 replacement shadow set 错误。控制完成后的可用性投影缺少刷新；多上下文 Control 事件在 Surface 上重复使用 item=0。

## What Changes
- 控制工作结束释放前台占用后刷新 Behavior 可用性。
- Control 上下文保留事件内索引，立即投影、延迟激活和 branch replay 使用同一身份。
- 添加空会话控制切换、上下文重建及边界激活回归。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `behavior-goal`: 空闲控制结束后的选择可用性。
- `session-timeline`: 多条控制上下文的独立身份与恢复。

## Impact
Shell 控制 worker、Timeline 投影和对应测试；不修改已有事实、不绕过 causal fold 校验。
