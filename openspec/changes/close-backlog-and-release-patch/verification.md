# 2.5.4 修复与发布验证

## 范围与版本

用户确认仅清理缺陷和测试债务，Remote Control、真实 provider/负载依赖优化及独立 MCP 新功能保留说明。清理前完整清单见 backlog-before.md；逐项闭合见 inventory.md 与各独立 change。原有工作区内的上下文/压缩/恢复修复及工具/Thought 聚合一并核对和回归，未重置。

起点正式版本为 v2.5.1；初始2.5.2和后续2.5.3候选分别被门禁发现的测试债务阻止，tag保留不移动，最终patch目标2.5.4。workspace、lockfile 和 changelog 同步。发布仅使用官方 `release.yml`，所有平台与 publish 开启，不绕过 required gates。各候选执行过程及最终提交、tag、run、产物核对按以下阶段记录。

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

## 正式发布执行

- 发布源提交：`1c88fccb4adf18cc57d525685f6e87ddb26353e4`；注解 tag `v2.5.2` 精确指向该提交，已推送官方 origin/main/tag。
- 干净工作区 `bash scripts/validate-release.sh v2.5.2` 通过，日志 `/tmp/grow-2.5.2-release-preflight.log`。
- 官方 [release run 40830093920](https://github.com/LordCasser/grow/actions/runs/40830093920) 从 main dispatch，tag=v2.5.2，Linux/macOS/Windows/OHOS 与 publish 全部开启。结果待流水线完成后补齐，当前不宣称发布成功。

### 2.5.2候选被门禁阻止

正式release Core job `123302199979` 与同SHA push Core run `40830080583` 均在已有 diagnostics `blocked_writer_drops_overflow_without_blocking_producer_and_reports_it` 失败：first之前合法出现records_dropped，测试错误固定了诊断位置。日志 `/tmp/grow-2.5.2-ci-core-job.log` 与 `/tmp/grow-2.5.2-pushcore-failed.log`。已取消未发布run以停止无效构建，v2.5.2 tag保留原commit，不移动、不发布失败候选。新建独立test-only change修正同步；下一最小patch为2.5.3，最终重新执行完整门禁。前述“正式发布尚未执行/待完成”均属原候选阶段记录。

### 2.5.3本地候选

生产代码与已全量验证的2.5.2候选相同；新差异仅patch身份/changelog与日志测试同步。最终diagnostics55通过，两个溢出测试100轮/200次执行通过。原子计数已转入待写诊断时不再以中间load假设所有权；线程结束后仍精确核对记录顺序、唯一diagnostic、count=1。scoped闭包自持sender以在断言unwind时先断开队列再join。默认CLI2.5.3构建成功（1m15s），日志 `/tmp/grow-2.5.3-cli.log`。相邻firehose测试无同类固定位置断言。Windows存储同源push门禁 `40830080669` 和Ubuntu协调job已通过；仅属此前同生产源的补充证据，最终2.5.3仍需全量同SHA门禁。

- 2.5.3发布源：`fce2f3d590dd4d069f089a7afd467a31c5d4d094`，注解tag `v2.5.3`精确指向该commit；`bash scripts/validate-release.sh v2.5.3`在clean committed tree通过。官方origin/main/tag推送成功。归档后严格18/18、archive739/739通过。

### 2.5.3候选的PTY契约债务

官方run `40832137750`（源fce2f3d59）完成16包Core库、Settings235、采样失败真实进程恢复、真实daemon Drop/panic回收与前十项PTY后，在 `verb_group_fold_expand_collapse_pty` 失败。失败日志 `/tmp/grow-2.5.3-core-failed.log` 显示旧测试仍期待Read3+Search2/Edit单独/Read2，而现行归档契约要求所有连续工具和Thought属于同一活动；实际摘要Read5/Search2/Edit1正确。修复独立test-only change，仍验证真实展开、查看成员和折叠，不改生产规则、不放宽期限。失败run取消且未发布，v2.5.3 tag不移动。该候选三平台协调/导出、Windows存储、OpenSpec与Linux五平台build均通过；这些不是最终新版同SHA门禁的替代证据。

下一候选在本地逐一跑完此前未执行到的发布PTY矩阵，并先等待同SHA push Core全部通过，再创建正式tag/dispatch，以避免再次把尚未验证的测试差异固化为发布候选。

### 最终2.5.4准备

活动组PTY已按真实idle/正文消失边界完成，DONE与idle共用90s绝对期限。一次最终精确通过后又连续3次通过。余下16项发布PTY全通过，详见align-activity-group-pty-contract验证记录。2.5.4仅递增patch身份并修正此测试；生产逻辑与2.5.3相同。先等待push Core全部成功，再创建tag并dispatch全平台release。

2.5.4默认CLI构建成功（1m10s，`/tmp/grow-2.5.4-cli.log`），最终格式与diff检查通过；严格规范18/18、archive740/740通过。活动组3次重复与余下16项发布PTY全部成功后才提交候选；创建tag前等待同SHA push Core全部成功。
