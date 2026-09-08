## Design direction
先贯通显式配置路径到CLI/TUI请求与冻结计划，再接入有界配置候选查询和统一选择。来源有明确、唯一且安全目标时可生成计划；显式目标用于-f、自定义布局及模糊列表。BYOBU_CONFIG_DIR只能证明请求提供的Byobu目标，不能假装为server完整加载日志。

## Source constraints
config_files不是成功加载日志；含逗号字符串不能可靠拆分，不能以任意第一项/最后一项为修复目标。查询失败、空或歧义需要清晰说明并提供显式目标方式。所有诊断显示和重载命令使用同一已选目标及现有转义；纯默认提示须明确其只是候选。

## Implementation checks
实现前继续核对doctor命令解析与确认流，选择最小参数形态，避免另建配置管理框架。测试默认、显式绝对路径、Byobu自定义目录、逗号、多个来源、不可用探测和预览/写入一致性。用fake runner验证探测边界，实际tmux可用时再做隔离server验证。

## Entry points checked
CLI为doctor_cmd::FixArgs与run_fix，TUI为slash::DoctorRequest及slash/commands/doctor的split_whitespace解析，后台effects调用select_fix_plan。显式含空格路径需要替换TUI的简单分词，复用仓库既有shell词法工具，不能仅拼接未转义字符串。

## Explicit input
CLI和TUI统一使用`fix ID --config /absolute/file`；TUI采用现有shlex处理引号，不执行shell。--config仅适用于tmux修复，拒绝相对路径、控制字符及路径遍历。路径进入FixRequest后冻结；SSH不得静默忽略该参数。

## Discovery and presentation follow-through
Automatic planning now rejects missing/empty/ambiguous/unsafe config_files evidence instead of selecting HOME. Transaction/scanner fixtures explicitly select their isolated file; dedicated discovery tests exercise absent evidence and real custom-path apply separately. Byobu retains its effective-directory requirement unless an explicit path is provided.

The fix-list currently filters any planning error out. Target uncertainty must instead remain discoverable with an explicit --config instruction; do not call an otherwise applicable tmux fix unavailable merely because its path requires user input. Doctor guidance also still comes from the legacy static TerminalContext path. Finish both presentation call chains using the same selector before archiving this change.

## Doctor report integration
CLI configured_report_for_terminal and TUI DoctorCommand::report_for_terminal now apply the shared selection policy after collection/runtime merging. The three registered tmux fixes receive the selected path and shell-quoted reload instruction. If selection fails, their unknown file remediation becomes explicit-target guidance plus the configuration line; it must not render that line as a shell one-off command. Existing notification suffixes are retained. Startup banners and truecolor/manual-only guidance still use the legacy default candidate path and need a separate presentation pass within this change before the full guidance contract can be marked complete.

## Final state
All implementation-stage follow-through above is complete. Registered Doctor fix guidance shares the planner selector; manual-only Doctor guidance adopts known targets with command quoting. Startup/default-only hints explicitly describe their paths as unverified candidates and require confirmation. No automatic writes fall back to these candidates. See verification.md final audit for scope and remaining platform-test limitations.
