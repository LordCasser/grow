# 验证记录

日期：2026-09-27。保持工作树中其他未提交修改；本 change 未提交或发布。

## 反例与修复

固定交错测试让仍在运行的 classifier 发起 ResetState、ReleaseChild 或 SetMode，随后试图返回 Allow。修复前，三个场景的请求结果都是 `Allow`，说明同步控制 API 返回后 actor 尚未撤销旧 token；不是 provider 延迟或人工提示超时。控制命令加入 one-shot ACK、调用方等待 actor 提交撤销后，同一测试三个场景均返回 `Cancelled`。

## 验证

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked -p workspace --lib permission:: -- --test-threads=4` | 400 passed、2 failed；两项原始 HEAD 已复现的 Auto reason 断言，见 backlog 与前次验证 |
| `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib permission -- --test-threads=4` | 80 passed |
| 新的固定交错控制回归 | Reset、ReleaseChild、SetMode 全部通过 |
| 涉及 Rust 文件的 `rustfmt --check --edition 2024` | passed |
| `git diff --check` | passed |
| `openspec validate --all --strict --no-interactive` | 16 passed |

## 范围外审计

高并发资源准入、旧 helper 的来源推断、Goal 并行预算语义和 Reset 落盘确认分别登记于 `openspec/backlog.md`。当前生产工具入口使用显式 `PermissionRequestSource::Child`，因此旧 helper 风险没有生产误路由证据。截图会话没有对应原始 provider/Sideband 时间线，本次固定交错证明的是控制撤销 bug，不推定截图中的具体超时也由该 bug 引起。
