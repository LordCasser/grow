## ADDED Requirements

### Requirement: Bounded post-update completion generation
自更新 SHALL 对每个 shell 的补全生成设置 10 秒子进程等待上限；超时或取消时请求终止直接子进程。补全生成命令失败 SHALL 不阻止后续补全步骤，也不覆盖已有补全文件。

#### Scenario: 补全进程超时
- **WHEN** 单个补全命令在 10 秒内未退出
- **THEN** 终止该直接子进程，保留目标文件并允许继续下一 shell。

#### Scenario: 更新等待取消
- **WHEN** 补全子进程运行期间调用方取消等待
- **THEN** 已启动的直接子进程被终止，已有补全文件保持不变。

#### Scenario: 成功与失败输出
- **WHEN** 补全进程结束
- **THEN** 只在成功且输出非空时更新目标文件；失败或空输出保留原文件。

证据入口：`crates/codegen/update/src/auto_update.rs` — `regenerate_completions`。不承诺任意派生后代树和文件系统调用的时限。
