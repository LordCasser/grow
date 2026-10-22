# Reuse canonical projections within one resume

## Why
R6 的 writer load typed planning 与随后 raw replay 为同一已校验 Timeline 重复构造 canonical projection，现代2048轮中合计约5.5秒；raw planner 即使发现 exact row 也在 candidate anchor 处重新编码同一正文。

## What Changes
一次 writer load 保留已生成的 canonical 结果供同次 replay 使用；复用仅限返回的已校验快照，不跨恢复或 resident delta。物理 projection 仍逐一校验 exact payload，已验证的物理行可借用到 anchor 处以避免重编码。保持缺失投影补写、重复抑制、rewind、cursor、resident frontier 和完成屏障。纯内部算法优化，skip_specs，不修改外部契约或持久格式。

## Out of scope
不增加持久缓存，不跳过 digest/conflict 检查，不改变通知执行或 TUI 播放节奏，不扩展到通用 JSON/forward 重构。
