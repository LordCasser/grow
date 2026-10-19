# Verification

## Scope and reproducibility

仅修复和验证源码；没有构建安装 release 可执行文件、替换 `~/.local/bin/grow`、重启或修改用户列出的 live sessions。

共享工作树同时存在 session suspension、planning、skill loading 等未提交改动。为分离本 change 的证据，使用 `git archive 8cc20454da9e62874170fb3ed1cc532591954947` 创建临时源码快照，覆盖本次 `ModelImageInputKey`、Shell sampling、read_file/image recovery tests；仅叠加 compaction 和 test persistence harness 中本次差异，不叠加其他功能。另包含同一任务先前已验证的中文 text-only classifier 与 current/historical tool-image 回归。Cargo target 复用本机依赖，源码目录相互隔离。

因此以下结果验证本次变更及相邻既有图片行为，不构成包含所有并发修改的完整工作树集成通过声明。

## Before the fix

`cargo test --locked --offline -p shell --lib image_input -- --test-threads=4 --nocapture`，在上述快照中先只添加回归：**16 passed, 2 failed**。

- `image_input_rejection_cache_tracks_runtime_changes_and_aliases` 在 alias-only 切换到同 transport 时失败，不能共享已有拒绝。
- `auxiliary_image_rejection_follows_resolved_route_and_blocks_recursion` 在同 alias 更新 endpoint/backend 时失败，新辅助模型被旧拒绝屏蔽。

## After the fix

所有 Rust 验证设置 `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`，使用 Rust 1.93.1、`--locked --offline` 和隔离快照 manifest。

首轮 `cargo test -p shell --lib image -- --test-threads=4`：**134 passed, 0 failed, 1 ignored**。ignored 为既有手动 48 MP memory measurement。此轮包含首次 read_file→Responses auxiliary→held projection ACK integration；最终轮再加入 known-primary 分支及后台 compaction 完整 transport 新旧校验。

最终轮：

| Command (with the flags above) | Result |
| --- | --- |
| `cargo test -p shell --lib image -- --test-threads=4` | **135 passed, 0 failed, 1 ignored**, 44.85 s；覆盖上述 known-primary 和 background image compaction 新分支。 |
| `cargo test -p shell --lib compaction -- --test-threads=4` | **90 passed, 0 failed**, 8.07 s；包含已有各 backend 的有界 compaction 请求、图片恢复与后台任务边界。 |
| `cargo test -p tool-types -p sampling-types --lib` | `tool-types` **102 passed**；`sampling-types` **301 passed**；含 JSON identity round-trip、query identity 和 image capability classifier。 |

测试过滤之间存在重叠，不将这些数相加为独立测试总数。Ignored 仍仅为既有手动 48 MP 内存测量，未把它当作通过。

`git diff --check` 通过。归档前 `openspec validate --all --strict --no-interactive`：**21 passed, 0 failed**。归档后结果见下。

## Adversarial review

覆盖矩阵、直接请求链路与 helper-level 证据的区别见 [audit.md](audit.md)。检查过 runtime key 的生成、resource 的 durable 写入/回滚、primary 与 auxiliary 冻结身份、配置重解析、防递归、existing descriptions、OCR/removal、ImageProjection ACK 后重建及后台 compaction 当前性校验。本次未扩展图片 fallback 框架或改变中性描述复用规则。

## Limits

未针对用户 live provider 发送新请求。OCR 成功、user cancellation、timeout 和 completed description cold actor restore 具有已有 helper/storage 级证据，但没有为所有组合新增完整 actor integration；不能将这些单元证据表述为整链路覆盖。测试读取文件结果的真实 bridge admission，不派发文件系统 read_file 工具执行本体。

## Archive and cleanup

`openspec archive harden-image-fallback-routing --yes` 已完成，归档场景已逐条核对上述分层证据。归档后 `openspec validate --all --strict --no-interactive`：**20 passed, 0 failed**；`openspec validate --archived --no-interactive`：**580 passed, 0 failed**。最终 `git diff --check` 通过。

验证后删除本次 `git archive` 临时源码快照、审查前源文件备份和三个测试可执行文件及其 `.d` 文件，保留共享 Cargo 依赖缓存，不清空其他工作的 target。
