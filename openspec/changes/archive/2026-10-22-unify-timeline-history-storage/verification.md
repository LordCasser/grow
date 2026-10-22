# C3 验证记录

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

实施、最终 correctness 与 release 配对测量已完成。实施前的基线和中间失败已在最终结果中区分；对抗性结论见[对抗审查](adversarial-review.md)。

## 实现边界

生产历史统一写入 `timeline.jsonl`；已准入 response 正文只存一次，candidate 仅保留无正文顺序 anchor，必要独立展示使用闭合 typed facts 或不可变来源引用。移除生产 updates writer、projection repair 和第二次 response commit。Session v7、physical record v1、Domain v27；不迁移旧会话。完整校验与全部历史交付后才 ready，未引入 SQLite、检查点或就绪/分页分离。

冷恢复另外修正为 child result/parent terminal → 清理旧 scope → 恢复 UI/receipt；补发 child 展示不重跑 SubagentStart Hook。来源、所有权与代价见 [source-inventory](source-inventory.md) 和 [scenario-coverage](scenario-coverage.md)。

## 构建与运行身份

Rust 1.93.1，普通 release、jobs=4、incremental=0、locked/offline，专用 `target/resume-release`。共享 checkout 有其他变更，未复位。最终生产 CLI4 SHA-256 为 `f4f432dce245cb2d585e21817a1a8a79e382e5d417839a125327173de74d34cf`；完整命令、源码及 executable hashes 见 final-build-context.json、after-test-binaries-21.json。

build21 后的两类源码变化分别留证：`final-22-test-only-change.json` 是 cfg(test) 的旧 reader 迁移，`final-rustfmt-source-map.json` 是 C3 文件纯 rustfmt。额外 test-support 诊断源 hash 见 `final-additional-source-hashes.json`，后续工具组合测试为 `final-25-tool-context.json`；当前源码与生产构建的对应证明见 `final-source-audit.json`。测试驱动显式使用实际 CLI；ignored 用例必须报告真实 `1 passed`，禁止把退出码 0、忽略或空选择当作成功。

## 最终正确性

| 范围 | 已确认结果与证据 |
| --- | --- |
| Pager 9 组 | 755 passed / 0 failed / 1 ignored；`final-21-validation-results.json`。 |
| PTY harness 单测 | 96 passed；同上。 |
| 现代完整历史 oracle | 128、2048、repair512、route512 四组逐正文/顺序/数量对照通过；开启 `GROW_PERF_ASSERT_HISTORY=1`，这些时长不作 A/B。 |
| Fork/roundtrip/load gates | Fork 1、roundtrip 2、普通 load 2、独立 peak RSS 1、e2e RSS 1 通过；`final-21-memory-results.json`。300 项 fork stream peak/journal 为 1.714116x，保留原 `<2x` 门槛。 |
| Shell scoped | Shell22 19 组 1331 passed / 0 failed / 4 ignored；`final-22-shell-results.json`。build21 的两个旧 cfg(test) reader 失败保留原日志，使用生产统一读取器后全组复验。 |
| 实际 CLI/PTY | CLI4 #01–12 与 dense #14 逐项真实通过；工具 #13 的最终组合用例 final25 为 1 passed，保留旧失败日志；`final-21-cli-results.json`、`final-25-tool-context.json`。 |

固定 wire 样本 46 行，包含 38 条持久 Grow 状态样例、7 类 ACP 与 1 条必须拒绝持久化的 transient SamplingAttempt。Grow typed variants 共 32（31 个 golden families 加独立 DiffReview）。不仅比较条数，还逐字段、逐物理 prefix 比较角色、来源、时间、tool ID、80/120 列 root/child 渲染；多 part cursor、半响应回退、双 fork、rewind、图片恢复 EOF/终态和晚到更新另有专项断言。

失败路径覆盖 checksum、完整行/断尾、schema、exact identity/payload、缺 artifact、fsync/lost ACK、preview credits、resident barrier/cancel、fork/import 执行隔离。直接覆盖与组合证据的界限逐场景写入 coverage；没有声称穷举所有类型两两交错或 producer side effect。

## Release 配对基准

最终配对运行已完成，详见[配对报告](paired-analysis.md)。recovery reorder 前的中间候选不计入最终表；测量与编译/正确性测试串行，3 对 AB/BA、冷进程/温热页缓存、固定合成业务内容，fixture 构造不进入 load RPC。`first_history` 是 ACP 收到首条历史；PTY `history_visible` 是末尾 sentinel 可见，两者不可混用。

| 2048 轮现代样本 | Before | After |
| --- | ---: | ---: |
| load RPC median | 747.1 ms | 416.7 ms（−44.2%） |
| 首条 ACP 历史 median | 438.3 ms | 232.3 ms（−47.0%） |
| 全目录 | 148,178,363 B | 74,849,404 B（−49.5%） |
| 32 KiB response 提交 median | 26.9 ms | 14.1 ms |
| ledger barriers / response | 6 | 4 |

三个进程中位数/range、PTY 与 RSS 详见配对报告。TUI 末屏没有一致加速：512 轮末条可见为 428.5 → 446.2 ms；单对慢 writer 首/末条及最大间隔也回退，但三次可见推进、输入屏障及 Ctrl+D 正确。不能以 RPC 改善宣称所有 TUI 延迟改善。常规 resume 峰值内存没有确证下降。当前主残余成本是统一 journal 全量解码/校验（代表 run 201.7 ms），没有偷换为延迟校验或提前 ready。

## 对抗性结论与限制

恢复仍是 O(历史事件与正文)，新 checksum/typed metadata 有成本，artifact 发布与单 journal 不是跨文件事务。repair 样本在新格式已无第二投影可修复，应披露结构性差异。整进程 RSS 与粗采样方向可能不同，不能承诺内存下降。

已单列既有 `CompletedOutputUnavailable` 在 child 存在但缺 canonical result 时可能被固化为成功的债务，见 [backlog](../../../backlog.md)；未混入本 change。现有确切 result link 验证未删除。root 通用工具的生命周期/展示正文/模型结果及 cold ACP 不重派发已有 final25 直接证据；admission 后客户端交付失败及其他工具/子树的所有组合仍须按 coverage 的实际证据说明，不能用相邻用例替代。

## 规范与清理

归档前全量 strict 为 18 passed / 0 failed（`final-strict-pre-archive.txt`）。C3 scoped rustfmt 和 git diff 检查通过；全局 fmt 的三处余项均与 HEAD 相同，未混入本项修复（`final-format-scope.json`）。

所有测量及测试结束并核对二进制 SHA 后，清理专用 `target/resume-release` 11.077 GiB，包括保留的 before executables；共享 target 与真实会话保留。见 `final-build-cleanup.json` 与 `final-retained-binary-identities.json`。本 change 已归档为 `2026-10-22-unify-timeline-history-storage`，两份 delta 已合入主规范；配套分析归档为 `2026-10-22-profile-release-resume-first-history`。归档后全量 strict 为 16/16，归档 strict 为 685/685；结果见 `final-strict-post-archive.txt` 和 `final-archived-strict.txt`。
