## Why

grow 已保存实际发送的 provider 请求证据，但现有 prefix 测试主要比较 `input`，不足以解释 tools、推理设置、图片投影与 epoch 的变化。需要先从既有证据定位本地变化，避免新增热路径全量序列化，也避免把 JSON 公共前缀误当远端 token cache 命中。

## What Changes

- 增加面向开发者的离线诊断脚本，读取完整会话快照内的 Timeline observations 和 `artifacts/sampling`，输出请求对比及可得 usage。
- 分别报告实际 tools 顺序/内容、system、历史 items、相关设置、cache hints、图片预算选择和 continuation epoch 的变化。
- 使用显式 attempt 身份和因果关系选择对比对象；缺少 route、谱系或 usage 证据时输出未知。
- 默认报告摘要、位置与 digest，使用有界读取和合成 fixtures 验证；开发者用法写入 `docs/development.md`。

## Capabilities

无产品契约变更。`.openspec.yaml` 设置 `skip_specs: true`：本项只增加离线开发工具，不新增 Grow 命令、RPC、UI、provider 调用、运行时状态或采样日志屏障，不创建 delta。

## Impact

预计新增 `scripts/analyze_prompt_cache.py` 及相邻测试文件。读取已有 `shell/src/session/sampling_evidence.rs` 格式，不修改其生产路径；不引入数据库或新缓存管理器。

可独立完成请求差异分析；[缓存计量](../preserve-cache-usage-availability/proposal.md) 完成后再验证其结构化用量投影。为 [provider 验证](../evaluate-provider-cache-affinity/proposal.md) 提供实验归因工具。
