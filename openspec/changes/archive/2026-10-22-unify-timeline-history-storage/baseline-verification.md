# 验证记录（实施验证进行中）

本 change **正在实施、未归档**。下列 before 数据来自 C2 后、C3 前的保留 executable；不能把基线或 DTO golden 当作本次实现通过。统一日志、typed projector、ACK/屏障和消费者迁移已落地；最终 release 构建、运行时矩阵及配对测量仍需全部完成。审查过程见 [adversarial-review](adversarial-review.md)。

## CLI correctness 阶段记录

`final-12-cli-results.json` 是无效尝试，不作通过证据：runner 错把退出码 0 当作测试通过，实际前 7 项均被 ignore，4 项选择了 0 tests；其 `PAGER_BINARY` 还指向 pager unit-test executable。余下两项真实失败，分别遇到 ACP initialize transport 关闭和 mock LLM 未配置。

随后使用 CLI3（SHA-256 `f5cbf930408698612a8acefa629b71f424247b633dee32ce087137bb96f44a4e`），将 `PAGER_BINARY` 与 `GROW_BINARY` 均设为该生产 CLI，并按 `--list` 确认完整测试名，以 `--exact --ignored` 执行首 11 项。前 7 项各报告 `1 passed`；第 8 项 minimal transcript 测试因 fixture 未 seed mock LLM config 而失败；三个 Leader 测试因共享 `LeaderCluster` 缺 provider config 而在父线程指示后中止。对应日志和逐项状态见 `final-13-cli-real-01.log` 至 `final-13-cli-real-11.log` 与 `final-13-cli-real-results.json`。这四项的测试 fixture 已补 `seed_llm_config`；现等待 fixture-build-14 manifest 后重跑第 8–11 项。

build13 的 shell ignored 测试中，tool-duration 测试报告 `1 passed`，日志为 `final-13-cli-test_tool_dispatch_duration_smoke.log`。Orphan reconciliation 测试进入 session/load 后失败，ACP 返回“Session ended while loading; retry the session load.”；`final-13-cli-test_subagent_orphan_reconcile.log` 未包含 child stderr，当前不能确认 actor 关闭的底层原因。现有 `load_session_with_timeout` wrapper 已调整为保留 ACP 错误并附 reader stderr，没有新增 API；需重编此 shell integration target 后再定位和验证。两项结果均不替代 C3 的最终运行矩阵。

这批结果只确认前 7 个指定 PTY 场景通过，不能替代最终 C3 correctness、release 配对测量或 OpenSpec strict 校验；本 change 仍未完成。

## Release 恢复基线

固定 Rust 1.93.1、普通 `--release`、jobs=4、incremental=0，配置与前两项一致。五组各三个独立进程；冷进程、温热页缓存，夹具生成不进入 RPC。正式矩阵没有与本任务编译或正确性运行重叠；用户其他主机负载不受控制。原始记录见 `before-measurements.json`、逐轮 txt/trace 和 `before-build-context.json`。

| Profile | RPC median ms | 首条历史 median ms | RPC min–max ms |
| --- | ---: | ---: | ---: |
| modern-128 | 104.647 | 39.205 | 95.067–115.183 |
| modern-512 | 213.501 | 118.510 | 212.844–217.823 |
| modern-2048 | 1048.748 | 574.041 | 753.965–1084.552 |
| modern-repair-512 | 527.583 | 409.150 | 471.663–569.511 |
| modern-short-512 | 136.589 | 42.930 | 116.623–137.264 |

2048 轮范围明显波动；后续正式对比必须使用保留的 before executable 交替复测，不能仅用较高的中位数算收益。该样本目录 148178363 bytes / 5 files，其中 Timeline 71032018、updates 77140456 bytes。完整目录计数不代表真实会话的其他 artifacts 也具有相同比例。

整进程 peak RSS 包含 fixture 和后置完整 updates 读取，不是 isolated-load 峰值。四组 before 完整历史 oracle（128、2048、repair512、route512）通过；这些正确性运行与 measurement-only 编译同时进行，耗时不纳入正式性能数据。

## 正常 response durable 提交

