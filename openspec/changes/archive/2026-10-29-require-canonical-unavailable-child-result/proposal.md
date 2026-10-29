## Why
CompletedOutputUnavailable观察在child存在但无SubagentResult时被用来合成Completed事实，后续parent也会被关闭，掩盖不可验证完成。

## What Changes
先读取并验证现有canonical child result；不可验证输出状态没有合成fallback权利，无result时不写child/parent terminal。正常可验证Completed与明确中断恢复保持。

## Impact
Subagent recovery内部fallback参数与真实文件回归。
