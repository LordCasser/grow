## Why
普通tmux自动修复固定HOME/.tmux.conf，Byobu诊断提示又与自动修复有效BYOBU_CONFIG_DIR不一致。自定义-f、XDG和多配置来源下可能写错文件，现有报告不含任何配置来源事实。

## What Changes
统一诊断与修复的目标选择依据，增加显式目标输入作为自定义/歧义场景的完成路径；采集有界server配置候选，不把未转义逗号列表或查询失败转换为默认路径写入。

## Impact
tmux共享查询、诊断fact、CLI/TUI doctor输入与预览/写入；既有备份、冲突检查、确认和不自动reload保持。
