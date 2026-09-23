# Verification: prune-pager-structural-inventory

## Scope

本 change 只处理 `openspec/backlog.md` 的 Pager 静态 inventory；不修改源码、测试、主规范或其他 active/archive change。判断基于 2026-09-23 当前工作树，工作树已有并行 dirty/untracked 改动，未将它们归因于本 change。

## Disposition

- 删除原第 327–445 行的 30 个 `## 待拆分：Pager ...` 段落（含空行）：它们只记录文件同时承担多个职责、静态契约数量或未来拆分/动态验证计划，没有独立故障、资源边界或安全证据。
- 删除原第 447–680 行中只谈模块大小、职责边界、fixture/builders、等待时长、断言写法、测试串行化或“缺少同文件测试”的结构/测试组织条目；这些不再作为独立 backlog 债务。
- 原第 327–681 行共 355 行，替换为 32 行，净删除 323 行；原段含 30 个待拆分标题和 234 个 Pager/Markdown/bracketed-paste 条目，现按验证入口合并为具体问题与覆盖缺口。
- 保留并压缩有明确触发条件的 process-global 隔离、worker/队列与缓存生命周期、持久化 revision、身份/坐标/宽度、授权 epoch、输入与协调行归属、诊断失败路径，以及默认覆盖缺口。保留 Markdown syntect/property gap 和 bracketed-paste 边界；保留原有跨 resume usage 与 Hook 快照规模/顺序条目。
- 本次不以 `changes/archive/` 中现有 Pager 修复的存在推断静态结构债务已解决；现有 archive 只覆盖各自 change 的行为范围。
- 对 `openspec/changes` 与 `openspec/changes/archive` 的名称/内容检索没有找到被删除静态标题的专属 change 或验证记录；例如已有 Pager archive 只覆盖 pager lifecycle、feedback、replay/restore 等具体行为，因此不能反向证明文件拆分建议已经完成。

## Checks

- Backlog 保留段落边界：nono 条目、跨 resume usage、Hook 条目和 bracketed-paste 条目仍可定位。
- 新 Pager 区域与本 change 的相对链接检查：pass；本次新增内容没有引入失效链接。整份 backlog 仍有一条既有的 `changes/improve-agent-communication-presentation/design.md` 链接未解析到当前目录（对应 archive 已存在），属于本 change 之外的历史 dirty-tree 问题，未在本次混入修复。
- `git diff --check`: pass。
- `openspec validate --all --strict --no-interactive`: pass（归档前 20 项，归档后 19 项；当前共享工作树未因本 change 产生失败）。root 同步删除的 `openspec/changes/verify-openspec-baseline` 未恢复。
- 未运行 Cargo、测试或构建。
# Post-archive link correction

The shared backlog contained an unrelated stale active-change link to `improve-agent-communication-presentation`. After confirming the archived design exists, the link now points to `changes/archive/2026-09-14-improve-agent-communication-presentation/design.md`. No issue text or product behavior changed.
