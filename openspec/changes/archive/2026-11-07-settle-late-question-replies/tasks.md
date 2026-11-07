# Tasks: settle-late-question-replies (调查 only)

- [x] 源码梳理 ask_user_question 全链（tool/worker/pending registry/gateway/pager 接收与提交），记录到 design.md §1
- [x] 分辨 blocking / 任务已完成 / 迟到答案三种场景可达性（design.md §2）
- [x] 记录 owner/state/代际 ID（tool_call_id + attempt_id；pager 无代际绑定）与迟到答案最早丢失点（design.md §1.6）
- [x] 给出最强竞争解释 E1–E3 与反证缺口（design.md §3）
- [x] 推荐最小修复方向与 terminal question identity 事实，供 feature-ask-blocking-choice 使用（design.md §4）
- [x] 设计最小 call-boundary fixture F1–F3（design.md §5）
- [x] 更正源码事实错误：`resolve_durably`（spawn.rs:759 调用 → pending_interaction.rs:126）与 `PendingInteractionGuard::drop` 均广播 `InteractionResolved`；pager session_notification.rs:1634/1906 → `dismiss_resolved_interaction`（interactions.rs:1426）按 session+call 关闭弹层并 restore stash；"残留/无取消链"结论证伪，无需新 cancel 协议（design.md §1）
- [x] 核对 gateway `enqueue`：`response_tx` 移入 `AcpClientMessage`，worker drop ext future 只 drop rx；"pager response_tx 已死"须按 transport task 生命周期另核，不得等同两层 oneshot（design.md §1.4，待父侧继续核 transport task）
- [x] 写入 F2/F3 测试源（`reverse_request_session_id_tests.rs`：场景1 长等待正常结算、场景2 receiver 关闭后迟到 response 惰性、retained-ACK open/resolve 分别扣留）；用户已授权本项测试
- [x] **runtime 验证已执行**（纠正"源就绪即完成"：无编译不等于实现完成，测试向量文字不代替 runtime）：
  - 判别跑：`cargo +1.93.1 test --locked --offline -p shell --lib -- <三个新 filter> rg_external_programs --test-threads=1 --nocapture` → `/tmp/grow-late-ask-discriminator.log`，5 passed / 0 failed，EXIT=0。编译修复仅限本夹具：matches! 内误置 assert message（retry2:1454）、E0507/E0277/E0716×2、helper 改用生产 `SessionNotification` 类型反序列化（手拼 JSON 键名与 wire 格式不符导致三测试一度全 panic）、retained-ACK mock 补记 held 事件入 committed。
  - 回归跑：全量 `reverse_request_session_id_tests` filter → `/tmp/grow-late-ask-worker-regress.log`，14 passed / 0 failed，EXIT=0（含原有 blocking Plan durable/Hook cancel 等）。
  - **结论如实记录：场景1/2 与 retained-ACK 均通过 = 反证。用户 bug 未复现，非修复结论；retained-ACK 只确立通用 persistence 前提，不作为用户场景 root。**
- [x] 父裁决：本调查不改产品行为、不新增cancel或durabledeadline；新需求另由offer-advisory-user-questions拥有。实际证据与原报告未复现限制见verification.md，归档仅关闭调查交付。
