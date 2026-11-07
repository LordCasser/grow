# Tasks

- [x] 核对实际安全/Auto/前缀授权/Shell投影路径，并用隔离无害标记程序验证 hostname 参数执行机制。
- [x] 通过真实权限入口和能力投影取得旧版失败证据，完成单一 rg 外部执行分类修复。
- [x] 运行安全命令、Auto、exact/prefix grant 与受限子 Agent 回归。（最终验收见下，workspace 19 green、shell 2 green。）

## Verification (coder, 2026-11-06，最终版)

**初始缺陷 Old-red**（`/tmp/grow-rg-permission-old.log`，真实 exit 101）：workspace **5 个 distinct 测试、6 次失败观察**（rg_hostname_bin filter 3个 + external_program_flags 2个 + heuristic hostname 重复跑1次）+ shell **2个 distinct failed**。含真实权限 actor Ask prompt（旧版静默 auto-allow、0 prompt）、prefix `rg` grant 旧版 AutoAllow、observational 误报、Auto routine 放行、Shell 投影 RX。

**分隔符缺陷 Candidate-red**（`/tmp/grow-rg-separator-candidate-red.log`，真实 exit：WS 101 / SH 101）：以 search_replace 临时还原首轮 helper（遇 `--` return false）后，workspace 5 failed（panics 定位于 `--glob --`/`--regexp --`/`--` 后 flag 各 case：manager.rs 7003/7039/7076/7142、auto_mode.rs 2114），shell 2 failed（tool/mod.rs 1498/1548）。证明保守全参扫描修复的回归检查在旧 helper 上失败。测试断言顺序已核：glob case 先于 literal `--` case panic，无需新增独立测试。跑后严格删除该唯一临时分支，恢复后 manager.rs sha256 与探测前记录完全一致（5942f64f…，/tmp/grow-rg-final-hashes-before.txt），未 reset 任何 dirty hunk。

**最终修复形态**：`manager.rs::rg_selects_external_program`（共享 `pub(super)` helper）保守扫描全部 words，识别 `--pre`/`--hostname-bin`（空格与 `=` 两形式）即取消 observational/routine/safe 捷径；**不做 `--` 短路**（依据：父实测 ripgrep 15.2 `rg --glob -- --hostname-bin HELPER needle FILE` exit 0 且 helper 执行，option-value `--` 使后续 flag-shaped 词不可判为 pattern）；`--pre-glob` 不含；exact grant 保留、deny 优先。`auto_mode.rs` routine 分支共享该 helper；Shell 生产代码未改。无效的 `--link=never`/`--no-hyperlink-format-uri` 假 flag 已换成实测有效的 `--hyperlink-format=none --color=never`。

**Final green**（`/tmp/grow-rg-permission-final-workspace.log`、`/tmp/grow-rg-permission-final-shell.log`，Rust 1.93.1 locked offline，--test-threads=1）：
- workspace（filters: rg_hostname_bin rg_external_program_flags heuristic_guards_rg always_allow_scope command_is_known_observational safe_command rg_separator）：**19 passed / 0 failed，exit 0**。distinct 构成：hostname-bin 2、external_program_flags 2、heuristic 2、always_allow_scope 1、safe_command 系列 12。
- shell（filter rg_external_programs）：**2 passed / 0 failed，exit 0**（`shell_required_access` 与冻结参数 `project_call_access` 均 All/拒绝 RX）。此前被 `reverse_request_session_id_tests.rs` 在途编辑编译错误阻塞的问题已由其 owner 修复，本次 shell green 为最终编译版本上的实际结果。
- 首轮 draft 中 "8 项全绿" 的计数有误：实际 distinct 为 workspace 6 + shell（当时受阻）；本节以上述 final 日志为准。

**Limitations**：保守扫描对字面 pattern 恰为 flag-shaped 词（如 `rg -- --pre pattern`）会 over-prompt（不误放行）；未实现完整 rg option parser，长选项缩写（`--hostname-b` 实测 exit 2 无 helper，不构成风险）未建模；受影响开发说明与 spec 由父更新（design/spec 已 strict 22 pass）。未 commit，未动邻居文件。

- [x] 父核实际diff和证据，更新开发说明并严格验证/归档。