新增 ignored test `session::storage::jsonl::response_commit_perf::normal_response_storage_commit_release`，只加测量，不改生产路径。每种正文 3 个 warmup + 21 个计时 response，每进程用真实合法 turn/step/request/admission；三个进程独立运行。setup、turn/step closure、最终完整 Timeline fold 不计入窗口。

计时窗口覆盖旧路径的 candidate anchor exact durable append、Timeline admission、含 digest 的 projection 构造、projection durable commit。32 KiB response 的各进程 median 为 41.215 / 31.235 / 41.405 ms；4 KiB 为 39.561 / 26.106 / 28.675 ms。不要将它称为整 turn 或 CLI 延迟。

同步探针在计时窗口开始前 reset，窗口结束后、关闭 turn 前读取；每个 21 轮样本均有 update file/directory 各 42、Timeline file/directory 各 21 个探针点。只统计两类 ledger，不含 summary、artifact 或其他同步 syscall。计时包含适用的 summary bookkeeping。三个进程均通过末尾完整 fold 和 48 个 admission 的计数核对。

`before-commit-*.txt` 保留每进程的 median/p95 与计数，`before-commit-measurements.json` 汇总。未保存每个 response 的原始 21 条样本，因此不能从这些文件重新计算其他分位数。首次 Python 收集器误以为 marker 位于行首，只解析到一条；Rust test 本身通过。收集器改为在行内定位 marker 后复用该原始输出，并完成另外两个进程，没有删除失败的 Rust 测试来补成绩。

## 对抗审查与范围确认

已逐实际 producer 核对持久/瞬时边界，见 [source-inventory](source-inventory.md)。`SubagentProgress` 在 early return 之前持久化；Workflow/Goal/Control expectation 缺失历史正文；display echo 与模型输入不等价；通知桥缺少 ChatState owner。不可把这些路径删除或原封装成 opaque Timeline envelope。

完整删除 updates 与保留精简展示账本的比较见 [scope-review](scope-review.md)。用户已要求继续原第三项的完整合并方案。现实现按闭合 typed History 事实与 Domain 共用物理日志，不采用独立精简展示账本。

## 展示字段 golden

`tests/fixtures/history-wire-input.json` 的 46 条样例经过当前生产 Grow/ACP DTO 解码，再编码与输入逐值相等，随后保存 `history-wire-golden.json` 并重新比较通过。覆盖实际持久族的 DTO、七类 ACP、图片恢复五态、Retry 三态，以及可信用量和已清除 cost 的 incomplete 用量。SamplingAttempt 是额外的控制 DTO 样例，不因此变成持久历史。完整 producer 覆盖与是否持久化以 source-inventory 为准。

这是手工输入的 **wire 字段 golden**，不是实际 producer 的历史捕获，不能证明顺序、因果、预算、脱敏或恢复路径。输入预检最初发现 RetryState 嵌套错误、图片字段命名错误和 PluginInfo 的未知字段；另有 false 默认字段按真实 serde 规则省略。已修正输入，未放宽解码器或从比较中滤掉真实 payload 字段。原失败输出保留在 `golden-capture*.txt`；最终两次通过见 `golden-final-capture.txt`、`golden-verification.txt`，身份与源码 hash 见 `golden-context.json`。

## 仍需完成

把全族展示 golden 接入新存储 projector 验证、选定并实现存储布局与所有读者迁移、checksum/ACK/并发/分支与 TUI 矩阵、after 与配对基准、独立 strict 和归档。所有正式 tasks 保持未勾选；不能只凭基线检查归档本 change。

## 基线阶段构建清理（实施前已完成）

measurement-only 与最终 golden 的 release 构建完成；最后两次 golden capture/verification 各为 1 passed。Pager 的五条既有 clone warning 未混入本项修复。格式与 git diff 检查通过；全量 strict 18 项、归档校验 678 项通过。这些只证明当前树与提案可校验，不代表 C3 已实施。

全部本任务 compiler 停止后，仅清理专用 `target/resume-release`，释放 6810275840 bytes（约 6.34 GiB）。保留 `c3-before` 内三份对照 executable，共约 431 MiB，供第三项配对基准使用；完成该项后再删除。原共享 target 未清理。具体大小、删除目录和保留二进制 SHA-256 见 `build-cleanup.json`。已测 golden executable 随构建缓存清理，源码、固定 golden、编译身份与原始输出均保留。
