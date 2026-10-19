# 验证记录

## Session 证据

只读核对 `01a14ef4-6810-7b22-a4e2-64574c1f37f2` 的 Timeline 和所引用 sampling artifacts：

| 请求证据 seq | 模型 / 协议 | 图片数 | 结果 |
| --- | --- | --- | --- |
| 359 | glm-5.3 / Chat | 0 | seq 362 正常完成 |
| 376 | glm-5.3 / Chat | 1 image_url | seq 377 HTTP 400 |
| 384 | glm-5.3 / Chat | 1 image_url | seq 385 HTTP 400 |
| 403 | glm-5.3 / Chat | 1 image_url | seq 404 HTTP 400 |

三次错误的原始 JSON 相同：`{"error":{"code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}}`。从无图成功到读图后失败，模型和 endpoint 保持 `glm-5.3` 与 `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions`；这不是切换模型才出现的条件。

记录只有 session-title Sideband，没有 image-description Sideband 和 ImageProjection；`sampling_usage/attempt_settled` 在首个拒绝后记录 usage=null。共享 classifier 没有将该错误识别成图片能力拒绝，图片一直保留在后续主请求中，因此用户再补充文本也会重复 400。证据支持“未进入辅助视觉流程”，不支持“视觉辅助已调用但失败”。

当前 personal config 的 `models.image_description` 指向 `deepseek/deepseek-v4-flash`；这只能证明当前配置值，不据此伪造 session 启动时的有效配置快照。PID 77090 的 executable 路径为 `/Users/lordcasser/.local/bin/grow`，当前同路径 `--version` 仍为 `grow 2.3.1 (7b7d56a)`。前一轮按用户选择只修复源码，没有安装，运行进程尚未获得共享 classifier 修复。

## 本次改动

没有新增运行时修复：归档的 `recognize-text-only-content-type-rejection` 已在所有图片请求共用的分类器中处理该错误，条件不依赖模型切换。本次只将上轮集成场景提取为测试 helper，新增 `current_chat_text_only_type_rejection_converts_new_tool_images`。current 用例先配置 Chat primary，再加入新的 `read_file` tool image；switched 用例先加入历史图片再选择 Chat route。两者检查同一现有视觉辅助、durable ImageProjection、无图重建、原图保留、Goal usage 完整性，并核对恢复前后 primary route revision 不变。

unknown route 首次发送原图的契约不变；不会仅凭配置了视觉辅助模型就预先丢弃所有原图，也不按模型名称猜测能力。

## 验证方法与工作区限制

尝试在当前工作区执行 `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib chat_text_only_type_rejection_ -- --nocapture`，被并发的非本任务测试改动阻塞：`runtime_restore_tests.rs` 使用不存在的 `StopReason::EndTurn`（E0599），`storage/jsonl/tests.rs` 访问不存在的 `PersistedData.timeline`（E0609）。没有修改这些文件来绕过问题。

使用 `git archive HEAD` 建立临时隔离快照，基线 commit 为 `8cc20454da9e62874170fb3ed1cc532591954947`。只从当前工作区复制共享 `is_unconditional_image_input_unsupported` 函数，以及两种 tool-image 回归及其 helper；其他并发产品改动均未复制。复用 Cargo target 的依赖缓存，构建源码来自冻结快照。该方法验证本次场景与既有共享修复，不声称验证当前所有并发改动。

隔离快照构建命令：`CARGO_TARGET_DIR=<shared-target> CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --manifest-path <snapshot>/Cargo.toml --locked --offline -p shell --lib chat_text_only_type_rejection_ -- --nocapture`，2 passed / 0 failed。两条 case 分别在选路由之前和之后加入图片，并均检查 fallback 前后 primary revision 不变。

在同一成功构建的快照测试二进制中运行 `image_input --test-threads=4`：16 passed；运行 `read_file_image_description_tests --test-threads=4`：6 passed。共 22 个相关回归通过，覆盖新旧场景以及既有辅助失败、OCR/删除、投影提交失败、read_file/PDF 图片保留与按路由投影路径。运行目录为快照的 shell package；未将独立并发代码的编译失败冒充通过。

`git diff --check` 通过。包括 Shell 的全 crate 格式检查因其他并发文件存在格式差异而未通过，未格式化其他任务的文件。归档前 `openspec validate --all --strict --no-interactive`：21 passed / 0 failed。Luna 子 agent 的只读审查确认 current 模式确实先选 route 再加入 tool image，且该测试维护不改运行时行为与契约。

测试完成后删除临时源码快照及其 Shell 测试二进制/对象文件；保留其他并发构建所用的共享依赖缓存。

## 范围

未向真实 provider 重发请求，未执行真实 session 中的工具，未修改真实 session、配置或安装程序，未重启会话。测试使用 loopback mock 和已有图片 fixture。

归档后校验：全量严格校验 20 passed / 0 failed；archive 校验 579 passed / 0 failed。临时快照已删除，定向清理快照产出的 Shell test artifacts 130 个文件，共 1.20 GiB；没有清空共享 target 或其他包缓存。
