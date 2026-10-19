# v2.4.1 发布前复核 — 2026-10-19

## 当前结论

四项退出/图片回归工作已经完成实现验证，并在本轮归档、提交及合入本地 main；具体提交与保护 MCP 工作的结果在本文件末尾记录。本轮没有更改版本、推送、创建 tag、安装或发布。当前可继续准备 v2.4.1，但尚未达到可发布状态。

前一轮 [候选范围及发布边界](../2026-10-19-prepare-v2-4-1-analysis/release-analysis.md) 仍作为历史基线，不能当作包含本轮四项的最终说明。其已记录的 Windows/Linux、release-dist 和资产验收限制仍成立；本轮没有对远程发布状态重新作出结论。

## 新增候选范围

| Change | 最终行为与相互作用 | 证据 |
| --- | --- | --- |
| audit-image-read-parameter-failures | 仅加 ReadFileTool 的真实 PNG 回归；pages/format 仍只用于 PDF，误参仍明确失败。原会话两次失败均为此类误参，正确读取和后续视觉辅助成功。 | [验证](../2026-10-19-audit-image-read-parameter-failures/verification.md) |
| reject-auxiliary-commands-during-suspension | 有效挂起接纳后关闭新辅助活动，迟到补全返回无建议，避免 actor 断言崩溃；已有 owner、用量结算和 checkpoint/退役邮箱保持活跃。过期新请求及物理排空后已耗尽的 deadline 在新 checkpoint 派发前结束，不放宽已发出写入的严格确认规则。 | [验证](../2026-10-19-reject-auxiliary-commands-during-suspension/verification.md) |
| recover-unconfirmed-quit-input | Pending 仍合并重复 Quit；Unconfirmed 下重新双击 Ctrl+D 走重试，r/f 保持显式动作。正常页面通过统一内容绘制后的出口画退出状态，minimal 使用同一 painter；长错误不能挤掉控制行。Ctrl+C 不变，迟到旧响应不能结算新请求。 | [验证](verification.md) |
| streamline-suspension-runtime-observation | root 身份和退役节点状态查询去掉随后丢弃的历史复制，补节点/整树阶段耗时；保留 child 事实和冷历史验证，不增加缓存或并行退役协议。 | [测量及限制](../2026-10-19-streamline-suspension-runtime-observation/verification.md) |

这四项叠加在前一轮八项加固及 main 的两项 Plan 改动之上。独立 MCP 分支上的 SDK/策略/身份改动不属于本候选；MCP 多轮交互仍沿原有门禁暂缓。共享命令循环中的有限工作准入保护不构成 MCP 功能升级。

## 测试结论与已知边界

实现阶段使用 pinned Rust/Cargo 1.93.1、macOS arm64。Shell/Tools 完整所选库分别通过 4,274 / 2,423 项；Pager 完整库通过 7,530 项；minimal 通过 95 项。随后最终过期准入/真实 mailbox/排空重试测试 3 项及输入/实际页面绘制测试 4 项通过。实际 AppView::draw 回归在修复前能检出 Agent 分支漏画状态，修复后 Agent/Welcome/Dashboard 均可见控制行。

先前四项真实 Pager 进程验证覆盖运行命令退出恢复、root/child/grandchild 树冷恢复、normal/minimal 原 Skill 快照；最终 renderer 源码重建 CLI 后又通过树冷恢复和 minimal 原 Skill 两项。没有通过手动重置 Idle 制造恢复。完整测试名、执行顺序、红绿日志及忽略项见各 verification；这些分阶段结果不等于在每个中间源码上重复执行全部库。

8,000 条独立 4 KiB JSON 事件的本地 debug 测量中，十次状态观察从 69.833 ms 降到 0.392 ms。它仅衡量去掉的查询复制，不能当作整次退出的速度或跨平台 SLA。旧进程的一秒采样不足以确认长时间等待的具体 owner；新增阶段日志用于后续定位。长历史重复验证、工具错误遮蔽及同步 compaction TODO 再投影仍见 [backlog](../../../backlog.md)，本轮不扩展其实现。

