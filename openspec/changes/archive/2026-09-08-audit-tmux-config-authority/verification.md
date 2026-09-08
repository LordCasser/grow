## Local evidence
TmuxCommand仅Version/OptionValue/OptionSupport/ControlMode；TmuxFacts只有extended_keys、set_clipboard、allow_passthrough支持与值。没有配置来源事实。TerminalContext::tmux_config_path返回固定默认提示，FixRequest路径独立取HOME或有效BYOBU_CONFIG_DIR。

## Upstream evidence (2026-09-08)
[tmux.c](https://github.com/tmux/tmux/blob/master/tmux.c)：TMUX_CONF扩展为cfg_files，-f可替换并追加。
[cfg.c](https://github.com/tmux/tmux/blob/master/cfg.c)：start_cfg迭代cfg_files调用load_cfg；load_cfg错误不把列表改为成功加载日志，后续source过程亦不在此追加列表。
[format.c](https://github.com/tmux/tmux/blob/master/format.c)：format_cb_config_files直接用%s,拼接路径，去掉末尾逗号，没有路径分隔转义。含逗号文件名与多文件字符串有歧义；不能直接split后认为每项为已验证目标。

## Decision
建立resolve-tmux-config-target：显式目标优先，服务器证据为候选来源，不能把本机当前环境默认值当成server启动环境。歧义应进入可操作的显式目标路径，不静默写入HOME默认文件。此审计没有部署新探测、修改真实配置或执行tmux（本机无可执行文件）。
