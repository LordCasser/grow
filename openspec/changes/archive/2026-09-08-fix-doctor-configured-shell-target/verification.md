## Evidence
全Pager调用搜索发现doctor_cmd::configured_report_for_terminal独立使用shell_home_and_kind和ShellKind::config_path；前一修复的搜索范围只覆盖diagnostics，遗漏此调用方。现改为FixRequest::from_environment与ssh_alias_is_configured，复用既有配置覆盖解析。远程提前返回保留。

新增/增强回归：自定义zsh/fish目标apply前false、apply后true；只有默认目标配置而自定义目标缺失时false。没有修改主机环境变量或真实配置。原重复默认路径helper已从调用方移除，生产全范围搜索无剩余同类调用。

`cargo test --locked --offline -p pager --lib diagnostics::fix --quiet`：32 passed，0.31s；`cargo test --locked --offline -p pager --lib doctor --quiet`：54 passed，0.07s。两组不按独立数量相加。macOS已有compact-unwind linker warning，均exit0。规范17项严格通过，归档后再校验。

resolve-tmux-config-target继续active；保留当前依赖缓存供同模块后续验证，完成该批后清理。
