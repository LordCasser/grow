## Why
fix-doctor-shell-config-overrides修复了规划和apply后的检查，但独立doctor报告仍通过shell_home_and_kind读取默认文件，可能对已完成的自定义目录修复继续报错，或把未使用的默认配置误判为已配置。

## What Changes
报告复用FixRequest同一目标解析和配置检查，移除该调用方的重复默认路径解析。

## Impact
独立doctor报告及CLI修复列表的SSH状态；tmux目标选择项目保持独立进行中。
