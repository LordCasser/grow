# 验证记录

## 范围

复用现有 resolver、Task coordinator、AgentRebuildSpec、actor Step 控制、Timeline checkpoint 和 permission manager；没有另建授权层。工作区同时包含其他任务及前一轮修复，不提交或覆盖其修改。

## 已执行

以下定向测试共 159 项通过；最终移除临时诊断后再次编译并运行完整跨进程 E2E，通过（最终复测 4.33 秒，不含编译）。

- Shell 身份回归 3 项：native 层级 ID/路径及大小写别名、plugin bare/qualified toggle/allow/deny、同名 native 优先、禁用 plugin 不露出 CLI bare fallback、Workflow capture 同样按正式 ID 授权、出生事实解析失败。
- toggle/reload 回归 8 项，包括新增 primary/child × CLI/plugin × disable/enable/delete 的真实 actor rebuild。
- validation 回归 9 项；Bash permission 集成 12 项；delegated capability 9 项；one-shot permit 4 项；AgentRebuildSpec 4 项。
- 跨进程挂起恢复端到端 1 项（4 个独立进程）：已通过 suspend → missing-birth 拒绝 → invalid-birth 拒绝 → restore + 实际后代创建 + 再次挂起。
- Agent/native/plugin discovery 回归 54 项通过。
- resolver/resume/context 相关回归 55 项通过；Core Regression 已包含 shell/agent lib 全测，无需增加旁路 CI 入口。

## 端到端断言

第一进程通过实际 coordinator 创建省略 capability override、默认 ReadOnly 的 child，真实工具审批后运行一次命令，在 Step 边界切换到默认 All profile，再于下一调用审批等待处挂起整树，确认原 actor 退出。后续独立进程从 JSONL 恢复同一 root/child：出生事实缺失/非法须阻止全树，正确事实须恢复原未派发 call ID 并重新审批；已完成命令不得重放；再实际派生请求 All 的后代，验证其有效上限仍为 ReadOnly。

HTTP provider 与人工审批响应为测试替身；session actor、coordinator、权限 manager、Bash 工具、文件写入、Timeline 存储、writer retirement、load/restore/release 均为生产链路。本轮不声称验证真实外部模型、真实终端 UI 或 MCP 服务端重连。

## 对前一轮 R2 的归因补充

原复审只用 resolver 探针验证重算 ReadOnly→All，未执行全恢复。进一步检查发现 source hash 还绑定有效定义与能力源，部分漂移会被这道屏障阻止，不能把原探针直接表述为已证实的端到端权限提升。此次修复目标是保留出生事实并正确恢复，不放松 source hash。

## 独立发现与 fixture 修正

首次端到端使用 macOS 临时目录别名 `/var/...`，挂起成功，恢复被 source hash 拒绝。另以 `/tmp/grow-authority-debug...` 重现，临时逐字段比较 source JSON 只发现 `/workspace` 从 `/tmp/.../workspace` 变为 `/private/tmp/.../workspace`。此问题属于路径身份入口不一致，已登记 backlog，未放松 hash 校验；权限回归改用初始即 canonical 的工作目录。临时 source dump 诊断已删除，手工重现临时目录已清理，不作为生产功能保留。

另核对 Spawn 无出生 MCP 成员集合快照、运行期 ceiling 持有 client identity；其跨进程成员约束需专项验证，已独立登记。此次 E2E 的 MCP pool 为空，不将 live MCP 测试外推为跨进程覆盖。

## 工具与规范检查

使用仓库固定 Rust 1.93.1，`CARGO_INCREMENTAL=0`、dev/test debug=0，复用共享 target。执行 shell lib 定向测试和 `cargo test -p agent --lib discovery::tests`；`git diff --check` 通过。临时诊断目录已删除；没有新增增量编译缓存，也未删除其他任务已有的 target 缓存。未安装或发布新的 grow 二进制。

`openspec validate --all --strict --no-interactive`：17/17 通过。归档后验证另记于下方。

归档已完成。归档后 `openspec validate --all --strict --no-interactive` 16/16 通过，`openspec validate --archived --strict --no-interactive` 758/758 通过；tasks 全部勾选。最后复核将正式路径查找限定为逐段精确匹配目录项，避免单个目标解析递归扫描其他 Agent 子目录；54 项 discovery 回归再次通过。

逐段路径查找调整后的最终 shell 构建完成；身份 3、toggle 8、validation 9、resolver 55 和四进程 E2E 1 项再次全部通过。未修改或放松 runtime source hash 校验。
