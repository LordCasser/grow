## Evidence
旧路径选择由ShellKind::config_path固定HOME/.zshrc及HOME/.config/fish/config.fish，FixRequest未捕获ZDOTDIR/XDG_CONFIG_HOME。官方路径依据见design中的zsh与fish文档。

实现捕获环境为请求字段，shell_config_path仅校验当前shell相关覆盖。测试使用隔离目录直接构造请求，不修改主机环境。custom config含空格；zsh/fish预览目标正确，apply与managed_alias_is_configured成功，HOME默认目录未创建。不安全相对覆盖拒绝，Bash忽略无关覆盖，空XDG采用默认。

`cargo test --locked --offline -p pager --lib diagnostics::fix --quiet`：31 passed，0.31s。`cargo test --locked --offline -p pager --lib doctor --quiet`：54 passed，0.07s。两组可能重叠，不合并为独立测试总数。现有macOS compact-unwind linker warning，构建和测试均exit0。

未启动用户shell脚本，未修改真实配置；环境之外动态重设或未导出的shell变量不可见。tmux自定义加载路径不在本次范围。全量规范严格通过，归档后再次校验。

测试后确认无cargo/rustc进程，cargo clean移除9176文件、3.8 GiB；可用空间恢复74 GiB。
