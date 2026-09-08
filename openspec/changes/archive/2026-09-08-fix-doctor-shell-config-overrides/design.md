## Design
FixRequest新增zsh/fish目录环境快照。Zsh使用已设置ZDOTDIR/.zshrc；Fish使用非空XDG_CONFIG_HOME/fish/config.fish，否则默认。复用SafeAbsoluteDirectory拒绝相对、根目录、控制字符等不安全覆盖，不默默写回HOME；仅验证当前shell相关覆盖。

## Evidence
[zsh Files](https://zsh.sourceforge.io/Doc/Release/Files.html)指定$ZDOTDIR/.zshrc；[fish configuration](https://fishshell.com/docs/current/language.html#configuration-files)指定$XDG_CONFIG_HOME/fish/config.fish。仅使用进程可见环境，不执行用户shell启动脚本推断未导出变量或动态重设。

## Verification
隔离路径覆盖zsh/fish预览与实际apply、原默认路径不写入、后置检查成功；不安全覆盖拒绝，默认和空XDG回退、无关变量不影响Bash。测试不更改主机环境或用户配置。
