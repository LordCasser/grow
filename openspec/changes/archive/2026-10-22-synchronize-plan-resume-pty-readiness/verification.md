# 验证与对抗性审查

发布源码为 `ac4ebb7630cf3c9b623af9d40678021c23faf1e4`。同提交 push Core run [39770985516](https://github.com/LordCasser/grow/actions/runs/39770985516) 的首次执行和一次重试均在 `deferred_plan_reviewed_after_resume` 等待 retained Plan 标题时超时。该场景只等首段历史就发送 `/view-plan`，不满足现有完整恢复后才开放输入的契约。两次失败不再按偶发问题反复重跑；发布 run [39770985598](https://github.com/LordCasser/grow/actions/runs/39770985598) 已取消，未公开 release。

修正只在 dev-only `pager-pty-harness` 中，为恢复进程配置独立日志，等待既有 `pager.replay.finalize` 后输入。未修改产品源码、超时、审批流程、断言或 CI 选择。诊断日志位于本场景独立 HOME，初始进程未使用该路径；finalize timer 在 SessionLoaded 同步状态恢复结束时才写入，不能被初始进程、历史正文或 end_batch 的提前完成满足。标记缺失时原 20 秒期限仍失败。之后仍由真实终端审阅和批准，并检查前台采样数，诊断本身不能令产品断言通过。

本机使用固定 Rust 1.93.1、独立 HOME/GROW_HOME 和专用 `target/plan-resume-readiness-250`；模型为本地受控 mock，无个人 provider 配置或付费请求。CLI 使用该发布 run 的 macOS ARM64 发行 archive，来源证明核对 signer workflow 和 source digest `ac4ebb7630cf3c9b623af9d40678021c23faf1e4` 通过，codesign 严格校验通过，隔离环境版本为 `grow 2.5.0 (ac4ebb7)`。不是新修复提交的重新构建，因本次只改 dev-only 夹具，可用于核对原产品的恢复行为，不能代替修复提交的正式 CI。

- `cargo test --locked -p pager-pty-harness --test plan_approval_resume --no-run --message-format=json`：通过，无编译 warning。
- 显式 `PAGER_BINARY` 指向上述 CI binary，执行该 test executable，`--test-threads=1 --nocapture`：两项通过，0 failed / ignored，12.87 秒。覆盖 live approval 和 deferred review 两种跨进程恢复，保留正文、Review、批准后恰好一次实施的原断言。
- `cargo clippy --locked -p pager-pty-harness --test plan_approval_resume -- -D warnings`：通过。
- 固定工具链 `cargo fmt --all -- --check`、`git diff --check`：通过。

边界：本机通过尚不能宣称 Linux 失败已在托管 CI 消除；必须让包含夹具修正的新提交重新通过官方门禁。未提前开放用户输入，未做就绪与历史分页分离。发布 tag 的后续处理另行确认，未移动已推送的 v2.5.0。

清理：删除本轮专用编译树，删除前 allocated 为 5132636 KiB；未删除共享 target/debug、target/release-dist 或现有本机候选。按本轮 CI binary 路径检查测试子进程，剩余 0。临时下载与原始日志留在工作区外，待发布决策后统一清理。

归档后严格校验：17 项当前规范/活动 change 通过；700 项 archive tasks 校验通过。该 change 已完成并归档；发布 change 继续保持未完成，等待精确提交与版本标签决策。

后续托管确认：纳入最终 source 8443a4d1648d17b10f993c845cb68d949c55235e 后，[官方发布 Core](https://github.com/LordCasser/grow/actions/runs/39797866048) 与 [push Core](https://github.com/LordCasser/grow/actions/runs/39797846368) 均通过；发布日志实际执行并通过两种 Plan resume 场景，原 Linux 失败已在新夹具下消除。
