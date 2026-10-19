# 验证记录

## 原因与已有恢复架构

本机 Loki session `01a14a92-2758-7ac3-8c08-ceb224b5490d` 的保存证据只读核对结果：

- seq 15415 的切换前 Responses 请求使用 `deepseek-v4-flash`，含 19 个 `input_image`；seq 15416 记录 HTTP 200 完成。
- seq 15471 的切换后 Chat 请求使用 `glm-5.3`，目标为 `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions`，1710 个 messages 含 19 个历史 tool-role `image_url`。
- seq 15472 引用 96-byte 原始响应 artifact `17a73a61c6545337f660dfe4ade3cafedd8d8f0f1df7f808eb2afbdba13149df`：`{"error":{"code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}}`。
- seq 15474 将其当成普通 fatal，未触发图片恢复。会话的请求图片预算没有淘汰图片。

已有 fallback 无须重建：`sampling-types::is_unconditional_image_input_unsupported` 同时决定 sampler 对 validation rejection 的 exact-zero usage，以及 Shell 是否记录 text-only 能力并进入视觉辅助 → OCR → durable ImageProjection → 请求重建。原 matcher 只识别英文声明或带 image 字段的类型拒绝，缺少上述中文终态列表，所以两个消费者都未识别它。

从 Git HEAD 提取未修改的共享函数，在临时 std-only Rust test 中输入同一原始错误和 image_count=19，验证返回 false。追加回归期间共享工作区出现了与本 change 设计一致的窄匹配实现；保留并验证，没有回滚并发代码。该实现只增加单/双引号两种精确终态 claim，沿用原有状态、图片数量及排除门槛。其他并发文件不属于本 change。

## 测试与审查

新增 shared matcher 矩阵覆盖单双引号、末尾标点/空白、无图及非 400；负例覆盖含 image 的允许值列表、空列表、其他字段、限定 tool-only 的后缀、普通 schema/type 错误以及精确 claim 后的 invalid-image/policy 错误。没有增加模型名或 endpoint 特例。

新增 `switched_chat_text_only_type_rejection_converts_historical_tool_images` 通过真实 SessionActor/Sampler 和 loopback HTTP 验证：先保存历史 tool image，再切换 Chat 路由；实际中文 400 后，已有视觉辅助收到原图；持久 Timeline 出现 ImageProjection，重放保留原图和描述；第二个 Chat 请求无图片且含描述；模型能力标记正确，活动 Goal 不出现 incomplete usage。单项已通过。

既有图片恢复 suite 用于补充检查辅助不可用、OCR/删除、混合结果及投影提交失败不能重试的路径。新增 case 本身覆盖视觉辅助成功，不单独重复构造 OCR 引擎失败；durable ACK gate 的失败覆盖复用已有测试。

Luna 子 agent 对 matcher 与新增 Shell case 进行了只读对抗审查，未发现列举边界的误匹配；指出新 case 不单独控制 ACK 延迟或经过 OCR 分支，以上述既有路径回归补充。审查期间没有由该 agent 执行 Cargo 或修改代码。

## 执行结果

- `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked --offline -p sampling-types -p sampler --lib -- --test-threads=4`：sampler 255、sampling-types 304，全部通过。
- `cargo test --locked --offline -p shell --lib switched_chat_text_only_type_rejection -- --nocapture`：新增端到端 case 通过，使用编译成功的 `shell-e13c2ed4401144f5` 测试二进制。
- 在同一已成功构建、包含本次修复的测试二进制上直接运行 `image_input --test-threads=4`：15 passed；`read_file_image_description_tests --test-threads=4`：6 passed。合计 580 个相关测试通过。
- 后续通过 Cargo 重跑这两个 Shell filter 时，共享工作区中独立 Plan 查询改动使当前 Shell 编译失败：`extensions/session_admin.rs:93` 引用了不存在的 `crate::config::sessions_cwd_dir`，并对 String 调用了 `to_string_lossy`（E0425/E0599）。没有修改该独立实现，也没有将重编译失败冒充通过。上述 21 个回归使用并发修改前已构建的同一个包含本修复的二进制；不声称当前整个工作区可重新构建。
- `cargo fmt -p sampling-types -p sampler -- --check`、`git diff --check` 通过。包含 Shell 的全 crate 格式检查遇到独立 `extensions/session_admin.rs` 的格式差异，未格式化他人的并发文件。
- `openspec validate recognize-text-only-content-type-rejection --strict --no-interactive` 通过；归档前全量严格校验 21 passed / 0 failed。
- 编译报告已有 `workspace` 与 `shell::image_normalize` 的 `fetch_update` deprecated warning；未混入本修复。

## 范围

只修改、验证源码。未向真实 provider 重发历史请求，未执行会话里的工具，未改写 session 文件，未构建替换安装二进制，也未重启运行中的会话。保留先前 nullable-usage 修复和其他并发改动。

## 归档与收尾

已归档并合入主规范。归档后 `openspec validate --all --strict --no-interactive` 为 20 passed / 0 failed；`openspec validate --archived --no-interactive` 为 578 passed / 0 failed。安装文件仍为 2026-10-06 的原 97,378,912-byte Grow，未替换。清理本次临时 baseline 源码/二进制，并按 package 清理 Shell、Sampler 与 sampling-types 的构建残留；保留共享依赖缓存供其他并发工作复用。
