## Why
FPS HUD 是实际隐藏调试能力，需要核对它在各渲染模式中的可达性和统计含义。源码显示 minimal 提前返回绕过采样与显示，与通用 debug 开关报告不一致；旧 profiler 注释也可能误导实现。

## What Changes
记录 minimal FPS 缺口及采样边界，建立后续独立修复项。不修改运行时契约，跳过 delta specs。

## Impact
仅审计与 backlog，不移除 FPS 功能，不改帧时钟。
