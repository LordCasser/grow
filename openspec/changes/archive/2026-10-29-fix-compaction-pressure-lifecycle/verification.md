# 验证记录（2026-10-29）

## 环境与范围
macOS arm64，Rust 1.93.1；`--locked`，`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`。在当前工作区验证，保留已有无关改动。用户 session 仅只读审计，未改写历史、未重放历史工具，也未替换正在运行的 Grow。

## 回归结果
| 验证 | 结果 |
| --- | --- |
| ChatState 全库 | 586 passed，3 ignored，0 failed |
| Shell 全库首轮 | 4373 passed，9 ignored；唯一失败为新增 same-slug 测试错误地假定新回合仍保留 SUPPRESS_TURN |
| 修正该测试后的 Shell `compaction` 套件 | 101 passed，0 failed，包含上述失败项、同 slug 窗口降低、child output budget、ACK failure、模型切换、取消与 committed+error |
| Pager `compaction` 套件 | 19 passed，0 failed，包含 finish_turn 清理 compaction feedback 并保留其它域反馈 |
| 非测试配置构建 | `cargo build --locked -p chat-state -p shell -p pager --lib` 通过 |
| 格式和差异 | `cargo fmt --all -- --check`、`git diff --check` 通过 |
| 归档前 OpenSpec | `openspec validate --all --strict --no-interactive`：17 passed |

Shell 首轮唯一失败未发现产品缺陷：同 slug 不走 model-switch 专用入口，但普通 pre-sampling 仍进行压缩恢复；SUPPRESS_TURN 按既有约定在新回合清除。修正测试以验证恢复 Started/Failed 各一次，继续保留零 provider 请求断言。此后生产代码没有变动。最终已覆盖 Shell 全库全部 4374 个非 ignored 测试；不将定向复验描述为另一轮全库运行。

为避免 Cargo 的 Pager build script 在每个过滤器调用时重编译，先通过同一组包 `cargo test --locked -p chat-state -p shell -p pager --lib --no-run` 得到测试可执行文件，再从各包工作目录执行全库或指定过滤器。每次核对了非零测试数量。

## 场景覆盖
- omitted reasoning 的 raw 减量大于 provider pressure 时，compaction 不归零；partial replacement、prune 和重复 build 保持有效请求口径与校准。
- rewind/route 重建、失败 ACK、native continuation、Goal/image 投影及 learned reasoning 使用 ChatState 全库回归；新增图片路由失效、native reset、reasoning 启用的即时压力通知断言。
- 大模型切到小窗口、同 slug 改 window、持续 suppression、child output budget 均不能发送明确超窗普通请求。
- replacement 已提交但仍超窗：手动/自动各一个 Completed，错误继续传播，旧 pending async notice 被清除；manual 错误带已发布终态标记。
- pre-prune 的永久 StorageFull ACK 失败向上传播；写入前 Invalid 保留原摘要 fallback。promoted summary cancellation 只有一个 Cancelled，无 Completed/Failed。
- Pager 结束回合清理遗留 compaction feedback，保留其它反馈。

## 验证限制
本轮未调用真实模型复现用户生产 session，也没有声称在所有 tokenizer 下精确计数；本地有效估值仍使用现有估算器并接受 provider 校准。没有安装或重启用户的 Grow。非测试 library 构建不等于 release CLI 发布或跨平台验收。真正超窗且一次有界摘要无法适配时仍会保留历史并终止本轮，见 [审查边界](review.md)。

## 归档结果
`openspec archive fix-compaction-pressure-lifecycle --yes` 已合入三个 capability。归档后严格全量校验 16 passed；archive 校验 716 passed，均无失败。

验证完成后执行 `cargo clean`，移除 15,173 个文件、7.6 GiB 编译产物；确认 target 已移除。
