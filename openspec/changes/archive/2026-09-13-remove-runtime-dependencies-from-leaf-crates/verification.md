# 第三轮架构优化：实施与验证

本轮完成四个独立变更：会话只读观察、Messages 工具 ID 编码、Workflow 有效项恢复上限、五条反向直接依赖。前两轮的恢复布局和 Timeline 优化保持；没有新增 crate 或外部依赖版本。

## 实施边界

| 变更 | 实际结果 |
| --- | --- |
| [keep-session-observation-read-only](../2026-09-13-keep-session-observation-read-only/verification.md) | full/light 只读派生 canonical Summary；持有 lease 的显式 writer restore 才修复持久投影，保留冲突/损坏拒绝。 |
| [encode-messages-tool-identities](../2026-09-13-encode-messages-tool-identities/verification.md) | 预留 native/合法 ID，摘要候选经请求内集合消解碰撞；调用/结果共用映射，普通合法路径早返回。 |
| [restore-latest-valid-workflow-runs](../2026-09-13-restore-latest-valid-workflow-runs/verification.md) | 逆序借用 Timeline 候选，成功恢复才计入 128 上限；返回保持时间顺序。 |
| remove-runtime-dependencies-from-leaf-crates | 共享值类型使用原有 tool-types/config-types，权限特殊行由 Pager 判定，更新配置操作由 CLI 组合。 |

## 验证结果

11 个 crate 的完整库测试共 13,386 passed、0 failed、16 ignored（既有 ignored 项）：

| crate | passed | ignored |
| --- | ---: | ---: |
| chat_state | 483 | 1 |
| config | 94 | 0 |
| config_types | 48 | 0 |
| grow_http | 10 | 0 |
| pager | 7201 | 11 |
| pager_render | 1002 | 1 |
| sampler | 241 | 0 |
| sampling_types | 278 | 0 |
| shell | 3795 | 3 |
| tool_types | 101 | 0 |
| update | 133 | 0 |

另通过 update 的 I/O/network 集成测试 25 + 17 项、CLI 主程序测试 35 项、实际 debug grow 的 continue/resume PTY 1 项。合计 13,464 项通过，重复执行的测试不重复计数。sampling-types 的最终快速路径另补跑全部 278 项通过。CLI debug 构建成功，保留原有 macOS compact-unwind linker warning。

命令及完整结果见 validation/ 下日志。核心入口：

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --lib -p tool-types -p config -p config-types -p grow-http -p sampling-types -p sampler -p pager-render -p update -p chat-state -p shell -p pager -- --test-threads=4
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p update --test test_io --test test_network -- --test-threads=4
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p cli --bin grow -- --test-threads=4
CARGO_BUILD_JOBS=2 cargo build --locked -p cli --bin grow
```

PTY 使用前轮已构建的 persistence harness，通过 PAGER_BINARY 指向本轮最终 target/debug/grow，验证 --continue 历史恢复和后续输入提交；没有为这项检查再构建一套 harness。

## 依赖复核

cargo metadata 明确移除 sampling-types → tools、grow-http → workspace/sampler、pager-render → workspace、update → shell。现有 core-regression workflow 增加这五条边的断言，本机执行同一检查通过。dependency-review.json 保存前后图；其中可达数包含 metadata 列出的 dev/optional 边，不等于实际链接依赖或构建速度。

VersionPolicy 仅移动模块路径并格式化，解析/夹取规则和对应测试保留。原 shell 配置解析、persist 和共享写锁没有改变；没有复制 CliConfig。channel 切换仍保留显式选择意图，允许原有 alpha → stable 降级判断；pinned 安装成功后才 await 配置写入 future。

## 审阅与未采用方案

新配置写入器候选因拆分共享锁、减少严格校验范围而撤回，最终使用组合层注入。初次源码编译发现 Arc<str> 转换和 channel_switch 参数搬迁遗漏，均已修正；离线测试因缺少已有锁定依赖 assert-json-diff 而未启动，联网获取该版本后完成。失败日志保留，未通过删除错误场景放宽检查。

新发现的相同原始 ID 跨 neutral/native 复用属于既有请求投影歧义，区别于本轮不同原始 ID 的编码碰撞，已记 backlog。Workflow Forgotten/checkpoint、请求投影共同遍历、在线 append 及领域封装仍需独立设计或测量。此次没有改写这些持久化语义。

## 磁盘与局限

全程单 Cargo 进程、2 jobs、复用 target，无新 worktree、release 或自定义 profile。构建前记录 36 个已有增量根目录，构建后确认无 Cargo/rustc 进程，仅删除本轮新增且出生时间晚于基线的 133 个根目录；原 36 个全部保留。编译产物与最终 grow 可执行文件保留。

清理目录的 du 合计约 13.95 GiB；文件系统可用空间从 34.03 GiB 到 43.62 GiB。du 与实际空间变化不同，不将逻辑目录量当作实际回收量。证据见 validation/disk-cleanup.json。

本轮没有新增性能倍数结论。前轮 Pager 进程内阶段约 41% 的改善不是全链路时延；真实长会话终端按键 p95 仍未测得。当前只执行 macOS 验证，Windows runner、真实 provider 调用未执行。

四个 change 均已归档。归档前严格验证 21/21 通过；归档后全量严格验证 17/17、archive 验证 332/332 通过。其余三个既有 active change 未改动。
