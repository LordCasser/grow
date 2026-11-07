# Verification — 2026-11-06

特性已实现并通过实际权限入口、能力投影和独立源码 review；纳入2.6.2工作树，未发布。

父在 macOS、ripgrep 15.2.0 使用临时目录的固定 hostname helper，只写同目录标记并打印固定域名：普通搜索不执行；`--hostname-bin FILE` 与 equals 形式执行；`--hyperlink-format=none --color=never` 仍执行。review又验证 `--glob -- --hostname-bin HELPER needle FILE` **exit0且marker=true**，推翻首轮候选“遇 `--` 即停止扫描”。`--regexp --` 控制亦执行但extraoperand造成exit2，仅作初始化副作用证据，不当成功搜索。`--hostname-b` 被CLI拒绝且不执行，不构造缩写漏洞。实验脚本和临时输出分别在 `/tmp/grow-rg-hostname-probe.py`、`/tmp/grow-rg-hostname-probe-review.log`；归档结论不依赖这些临时文件今后仍存在。

生产改动归 Workspace permission：共享 `pub(super) rg_selects_external_program` 识别 rg head及两个执行选项，两种参数形式；沿既有pre检查策略保守扫描全部参数，安全/always-safe、prefix grant例外和Auto routine共用。Shell生产投影不改，通过既有 `command_is_known_observational` 读到同一事实。显式deny优先；普通rg prefix不能授权任意helper；精确invocationgrant与always-approve既有用户策略保持。不新增公共API或完整rgoptionparser。

Rust1.93.1、locked/offline、RUST_MIN_STACK=16777216、CARGO_BUILD_JOBS=2，单一Cargo窗口实际验证：

- 初始代码的hostname缺陷：`/tmp/grow-rg-permission-old.log`，8次失败观察含重叠filter；真实Ask actor原0prompt静默允许、Auto routine允许、prefixgrant/observational及ShellRX投影失败。重复filter不计独立覆盖。
- 初候选的separator错误：`/tmp/grow-rg-separator-candidate-red.log`，**Workspace5 failed / Shell2 failed，两次exit101**。真实Ask0prompt、prefixAutoAllow、heuristicAllow和observationaltrue均在glob/regexp用例处失败，Shell两个冻结/直接投影也失败。只临时恢复旧helper的早退分支，跑完删除该唯一分支；manager文件SHA256与探测前逐字节一致，其他dirtyhunk未reset。
- 最终代码：`/tmp/grow-rg-permission-final-workspace.log`，**19 passed / 0 failed，exit0**；safe-command、Ask、Auto、prefix/exact/deny、scope与普通搜索控制。`/tmp/grow-rg-permission-final-shell.log`，**2 passed / 0 failed，exit0**，实际shell_required_access及typed ToolInput冻结参数投影为All，不可由RX ceiling包含。

此前Shell green被邻居未编译的迟答问答测试宏/borrow错误阻塞；原owner修正并跑14相关回归后，本项在最终代码上重跑通过，不用那次编译失败掩盖权限回归。独立review `01a1b008-4b7d-7701-a3cd-df7283d1b3de` 亲核sharedhelper、deny顺序、exact/prefix、字符串旁路、Auto与delegation，没有blocking发现；父亲核机制反例、最终源和日志接受。

限制：同形字面pattern（如`rg -- --pre ...`）可能额外复核，这是保守策略；完整rg语法与其他可执行选项未新增建模。机制实验与Grow真实权限决定/冻结投影分别验证，没有宣称执行未授权破坏操作或复现Windows现场。未改真实用户config/session，未提交/推送/发布。
