# Verification

## Scope

- 日期：2026-10-18。
- 静态基线：`0a0dd07cd3aa35d39365ae971df8f9b316185c9d`。
- 用户最终范围：仅建立 OpenSpec change；具体代码由用户后续统一实施。
- 本次只修改本 change 目录。未修改产品源码、主规范或 docs，未运行 Cargo 构建与运行测试，未归档。

## Static review

- 已核对退出/CancelTurn、shutdown 队列拒绝、sampler 结算、child coordinator 内存 owner、`resume_from` 派生路径及 cold orphan reconciliation。
- 已保留真正取消与挂起的区别：命令可以 Interrupted，child 任务不因此 cancelled；已有真实 terminal 不被暂停覆盖。
- 已显式调整现有 Ctrl+C/teardown、未闭合 Turn 与孤立 child 恢复契约，其他改动通过对应 capability 的 ADDED Requirements 表达。
- 已将 partial suspension、unknown ACK、双重 resume、恢复接纳后再崩溃、旧 writer 存活、权限漂移、外部操作未知与迟到交付列入验收矩阵。
- 对抗性审查发现并修正两项冲突：通用 Turn recovery 原先无条件写 interrupted，现为有效挂起保留原 Turn；未接纳 response 原先宽泛允许重采样，现保留 typed 安全条件、provider 托管工具副作用限制、原 attempt 额度和绝对期限。修正后定向复核通过。
- 自查补充恢复资格的单次消费：恢复已开始后无新挂起又异常退出，不能使用旧 checkpoint 回滚或获得永久自动重启资格。
- 全部 5 个 MODIFIED requirement 均匹配主规范原名称并保留原场景标题；扩展场景明确挂起的例外，未通过删除失败场景掩盖契约差异。

## Commands

| 检查 | 结果 |
| --- | --- |
| `git pull --ff-only` | Already up to date；本次静态基线与远程一致 |
| `openspec validate suspend-session-tree --strict --no-interactive` | 通过 |
| `openspec validate --all --strict --no-interactive` | 初次 18/18 通过；最终复查 18 通过、1 失败，新增范围外 change 见下文 |
| `openspec status --change suspend-session-tree` | proposal、specs、design、tasks 共 4/4 planning artifacts complete；不是实施完成 |
| 文档完整性脚本 | 5 个 MODIFIED requirement 均保留主规范所有原场景标题；13 个文件无行尾空白 |
| `git diff --check` | 通过；未跟踪的新文件另由上述脚本检查 |
| `git diff --name-only`、`git diff --cached --name-only` | 均为空，无已跟踪文件或暂存改动 |

最终复查时，工作区出现本次未创建或修改的 `openspec/changes/unify-skill-loading/`。`openspec validate unify-skill-loading --type change --strict --no-interactive` 报告 `Change must have at least one delta. No deltas found.`。该 change 导致全量校验退出码为 1；本 change 校验通过。保留该目录原样，不为消除范围外失败修改它。

## Runtime verification

`acceptance.md` 的 S-01～S-30 全部未执行。没有声称 Ctrl+C 挂起或自动子树恢复已经可用。
