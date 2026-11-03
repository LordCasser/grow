## Why

Ctrl+X 前缀中仍只按 foreground turn 显示 Stop/Close；但 idle foreground 有 Goal/Workflow/child/queue 时第二按实际停止活动，提示不能再声称关闭。

## What Changes

导引使用既有 activity/row projection 与 CloseSession command 区分停止当前 turn、停止 session 工作、忙碌、重试关闭和关闭。帮助说明同样覆盖后台工作。

## Capabilities

无新增行为契约；只校正已实现命令的展示文案，skip_specs: true。

## Impact

AgentView render 与 shortcut long help。另记录既有组合 PTY 测试最终运行结果。
