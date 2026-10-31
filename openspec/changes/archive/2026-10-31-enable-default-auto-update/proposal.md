## Why

Settings 已将未配置的 auto_update 显示为 true，但更新器两个入口要求 Some(true)，导致默认不检查更新、也不触发提示。完整 TUI 的既有提示仅在欢迎页可见，检测完成时用户可能已进入会话。

## What Changes

- 未配置 auto_update 时默认启用更新检查与自动下载，显式 false 继续关闭。
- 完整 TUI 检测到更新时显示即时 toast，保留欢迎页的版本与重启提示；minimal 保留原有 scrollback 提示。
- 保留现有缓存、版本策略、禁用参数与发行包管理边界。

## Impact

update 的两个自动更新入口、Pager 更新完成通知与 client-surfaces 契约。
