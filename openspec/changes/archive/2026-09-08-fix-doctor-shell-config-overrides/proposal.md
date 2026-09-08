## Why
SSH修复固定选择HOME/.zshrc和HOME/.config/fish/config.fish，忽略可见的ZDOTDIR与XDG_CONFIG_HOME，可能写入不会被目标shell加载的配置并报告成功。

## What Changes
请求捕获shell配置目录覆盖，规划使用相关shell的安全绝对目录，未设置时保持默认。预览、事务、后置检查沿用同一计划路径。

## Impact
Pager doctor SSH修复及测试；tmux/Byobu路径和Bash默认路径不变。
