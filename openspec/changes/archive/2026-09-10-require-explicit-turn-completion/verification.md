# Verification

## Scope and evidence

- 基线 `24e930fa` 已含 portable 工具往返修复；本次不是重新修改 provider parser 或捕获的 HTTP evidence。
- 原 session 的三次原始 Messages `end_turn` 证据见 [来源审计](../2026-09-10-audit-provider-stop-provenance/investigation.md)。本次将其截图中的完整行动预告作为真实 Turn 回归输入，并增加英文句号版本，判定不依赖冒号或语言。
- 新机制在普通 Turn 的采样工具目录中提供 FinishTurn。只接受独立、严格有效、有非空理由且同响应含可见回答的声明；调用结果持久化确认后才返回完成候选。Stop gate 保持最后的扩展检查。
- `completion_kind` 保存显式完成或等待；原始 provider stop reason 不被改写，也不从宿主标签倒推 provider 行为。

## Regression scenarios

| 场景 | 断言 |
| --- | --- |
| Messages / Chat Completions / Responses，中文原始预告或英文句号 | 一次无工具合法响应结束后继续同一 Turn；接着执行一个真实 todo 工具；最后取得显式完成；总计三次请求、一个 Turn terminal、业务工具执行一次。 |
| 独立完成和两种等待声明 | 写匹配结果，终态种类区分 explicit_completion、waiting_for_user、waiting_for_background。 |
| 混合工具、重复声明、缺参数、空回答 | 拒绝完成，关闭其调用/结果配对；业务工具按原派发链执行；下一响应可以纠正。 |
| 连续三次无声明 | 明确协议错误，只有三次请求，不产生正常完成。专用 Agent 的旧自动恢复不能再打开该失败。 |
| 末尾响应期间接纳插话 | 下一请求包含新输入，必须有新的声明，不能沿用上一响应。 |
| Goal 预算耗尽 | 无工具预告接纳后只发生原一次请求，后续协议恢复不能超预算采样。 |
| 用户取消 | 真实 cancel_running_task 撤销前台所有权，释放原响应后无第二次采样。直接调用夹具使用前台 stub，按既有取消测试契约返回 terminal ownership 错误；不据此声称覆盖实际 ACP 进程的完整取消链。 |
| 拒绝与结构化输出 | 拒绝不触发协议恢复；专用 Agent 的旧完成恢复也不再覆盖拒绝。三协议的 schema 请求不暴露 FinishTurn，沿既有结构化契约一次完成。 |
| 持久化确认/磁盘失败 | ChatState 的控制结果写入在 ack 前不返回成功；StorageFull 报错；成功结果计入 Surface token 压力。 |

## Executed checks

- `RUST_MIN_STACK=16777216`，Cargo 编译并发 `-j 2`，测试线程 4；复用当前 target，没有创建 worktree 或构建 release/CLI。
- ChatState 全量：475 passed，1 ignored。记录 `/tmp/grow-explicit-completion-chat-state.log`。
- Shell 全量第一轮最终验证：3797 passed，3 ignored。记录 `/tmp/grow-explicit-completion-shell-final.log`。随后对专用 Agent 拒绝恢复边界加了最终补充，最终复验结果在下方补记。
- 旧 fixture 的普通 final 显式加上声明，Sideband summary、拒绝、截断和故意缺失声明的原始响应保持原样。没有用全局 mock 自动补齐完成标记掩盖失败场景。
- 第一次新增插话测试因 mock 请求类别不匹配挂起；修正为有两个可见工具的 foreground fixture，并增加 5 秒等待上限。第一次 Goal 预算夹具缺少 root accounting mailbox，已接入与现有 compaction/image 回归相同的用量结算 owner。

## Limits

回归使用真实 sampler/Turn/工具派发/Timeline 和本机 HTTP SSE mock，未向用户的在线模型端点发请求，未替换已安装 Grow。证明的是响应终止不能隐式完成 Turn，不是对模型的业务完成声明做形式化真实性验证。单条声明出错仍可提前结束；其证据现在明确可追溯。

Responses 的阶段跨 portable 投影仍单独登记在 backlog。没有因此引入分类 Sideband、后台续跑或标点启发式。

## Final checks

- 最终源码的 Shell 全量：3798 passed，3 ignored；`/tmp/grow-explicit-completion-shell-verified.log`，91.21 秒。此前最终定向 8 组集成回归全部通过，`/tmp/grow-explicit-completion-focused3.log`。完整 Shell 与 ChatState 合计 4273 passed，4 项既有 ignored；定向测试已包含在全量内，不重复计数。
- `git diff --check` 通过。归档前 `openspec validate --all --strict --no-interactive`：19 passed，0 failed。
- 已归档并合入 `model-sampling` 主规范。归档后 strict 全量：18 passed，0 failed；archive 全量：313 passed，0 failed，记录分别为 `/tmp/grow-explicit-completion-openspec-post.log` 与 `/tmp/grow-explicit-completion-openspec-archive.log`。
- 本次构建最低约剩 2.1 GiB。已删除本次创建的 Shell 增量目录 `target/debug/incremental/shell-13603hiyxizn7` 及 640 个 Shell 链接临时对象（逻辑大小 3215 MiB，部分与增量目录共享存储，不相加当作物理回收量）。测试可执行文件保留，清理后用它完成最终全量复验，磁盘恢复至约 6.9 GiB。
- 未删除用户会话、其他项目、工具凭据或源文件，未创建 Git 提交、安装程序或发布版本。
