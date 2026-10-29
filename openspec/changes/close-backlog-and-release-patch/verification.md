# 2.5.2 修复与发布验证

## 范围与版本

用户确认仅清理缺陷和测试债务，Remote Control、真实 provider/负载依赖优化及独立 MCP 新功能保留说明。清理前完整清单见 backlog-before.md；逐项闭合见 inventory.md 与各独立 change。原有工作区内的上下文/压缩/恢复修复及工具/Thought 聚合一并核对和回归，未重置。

当前正式版本为 v2.5.1，本次最小 patch 递增为 2.5.2；workspace、lockfile 和 changelog 同步。发布仅使用官方 `release.yml`，所有平台与 publish 开启，不绕过 required gates。正式发布尚未执行，提交、tag、run、产物核对在本文件后续补齐。

## 本地环境与最终结果

macOS arm64，Rust 1.93.1；锁定依赖、默认 feature。`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`，测试线程4；真实进程用隔离 home/socket。未使用线上 provider。

最初的16包 Core 聚合发现 diagnostics Chrome夹具、Pager watcher/config启动间隙及一处真实canonical descendant高水位夹具失败，均修复后复跑受影响全包，未跳过 MCP 测试。原始初轮日志 `/tmp/grow-2.5.2-core-final.log` 含失败，不能作为整条命令成功证据。

| 最终验证 | 结果 / 本地日志 |
| --- | --- |
| Pager library | 7620 passed / 16 ignored，`grow-2.5.2-pager-final3.log`；含全部 MCP hot reload |
| Shell library | 4413 passed / 9 ignored，`grow-2.5.2-shell-last.log`；不同feature union的较早全Core为4417，不是删除测试 |
| ChatState / agent / sampling-types / sampler | 602/482/315/259 passed，初轮Core对应包均通过 |
| memory / workflow / pager-minimal / tool-types | 302/65/95/123 passed，初轮Core对应包均通过 |
| Tools | 2428 passed / 7 ignored，包含LSP refresh握手 |
| diagnostics | 54 passed，`grow-2.5.2-diagnostics-final2.log` |
| pager-render / tty-utils / test-support | 1022/29/71 passed（pager-render 1 ignored），affected-final/Core各包结果 |
| Settings integration | 235 passed，`grow-2.5.2-settings.log` |
| Git identity真实仓库 | 4 passed，`grow-2.5.2-git-identity.log` |
| 多客户端真实PTY | 5 passed，`grow-2.5.2-leader-pty.log` |
| 真实daemon正常Drop/模拟panic | 1 passed，`grow-2.5.2-daemon-owner-final.log`；模拟panic由catch_unwind截住，检查已wait/reap |
| 协议失败继续/切模型/进程重启 | 精确1 passed，`grow-2.5.2-protocol-recovery.log` |
| local IPC | 7 passed，`grow-2.5.2-ipc.log` |
| Python workflow/runner契约 | 34 tests / 1 skip，`grow-2.5.2-script-tests-final.log` |
| 默认CLI build | 成功，`grow-2.5.2-cli-final.log`；本地debug显示2.5.2/alpha，正式身份由release-dist构建 |

Shell全量包含真实文件/SIGKILL的compaction恢复测试，覆盖replacement已落盘但ACK未返回后新进程恢复；该结论不等同物理断电。低值provider校准、连续压缩再增长、显式输出预留、Goal cold恢复/Control/route/rewind采用真实actor/受控persist；不宣称真实provider故障实测。

OS watcher注册无法取消；LocalSet可先结束等待，runtime仍持有阻塞任务，shutdown可能等待系统返回。native交错与Markdown clone按调用证据关闭待证实项，不虚构性能收益。未启用的跨多选区连续摘要与provider默认输出预留均保留明确契约。

## 规范与发布检查

归档前全量OpenSpec 33/33通过。最终格式、diff检查、所有已完成change归档与archive校验、本地clean-tree发布preflight、正式CI及产物见下方后续记录。

- Debug-only panic测试在完整debug Pager中通过；release配置（默认debug_assertions=false，opt-level0/LTOfalse以只验证编译条件）枚举为0，该模块其他5项枚举及运行全部通过，未跳过整个模块。
- 归档中检出精确导航delta误标MODIFIED但主规范无同名条目，改为ADDED后归档，保留说明；未移除失败行为场景。

- 15个本轮已完成独立change已归档；归档后全量严格校验18/18通过，archive验证738/738通过。旧MCP多轮提案与发布阶段记录保持活动，不假勾未完成项。
- `cargo fmt --all --check`通过；staged源码/规范/开发文档的diff whitespace检查通过。既有原始验证日志和reproduction.patch保留原样，其中有尾空白/空行，不通过全文件whitespace检查，未为检查改写原始证据。连续两次同环境默认CLI构建均无编译，0.99s与0.38s，验证Git监听修复未造成无修改重编译。
