# 生命周期回归覆盖记录

本记录区分新加的整链路证据与仍未覆盖的组合。最初主工作树构建因并行 planning 字段变更失败，本 change 以隔离 HEAD 快照验证；具体命令与最终结果见 verification.md。早期 fixture 编译/初始化问题不作为生产缺陷证据。

测试位置：`crates/codegen/shell/src/session/actor/tests/image_input_recovery_tests.rs`。

## OCR 成功、空结果与失败

`image_fallback_ocr_success_empty_and_failure_complete_the_durable_request_chain` 在 Unix 上以 task-local 可执行文件替身运行本地 OCR，逐次覆盖三种进程结果：输出 `OCR dialog E81` 后成功退出、成功退出但无输出、以及退出码 1。每轮输入均是实际 PNG 图片；MockInferenceServer 依次返回 primary 多模态能力拒绝、auxiliary 请求 400、以及 primary 继续响应。

测试在 ImageProjection ACK gate 处暂停，断言 projection ACK 前只有原始 primary 与 auxiliary 两个请求、turn 尚未结束，OCR 文件收到的确实是 PNG 字节。ACK 后再断言 primary 重试增加到第三个请求，前两个请求仍带图，重建请求不带图。成功 OCR 使用 `LocalOcr` provenance、包含识别文本并保留一个影子图片组；空输出和 OCR 失败使用 `UnsupportedModel` provenance、插入规范占位文字且不再保留活动图片组。三种结果都检查 Timeline 中 sealed 原图仍存在。

边界：此用例将 auxiliary 失败固定为 HTTP 400，未覆盖 OCR 超时、取消 OCR 子进程、非 Unix 平台，也不覆盖 auxiliary 返回有效描述的路径（由其他测试负责）。当前断言证明 projection ACK 是重试门槛；尚未证明实时 OCR 在所有平台上的进程行为。

## 取消

`cancelling_image_fallback_before_or_during_auxiliary_stops_later_work` 构造两个含图片的输入组，并实际启动 foreground `AgentTask`。三个 phase 分别阻塞 primary 的能力拒绝响应、主模型拒绝后的首个辅助 Attempt 持久化 ACK、以及首个辅助 HTTP 请求。三种情况下都通过 `cancel_running_task` 取消任务，等待真实 producer 完成后再释放 MockInferenceServer barrier，避免把“稍后才发请求”误判为已停止。

断言包含：primary 请求数为 1；辅助请求数在准入前为 0、运行中为 1；本地 OCR 调用计数为 0；无 ImageProjection；两个原始图片组仍在 conversation。运行中取消分支特别验证第二组不会排队发出，且取消后没有 primary 重试。

`cancelling_image_fallback_during_ocr_reaps_process_without_resubmission` 另覆盖 OCR 子进程已启动后的取消：读取实际子进程 PID，经真实 Stop 入口中止正在运行的 `handle_prompt` producer，显式 join 生产任务并检查 PID 已消失。断言没有投影、没有主请求重试、没有额外辅助请求，原图仍在当前 Surface。此用例为 task-local OCR 程序使用直接 prompt future，foreground abort handle 指向该 future；前面的三阶段取消使用生产 `AgentTask::new_prompt` 入口。

仍未单独覆盖第二组 auxiliary、deadline 到期和投影 ACK 挂起期间取消；本记录不声称覆盖这些组合。

## 冷恢复与 completed Sideband 描述复用

以下四个测试共享 `assert_image_fallback_cold_restore`：

- `image_fallback_cold_restore_reuses_completed_description_before_projection`：Sideband 已完成，但投影 ACK 尚未确认时中止原 turn；新 actor 从同一 JSONL storage 装载 Timeline 并使用新建的空描述 cache。
- `image_fallback_cold_restore_reuses_acknowledged_projection`：原 actor 完成 projection ACK 与 primary 重试后，再创建新 actor。
- `image_fallback_cold_restore_reuses_description_without_auxiliary_route`：恢复前关闭辅助配置，仍必须复用已完成 Sideband，不进入 OCR/removal。
- `image_fallback_cold_restore_reuses_acknowledged_ocr_projection`：Unix OCR 分支在辅助 Sideband 失败后生成本地 OCR 描述并完成投影 ACK 后，再创建新 actor。

这些测试都使用实际 fallback 与 JSONL session；视觉描述对应 Completed Sideband，OCR 对应 Failed auxiliary Sideband 与 LocalOcr projection。通过断开旧 actor 的 `Arc` 确认其 cache 已释放，再创建 restored actor。恢复后提交包含同一图片来源的请求，断言服务器总请求数只增加一次（只有恢复后的 primary 请求），vision-model 请求总数仍为一次，重建请求不带图且包含已持久化描述；同时确认原始 sealed 图片仍在加载出的 Timeline。投影前 crash-window 用例还断言 ACK 前转 task 被中止；已投影分支确认恢复 actor 的 projection attempts 数符合是否已有投影的预期。

边界：这些用例聚焦同一 session 与同一 immutable source 的复用，包含恢复后新输入改变全局 revision。存储回归另核对不同来源/prompt、未持久化的查询修订、未来 candidate 修订、未完成结果、rewind 丢弃来源及损坏的其他 Sideband。当前 cold restore helper 给 auxiliary HTTP 200 的路径以及 OCR auxiliary 失败后本地 OCR 的路径；不覆盖辅助请求取消/超时后的重启恢复。`acknowledged_ocr_projection` 仅 Unix 编译。这是 actor + ChatState + 持久化/资源重新装载 + HTTP 的整链路测试，不是启动第二个 CLI 进程或调用 live provider。

## 当前验证状态

执行结果见 verification.md。第一条修复前有效运行是 5 passed、1 failed：未投影冷恢复因全局 revision 相等条件重复调用辅助模型；另一个配置关闭回归在第一条修复后稳定失败（7 passed、1 failed），证实描述查询不应依赖当前辅助路由。更早的 fixture 编译/初始化问题不作为生产缺陷证据。最终结果由 verification.md 记录。
