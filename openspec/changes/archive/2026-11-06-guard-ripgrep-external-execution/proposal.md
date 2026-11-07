# Why

2.6.2 候选核对发现，Grow 的安全命令、前缀授权例外和 Auto 只排除 `rg --pre`，`--hostname-bin` 仍能被视为普通搜索。父在 macOS、ripgrep 15.2.0 使用临时标记程序确认：空格与等号两种参数都会启动指定程序；即使 `--hyperlink-format=none --color=never` 仍会启动。此实验确认的是 rg 的执行机制，不冒充 Grow 全链越权复现。

# What Changes

将 `--hostname-bin` 的外部执行能力纳入现有 rg 风险判定；安全搜索快速路径、Auto 启发式、默认 always-allow 范围及受限子 Agent 的 RX 投影使用一致结果。保留精确用户授权、现有 deny 优先和普通 rg 搜索行为。

# Impact

只修改命令权限所有者及必要调用边界回归，不扩沙箱或命令解析框架，不顺带改其他上游候选。独立验收真实权限决策 old-red/new-green、可观测无害执行对照和 Shell 能力投影。
