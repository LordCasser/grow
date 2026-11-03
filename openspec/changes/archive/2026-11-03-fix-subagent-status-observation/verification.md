# Verification

日期：2026-11-03。工作区包含其他在制变更，以下只声明本次观察链路的证据；未运行全仓测试，也未替换用户正在运行的 Grow 进程。

## Incident evidence

用户提供 parent `01a19a05-5281-7201-bae3-89f599512353`。只读核对其 canonical 记录：

- Rust F9-1：`01a19f7c-8dbb-7a40-a0e0-8ee35eb79533`，Domain seq 11792 为 cancelled Ended，引用 child Result seq 1944。
- TS F9-2：`01a19f7d-6300-7043-b6ba-135b8eb5b13b`，Domain seq 11794 为 cancelled Ended，引用 child Result seq 586。
- 截图前的 seq 13250/13256 查询这两个原 ID，却得到 unauthorized；第三个仍在 coordinator 的 child 正常返回 running。seq 13270 随后产生截图文字。

因此本次具体故障是查询依赖 runtime 注册表，旧任务已存在的持久终态没有进入模型查询路径；不能据工作区文件把这两个任务认定为完成。

## Regressions

| 验证 | 结果与范围 |
| --- | --- |
| `cargo test --locked -p tools --lib implementations::grow_build::task -- --test-threads=4` | 204 passed。覆盖 backend 的剩余原期限、历史查询入口、既有授权/终态，以及单项和多项观察失败分类。 |
| `cargo test --locked -p shell --lib durable_history_observation -- --nocapture` | 2 passed。真实文件的 completed/failed/cancelled、17 MiB 来源日志、root/direct-parent、同树非所属来源、外部/未知/Workflow 隐藏来源、open、错误 Result 引用、输出缺失/损坏、查询前后日志字节不变。 |
| Shell test binary，filter `progress_publisher_tests` | 4 passed。网关只有一项在途时本地样本由 turn 1 推进到 2；原 ACK 返回后发送当前样本；取消关闭第二项交付。 |
| Shell test binary，exact `agent::mvp_agent::tests::subagent_restore_authority_tests::suspended_child_cold_restore_retains_birth_authority_end_to_end` | 1 passed，89.65s。复用原有跨进程冷恢复矩阵，恢复原未完成工具调用、尚未重新采样时，查询原 child 得到真实 Running 计数。 |
| Shell test binary，filter `explicit_delete_is_independent` | 2 passed。共享 reference_timeline 更名/复用后，原删除资格的 journal/sampling 预算边界不变。 |
| `git diff --check` | passed。 |

在共享 backend 文件出现其他在制改动后，再执行 `cargo test --locked -p tools --lib channel_backend -- --test-threads=4`，18 passed；本次查询入口与期限回归仍通过。

旧实现的失败已实际复现：观察不可用被投影成 running；网关等待期间计数停在 1；冷恢复查询原 child 得到 Unavailable。没有放宽这些断言来使测试通过。

共享工作区中一项在制 coordination 测试缺少 `tokio::sync::oneshot` 导入，阻止整个 Shell 测试目标编译。本次仅在该测试函数补齐导入，保留原测试正文与其他改动。

## Real-session read-only probe

临时探针从用户会话的只读 canonical snapshot 初始化真实 ChatState owner，以原 source 身份绑定 ChannelBackend，模拟 coordinator 无该注册的 Unauthorized 响应，再通过公开 query 读取真实 child Result 链。两项分别返回 cancelled，查询阶段耗时 984.315ms 与 661.467ms，均在原五秒期限内。预先加载来源历史的时间不计入 resident 查询；这不是冷加载或已运行进程的热修复测量。探针没有运行工具、恢复执行或修改用户记录，源码已移除，没有将会话正文复制入仓库。

最终实现直接复用 resident ChatState 的已接纳来源事实；nested root/child 使用既有 canonical reference 校验，所需输出继续验 hash。原期限包含 coordinator 和后续查询，不重置计时；只读 worker 退出前继续持有原许可。没有新增生命周期缓存、writer 或完成推断。

## Adversarial review

- cache 缺项本身不授予访问权；独立核对 source/root/direct-parent，Workflow 隐藏 child 仍拒绝。未授权时不打开目标 child。
- 有 Spawn 无 terminal、通信失败和超时不制造 cancelled/completed，也不再标成 confirmed running。
- Result 链损坏明确失败；输出 artifact 损坏保留 canonical 完成事实并报告 output_unavailable，不写替代结果。
- 采样与 UI ACK 分开，仍最多一项 UI 在途；runner 的独立 child-token guard 在返回/挂起时结束旧采样器，不反向取消任务。
- 复用共享 target，没有另建编译目录或清理其他在用构建；只保留最终验证记录，不提交中间日志。

OpenSpec 已归档：归档前全量严格校验 19 passed；归档后 `openspec validate --all --strict --no-interactive` 为 18 passed，`openspec validate --archived --no-interactive` 为 810 passed。三个增量要求已合入 session-timeline 主规范。
