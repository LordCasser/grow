# 验证记录

日期：2026-09-13（Asia/Shanghai）。Shell 完整库测试 3,795 通过、3 ignored。

workflow_restore_uses_timeline_ownership_and_caps_run_count 在同一隔离 fixture 中分两阶段验证：
1. 131 个有效 run：仅返回 wf_003..wf_130，精确验证数量和顺序。
2. 将最后三个变为 cleared、脚本缺失和 script hash mismatch：回补后返回 wf_000..wf_127，精确验证数量和顺序。

保留原 Workflow manifest、目录能力、symlink、脚本/args hash 和有界读取错误回归。遍历直接借用 Timeline 的逆序 Spawned，不分配全部 run ID 的副本；使用既有 O(1) lifecycle 查询，达到有效项上限停止并 reverse 返回。

审阅修正：子代理最初替换了原 all-valid cap 场景，已恢复为两阶段验证；第二阶段预期索引的错误已修正，未通过削弱断言绕过失败。最坏 I/O 随有限 Timeline 候选数增加；单文件预算、返回数量上限不变。Forgotten/checkpoint 仍是独立持久化设计。


完整构建、测试、依赖图及磁盘记录见 [同批整体验证](../2026-09-13-remove-runtime-dependencies-from-leaf-crates/verification.md)。
