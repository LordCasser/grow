## ADDED Requirements

### Requirement: Update version probe process lifetime
自更新的目标版本探测和候选二进制冒烟检查 SHALL 将直接校验子进程的生命周期绑定到本次等待；超时或取消等待后 SHALL 请求终止该直接子进程，避免校验失败后继续后台运行。

#### Scenario: 校验超时
- **WHEN** `--version` 进程未在既定超时内退出
- **THEN** 校验返回失败，直接校验子进程被终止。

#### Scenario: 等待被取消
- **WHEN** 调用方取消正在运行的版本校验 future
- **THEN** 已启动的直接校验子进程被终止。

#### Scenario: 正常返回
- **WHEN** 版本进程正常退出并返回有效版本
- **THEN** 版本探测返回解析后的版本，冒烟检查返回成功。

证据入口：`crates/codegen/update/src/auto_update.rs` — `probe_version_by_exec`、`smoke_test_binary`。此要求不包含刻意脱离父进程的后台更新任务，也不承诺终止任意派生后代树。
