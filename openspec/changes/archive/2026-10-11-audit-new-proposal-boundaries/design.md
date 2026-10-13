## Scope

范围限定为 `add-mcp-multi-round-elicitation`、`retain-plan-for-later-review` 与 `record-upstream-review-2bdd1d6`。以当前 `openspec/specs/` 为已归档契约，以当前工作树为实现证据；不把未提交实现当成固定 HEAD 或发布事实。

## Method

从真实入口核对 MCP dispatch/recovery、pending registry、Plan control/approval/handoff、Pager preview 和持久恢复。Atlas 只作为局部定位辅助；当前文件内容与索引不一致时以直接源码为准，不把空搜索当作缺失证据。

审计补充限定为这两项能力的直接边界，不借机重构通用交互系统、权限引擎或调度系统。仅作澄清的内容补入现有 requirement；独立的新不变量才增加 requirement，并对应验收行。

## Validation

检查新增场景、任务和验收映射及相对链接，执行严格全量 OpenSpec、diff 检查，再仅归档本审计 change 并验证 archive。保留修改前文件 hash，用于确认未触及产品代码、主规范和其他在制文件。不运行 Rust 构建或产品测试，不把静态推演称为运行验证。
