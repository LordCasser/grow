## 根因与改动

`handle_coordination_inquiry` 原先直接对冻结 Surface 调用 `project_portable_history`。该投影保留附件，不按图片负能力选择文字；前台 ImageProjection 保留的「原图 + 描述」因此仍作为图片发给新纯文本模型。与此相比，`handle_side_question` 已等待主投影并选择已确认文字。

共用 `freeze_sideband_context` 后，两种只读问答都等待主投影的 durable ACK、选择已描述图片的文字，并保留 portable 工具证据。协调 client、显式模型与图片能力身份由同一 prepared config 生成。主图片转换和负能力记录仍由原所有者负责。

## 验证

- 旧行为复现：新增 `coordination_reuses_image_descriptions_after_model_switch_on_all_backends` 在旧实现失败，实际 HTTP 请求包含 1 张原图，预期为 0。首次默认小栈执行发生 stack overflow；采用文件已有的 `run_with_session_stack` 夹具后，新测试可由普通 Cargo 命令运行。栈问题属于测试执行环境，没有放宽图片断言。
- `cargo test --locked -p shell --lib session::actor::tests::image_input_recovery_tests::coordination_ -- --nocapture`：5 passed。图片分支矩阵覆盖 Messages、Chat Completions、Responses，用户附件与工具结果，有/无负能力缓存；共 12 个切换组合。其余用例覆盖转换中等待、ImageProjection ACK、等待期间切换 endpoint/model、请求前释放读锁、询问及 session 取消、已知未描述图片、未知路由真实 HTTP 拒绝、主投影失败。
- `RUST_MIN_STACK=33554432 cargo test --locked -p shell --lib session::actor::coordination::tests -- --test-threads=4`：12 passed，包含三 backend 完整工具证据、FIFO、取消、审批、InfoRequest 生命周期及恢复身份。
- `RUST_MIN_STACK=33554432 target/debug/deps/shell-fa5e9ad84e948877 side_question --test-threads=4`：12 passed。使用上一条 Cargo 命令构建的同一测试产物，覆盖共用入口的既有图片等待/失败/取消、配置快照、工具配对和重试行为。
- `git diff --check`（本次修改路径）：通过。
- `openspec validate --all --strict --no-interactive`：18 passed，0 failed。

编译存在原有 Atomic `fetch_update` deprecated 与 macOS 大 unwind section 警告，无编译错误。所有 provider 测试使用 loopback mock HTTP，不使用在线付费模型；没有执行真实服务验收或完整 CLI 多进程脚本。

## 对抗审查

- 未按图片标签猜模型能力；描述复用独立于缓存，未标记且未转换的图片仍保持首次乐观语义。
- 已知纯文本路由缺描述时直接失败，不删除图片、不调用视觉辅助、不写负能力；未知路由拒绝也不越权恢复主 Surface。
- 主转换失败不发布未确认描述；已确认描述只改请求副本。矩阵用例比较询问前后主 conversation 完全一致，且只发生主会话已有的一次投影。
- 等待持有的图片读锁在 InfoRequest attempt 持久化前释放，provider 不阻塞后续主图片转换；取消不会留下新的 provider 请求。
- 等待 ACK 时变更路由，实际 HTTP 仍发送至原 endpoint，并保留原模型，未拼入新模型或新能力身份。

## 边界与工作区

工作区有大量预先存在及并行任务的修改；本次只修改三个 actor 实现文件、既有图片测试文件、对应开发说明及本 change。未变更 sampler 编码、图片持久格式或协调传输。构建复用现有 shared target（开始约 88 GiB），其他 Cargo 任务同时使用该目录，未删除共享构建缓存或创建独立 target。

未知路由且主会话从未产生可用描述时，首次咨询仍可能收到 provider 的图片能力拒绝；本修复针对已提交文字分支遗漏及转换等待，不授予咨询修改主图片状态的权限。

## 归档校验

`openspec archive fix-coordination-image-context --yes` 合入 local-coordination 新要求。归档后全量 strict 校验为 17 passed；archive 首次校验仅报告本 change 尚未勾选的归档后验证任务（804 passed、1 failed），执行并登记后补齐该任务，最终 archive 校验为 805 passed、0 failed。`rustfmt --check --edition 2024 --config skip_children=true` 对 sideband、recap 及图片回归文件通过；coordination 只按本次新增片段保持格式，未格式化其原有用户改动。
