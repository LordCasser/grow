# 验证

## RED 与根因

新全表生命周期用例实际失败：`every_writable_setting_dispatch_failure_retry_reset_and_rollback_lifecycle` 的 show_tips 在成功保存 true、reset false 保存失败后得到 Some(Bool(false))，预期最近成功的 Some(Bool(true))。完整库组当时 332 passed、1 failed、1 ignored；失败断言保留。

生产证据：current_value_for 使用 show_tips None→false / auto_update None→true，而 pr13_effective_default 仍为 true / false，set_show_tips 的 prev_effective 也为 true。旧 rollback 将最近成功的非默认值误判为 default 后清为 None，回显反转。auto_update 的同形偏差由代码核对确认，未声称另有独立旧版本 RED 执行。

## 修复与 GREEN

仅修正 setters.rs 三处默认字面值，未修改 writer、文件格式和队列算法。旧 show_tips 测试继续验证首个默认值提交、相同提交幂等、回滚 None 及两阶段 payload，将错误旧默认期望与输入更新为当前 false。全表生命周期和代表队列用例明确覆盖 show_tips / auto_update 的初次失败、最近成功基线、reset 失败及重试。

Rust 1.93.1；`cargo test --locked -p pager --lib --test settings_e2e --no-run --message-format=json` 成功。沿现有 `RUST_MIN_STACK=16777216`、Pager package cwd 直接运行该次 Cargo 实际产物：完整 integration 235 passed、0 failed、0 ignored；library `settings --test-threads=4` 为 333 passed、0 failed、1 ignored（原有 picker_visual_smoke_debug）。库与 integration 有覆盖交集，不累加为独立场景总数。日志保存在本机 /tmp/grow-settings-interactions-{integration,library}.log。

此变更由补覆盖发现的确定缺陷触发，产品改动独立记账。测试路径及适用范围见 [覆盖范围](../2026-10-20-complete-settings-interaction-regressions/coverage.md)；本轮只提交，不推送或发布。

最终校验：修改 Rust 文件 rustfmt --check、git diff --check 通过；归档前 current/all strict 18 passed、0 failed；归档后 current/all strict 16 passed、0 failed，archived strict 666 passed、0 failed。
