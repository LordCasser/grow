## Why

本 change 只补充测试与审计证据，不改变运行时行为或已归档契约。此前的共享分类器修复已归档；本次增加同一路由下历史 tool image 的回归，验证既有图片 fallback 在当前模型持续发起请求时也会启动。

只读证据来自 Loki session `01a14ef4-6810-7b22-a4e2-64574c1f37f2`：seq 359 的 `glm-5.3` Chat 请求不带图片并在 seq 362 成功；随后 `read_file` 工具结果附带一张图片，seq 376、384、403 的请求均包含一个 `image_url`，对应 seq 377、385、404 均返回精确错误 `messages.content.type 参数非法，取值范围 ['text']`。全会话 Sideband 仅有 session-title，没有 image-description Sideband，也没有 `ImageProjection`，因此视觉辅助请求从未获准入，事实不支持“视觉辅助调用失败”的解释。

当前 personal config 的 `models.image_description` 为 `deepseek/deepseek-v4-flash`。运行中的 PID 77090 使用 `~/.local/bin/grow`，其 `grow --version` 为 `2.3.1 (7b7d56a)`。用户授权的是源码修复，未授权安装；该运行中二进制仍使用旧分类器。本 change 不安装或启动程序，也不改真实 session。

## What Changes

- 在现有 `image_input_recovery_tests.rs` 增加当前模型继续使用 Chat Completions 的 tool-image 回归，并复用现有场景 helper。
- 验证精确中文 400 错误进入既有视觉辅助、durable `ImageProjection` 和无图请求重建路径，同时保留原图及完整 Goal usage。
- 记录该测试覆盖当前模型路由，不推断或改变 unknown route 首次发送原图的既有行为。

## Scope and Status

这是纯测试与审计记录，不改变产品行为、接口、持久化或安全契约，因此设置 `skip_specs: true`，不添加 delta spec。测试及验证结果尚待主实施路径完成后记录；tasks 保持未勾选。
