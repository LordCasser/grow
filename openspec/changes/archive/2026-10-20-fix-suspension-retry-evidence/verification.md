# 验证记录

## 修复与证据

- F1：挂起按 TaskCompleted + Ordinal(1) 查询 canonical Received，已消费回执也复用；查询不可用失败关闭，未接纳才提交结算正文。Terminal 的正常完成通知持久 ACK 与命令挂起由同一 owner 串行处理，不放宽异正文冲突检查。
- F2：从 Timeline Opened/Resolved 和当前 SuspendRequested 边界重建逻辑等待；owner Drop 后和 owner 已写 Suspended 后均可重试。删除失去调用方的临时 snapshot_attempts。
- 真实问题 worker 的 foreground receiver 可能先于 service shutdown 关闭；现以 runtime_suspend 决定 Suspended，与权限/审批路径一致。非挂起的取消、超时及回答语义有既有回归保护。
- 对抗性核对包含已回答/允许/拒绝/取消/超时/失败的 attempt、旧 operation 的 Suspended、缺失/错误 operation、完成回执已消费，以及恢复 source/operation 不匹配的失败关闭。

## 命令与结果

使用仓库指定 Rust/Cargo 1.93.1（PATH 前置 ~/.cargo/bin），未使用 Homebrew 默认 1.99.1。

```sh
PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p shell --lib -- session::actor::suspension:: session::actor::tests::reverse_request_session_id_tests:: session::actor::tests::runtime_ session::actor::tool::preparation::restored_pre_tool_hook_tests:: session::pending_interaction:: --test-threads=1
```

结果：**37 passed，0 failed，4268 filtered out**，运行 3.25 秒。

- 三类等待 Permission / Question / PlanApproval × guard Drop / durable Suspended 两种清理路径，共六个跨阶段场景：首次在 owners drain 越过 deadline，原 operation 重试成功，checkpoint 保留原 call/dependency；重建 ChatState Timeline 和新 EventTracker 后执行 prepare + commit，等待仍在且没有伪造 ToolResult。
- 已有正常完成通知的 pending/consumed 两种状态可挂起；首次由挂起生成的通知在 checkpoint 前超时，原 operation 重试、重复成功请求、prepare/commit/release 后仍只有一个 receipt。
- 真实 question worker 在 shutdown 尚未取消但 foreground receiver 关闭时记录 Suspended；非挂起取消、持久化失败和回答路径一并通过。
- 既有 question / plan approval 的新 attempt 重新绑定测试、权限恢复 Hook gate、完整运行时 release 和 Workflow 同身份 child 恢复测试通过。

初始测试夹具修正记录：原始 origin 字符串并非 PromptOrigin JSON；手工 CommandsSettled 必须先有 SuspendRequested；restore 必须新建 EventTracker，不能覆盖旧物理 actor 的 active tracker。最终测试重建 Timeline 与新 actor，未通过跳过恢复或放松断言掩盖这些失败。

以下检查通过：修改的六个 Rust 文件 rustfmt --check、git diff --check，以及归档前 OpenSpec 全量严格校验（17 项）。归档后 `openspec validate --all --strict --no-interactive` 为 **16 passed / 0 failed**，`openspec validate --archived --no-interactive` 为 **651 passed / 0 failed**。

## 范围与限制

测试使用真实 SessionActor、ChatState Timeline 和等待 guard/worker；命令结算在夹具中按合法顺序预置，未运行真实 shell 子进程。人工等待跨阶段测试推进至恢复 prepare/commit 的执行 gate；真正重新打开问题/审批并答复由已有独立测试覆盖，未声称完成带真实模型、终端 UI 和进程重启的整链路测试。

没有改写或强退原故障 session，没有替换已安装二进制。已运行的旧进程不会自动获得源码修复。

## 构建清理

完成验证后执行 `PATH="$HOME/.cargo/bin:$PATH" cargo clean -p shell`，清理 782 个文件、6.5 GiB shell 编译产物。没有删除整个 target 或干扰其他包的构建。
