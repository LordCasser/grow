# Verification

## 最终验收 — 2026-10-19

完整物理 ACP/Grow 历史的计数恢复在存储 writer load 内、任何恢复写入之前完成，不增加跨 persistence/bootstrap 的状态字段。rewind 隐藏的已占用 ID 仍参与下界，Timeline-only expectation 补足 cache 未写窗口，exact append 冲突拒绝保持不变。

`cold_writer_load_seeds_from_full_physical_update_snapshot` 与 `cold_load_reseeds_from_expectation_missing_from_cache` 在 focus122/123 各 1/1，并在最终 Shell126 通过；前者直接从 load 生成并持久提交新事件，不由测试手动 reseed。normal/minimal 的真实 headless 新一轮在 Skill surfaces123 的两个流程中成功，修复前 surfaces120 的 sampling-anchor 冲突已关闭。

最终包级回归为 Shell126 **4251 passed / 0 failed / 5 ignored**、Pager124 **7503 / 0 / 12**、Tools121 **2416 / 0 / 7**。日志分别为 `/tmp/grow-v2-4-0-shell-final126.log`、`/tmp/grow-v2-4-0-pager-final124.log`、`/tmp/grow-v2-4-0-tools-final121.log`。ChatState108 567/0/2、tool-types106 123/0、Agent110 482/0 及各 backend 的既有通过记录继续作为相应未改责任层的证据。CLI122 构建并报告 `grow 2.4.0 (8cc20454)`；后续修改仅为测试夹具，未更改生产语义。

当前真实进程验证共九项：CLI122 的 root、三层树、部分释放、并发恢复和普通 continue 五个 exact PTY 各 1/1（`/tmp/grow-v2-4-0-pty122-results.json`）；Plan approval 与 Defer 后冷恢复审阅为 2/2（`/tmp/grow-v2-4-0-plan-pty122.log`）；normal/minimal Skill 挂起、原快照恢复、导出和 headless 继续为 2/2（`/tmp/grow-v2-4-0-skill-surfaces123.log`）。

验收以生产 owner 的不变量为单位组合 Timeline、Actor、coordinator、存储故障注入、UI reducer 与真实进程证据。下方旧矩阵记录各阶段已运行和未穷举的排列，不再把缺一个巨型组合夹具当作实现阻断，也不声称每个排列都在实机执行。物理断电/满盘、所有状态交叉组合、Linux/Windows 实机和真实 MCP 多轮互操作未运行。MCP 多轮按用户决定暂缓，其门禁保持关闭。版本总体证据见 [联合验证](../2026-10-19-prepare-v2-4-0/verification.md)。

## 归档后校验 — 2026-10-19

本轮六个 change 已归档，规范合并与引用修正完成。`openspec validate --all --strict --no-interactive` 为 **16/16**，`openspec validate --archived --no-interactive` 为 **605/605**，日志 `/tmp/grow-v2-4-0-openspec-all128.log` 与 `/tmp/grow-v2-4-0-openspec-archived128.log`。39 处迁移后的 Markdown 引用均可解析；六份 tasks 无未完成项。格式与 diff 检查通过。当前唯一 active change 为用户暂缓的 `add-mcp-multi-round-elicitation`。

## 历史阶段证据

以下保留原始诊断、失败和阶段矩阵；其中的“当前 / 待运行 / 未实现”描述当时状态，由上方最终记录覆盖。
## Baseline

2026-10-19，`/tmp/grow-v2-4-0-skill-surfaces120.log` 的两个真实进程测试均通过挂起、同身份恢复和原 Skill 正文导出，但 headless continue 新一轮在 sampling anchor exact append 失败。静态诊断确认 noReplay 绕过完整更新历史的 event counter reseed。

## Implementation status

冷 writer load 现从已经解析的完整物理 updates 快照读取最大合法 eventId 后缀，并与 Timeline expectations 合并后、在 load-time projection recovery 和 actor 发布前推进计数器。该职责在存储 owner 内完成，没有新增跨层传递字段；观察型 load 不读取物理更新来恢复 writer identity。exact append 冲突规则未改。

新增 `cold_writer_load_seeds_from_full_physical_update_snapshot` 覆盖 ACP/Grow 高序号及 rewind 隐藏的物理更新；既有 `cold_load_reseeds_from_expectation_missing_from_cache` 覆盖缺少缓存行的 Timeline expectation。当前未运行 Cargo，测试结果与 noReplay 后续 prompt PTY 尚待 root 统一构建验证。