本轮最终化仅修改文档、规范归档和 Git 集成；运行时源码与上述已验证实现保持一致。因此复用已完成的实现回归，并重新检查格式、链接、规范、版本/lock、精确 runner 与发布 DAG，不以大量重复编译代替验证范围判断。

归档后严格主规范/活动项校验通过 **16 项**，严格 archive 校验通过 **625 项**。pinned 格式检查、diff 空白检查、**16 项** Python 维护/runner 测试、**57 个**本地 package 的 locked metadata/lock 身份核对均通过。107 份 Markdown 中 583 个相对目标及 329 个锚点均存在；外链及 123 个历史绝对本地文件/诊断引用不属于这项相对链接检查。发布/build 脚本语法、六份工作流 YAML 与提取的发布静态契约检查通过。

## 发布前仍需完成

1. workspace/Cargo.lock 产品版本仍为 **2.4.0**，`crates/codegen/shell/changelogs/2.4.1.md` 尚不存在。正式准备版本时更新 workspace 继承链和对应 lock 条目，保留独立版本库；新说明应覆盖前轮八项、本轮四项及两项 Plan 改动，不能改写 2.4.0 历史说明。
2. 复核期间，另一项操作已将原先未提交的 MCP 改动隔离到 `codex/mcp-policy-sdk-identity`，main 已干净。本候选不能合入该分支或套用其 stash；正式发布应以最终版本的干净提交为源码，并绑定 annotated tag。本轮不创建 tag。
3. 官方 publish DAG 仍要求同一不可变 SHA 的 core、原生协调/导出、Windows storage、OpenSpec、平台构建及 dispatch guard 成功。失败、跳过、取消均应阻止发布。本地 macOS 回归与静态检查不能代替 hosted Windows/Linux 结果。
4. 最终 release-dist 的十个平台产物及可运行平台 smoke、macOS 签名、Windows grow.exe、单入口 tar、attestation 和 SHA256SUMS/下载重验仍须通过。musl/riscv64/OHOS 既有原生运行限制不因构建成功而消失。当前未执行托管构建、产物验收或发布。
5. `scripts/validate-release.sh` 完整入口要求干净已提交源码、annotated tag、版本与非空 notes。本轮只跑可独立执行的静态契约和 runner/DAG 测试，不伪造 tag 让发布预检变绿。

## 本轮集成结果

- 实现、回归、四项归档及主规范提交为 `174e6cac`（`fix: harden session suspension and archive exit regressions`），已从 `dc7c02d8` 快进合入本地 main；合入后工作区和索引均干净。后续集成记录提交仅修改本归档中的文档，不改变已验证源码。
- 本轮最初对 main 的 34 个 tracked 改动与 17 个 untracked 文件保存了逐文件 SHA256 快照。复核期间，另一项操作将其中 49 个 MCP 源码/文档文件提交至独立分支 `codex/mcp-policy-sdk-identity` 的 `5b375b07`。逐文件核对这 49 个文件与原快照完全相同；backlog 原措辞保留在既有 stash `29671011`，额外 Python cache 的原件也在快照中。本轮没有恢复或删除这些独立工作。
- main 不包含 `5b375b07`，MCP 专属源码、extension-runtime 主规范和两个 MCP archive 均未进入本次提交。共享 run_loop.rs 中只有当前挂起契约所需的有限活动准入修复；未带入 SDK 类型、client_source 或新工具策略实现。MCP 分支 HEAD 及既有 stash 在合入前后保持不变。
- 合入 main 后再次通过 pinned 格式、严格主规范/活动项 16 项、archive 625 项及 107 份 Markdown 的 583 个相对目标/329 个锚点检查，均无失败。记录补充没有改动运行时源码。
- 本轮仅提交并合入本地 main，未推送、打 tag、安装或发布。正式发布前仍须完成上列版本/notes、同 SHA 托管门禁和 release-dist 产物验收。
