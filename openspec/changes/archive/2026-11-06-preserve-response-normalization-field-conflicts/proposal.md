# Preserve Responses field conflicts across usage detail completion

## Why

`crates/codegen/sampler/src/client.rs` 的 Responses 解码在 typed decode 失败后按优先级运行 `normalize_response_usage_details`：它把整个完成事件读成 `serde_json::Value`、给缺失的 `input_tokens_details`/`output_tokens_details` 补零、再重新序列化整事件。`Value` 只保留同名 key 的末值，因此补 usage 明细这一例外会同时擦掉 SDK 本应拒收的重复已知字段（`response.id`、function `name`/`call_id`、`status`、顶层 `type`、`sequence_number`、null-first `id` 等）。

已在隔离真实链路复现（loopback HTTP/SSE → `SamplingClient::conversation_collect` → Responses L2）：同一 terminal snapshot 在 usage 明细完整时被 Serialization 拒收，仅省略明细时却返回含 `test_tool` 工具调用的 Completed 候选。证据与成对输入见 `openspec/changes/archive/2026-11-06-analyze-sampling-decode-residuals/analysis.md` 与 `/tmp/grow-similar-usage-Ep8Z1S/`（六条原始 SSE fixture、probes.patch、双份日志）。尚无真实 provider 发送此类形状或工具被执行的证据；本项按错误拒收一致性缺口修复。

用户已授权处理 backlog 非延期项中的这一独立行（backlog「采样解码后续项」第 1 项），只做本项。

## What Changes

- `normalize_response_usage_details` 不再重编码整事件 `Value`。适用性判定沿用现有 `Value` 语义（terminal 类型、usage 形状、明细缺失/非对象拒修），实际补零改为沿原始 JSON 文本定位 `response.usage`（及两个 details 容器）的字节区间，把缺失 key 以原始文本形式局部拼接进 decode 副本。
- 补全副本除插入的 usage 明细外逐字节等于原始输入：任何位置的重复已知字段保留原字节并继续由锁定 SDK 拒收；unknown 扩展键（含重复）维持 SDK 既有容许习惯，不由本 helper 改变接纳。
- raw evidence、cache availability（未报告 vs 明确 0）、候选身份/终态/args 校验、既有 arguments/annotations/sparse-failed 例外及恢复/结算归属全部不变。

## Impact

- 代码：`crates/codegen/sampler/src/client.rs`（含其现有 tests 模块新增回归）与 `crates/codegen/sampler/Cargo.toml`（父授权写范围扩展：仅在既有 `serde_json` 依赖上启用 `raw_value` feature，无新依赖/版本，lock 不变）。
- 契约：`specs/model-sampling/spec.md` 新增一条 Responses terminal usage 补全边界 Requirement（含 WHEN/THEN 场景）。
- 无新依赖、无新公共实体、不改 SDK/fork、不改 stream 错误分类或 `sampling-types/error.rs`（另任务）。不补 summary/status/timestamps，不加新 capability。
