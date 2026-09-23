# Change: Prune non-actionable Pager inventory

## Why

`openspec/backlog.md` 的 Pager 段落混入了大量只依据文件规模、职责混合或测试组织方式提出的重构建议。这些条目没有可复现行为、资源边界、身份/授权不变量或明确覆盖缺口，无法作为下一项修复的验收入口，也会遮蔽少数仍值得跟踪的具体问题。

## What Changes

- 删除 2026-09-07 Pager 静态 inventory 中只有“待拆分/补测试/职责混合”描述的段落和条目。
- 将仍有具体触发条件的身份、资源、输入、授权、渲染边界和用户可见覆盖缺口压缩为可验证的 backlog 条目。
- 保留 Markdown、bracketed paste、usage 和 Hook 等非结构性债务；不把已有 archive 当作 Pager 结构债务已解决的证据。

## Impact

仅修改 `openspec/backlog.md` 与本 docs-only change 的审计记录，不修改主规范、源码、测试或其他 change。由于没有行为契约变化，设置 `skip_specs: true`。
