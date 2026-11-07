# 2.6.2 候选验证记录

## 范围与身份

本轮只完成用户收拢后的在途及发布必要修复，不启动未实施的卸载/普通更新保存错误、Markdown 性能、Workflow 测试隔离或 Agent 公共 API 清理。ACP 容量实验停在准备，未写源码或生产政策。版本准备不包含提交全部 dirty、共享 tag、push 或发布 Release。

根 workspace 版本已从 2.6.1 更新为 2.6.2，33 个继承版本的本地包同步；其余独立版本包保持。999 条外部 name/version/source/checksum 记录更新前后完全相同，结构化指纹 `d3a5b748c0463c128c91f4b916560ef0214f2e81f02217a3cbaf184eae8e0845`。`cargo +1.93.1 metadata --locked --offline --filter-platform aarch64-apple-darwin --format-version 1` exit0。第一次无平台限制 metadata 因离线缓存缺少未使用的 Android 包而失败，随后当前 macOS 平台验证成功，不以此声称全平台构建。

候选保留原有大量未提交工作，未把旧 HEAD 当作这些修复的已提交来源。最终2369个Rust/manifest/lock文件的逐路径SHA清单为 `/tmp/grow-262-final-source-manifest.json`，SHA256 `86a0a74807a9de3e9d9da38d3f8a60d884fcda70794a03c77cebc21191701fba`；基线HEAD `8ab486a6e45870561b91dba872934a6e24a795f1`。默认二进制构建后只有两项测试观察修正及文档变化，不影响生产代码；二进制SHA再次核对保持下列值。最终工作区有272个dirty路径，属于用户原工作与本轮候选的合并现场，不自动提交或清理。

## 已有验收证据

- 长历史退出：完整现场 76,713 个 domain events 的真实 owner 旧路径 25,050ms，五秒期限过期；新窄查询 3.916ms，runtime 与 191 组子任务事实相同。合法复杂历史 305,653,152 bytes 的真实 ACP 退出由无采样旧红 5,002ms unconfirmed 转为 123ms suspended；actor/writer 实际退役，原始前缀 SHA 和 cold 完整验证通过。默认 CLI 双 Ctrl+D 与恢复已由最终产物完成，见下节。详见 `../2026-11-07-avoid-exit-history-replay/verification-shell-consumer.md`。
- AC-07 真实 ACP 更新结果采用已验收归档：checkout 与 registry commit/plugin map 一致，第二次 tip no-op，原选择/pinned/local/reload 语义保持。见 `../2026-11-07-adopt-acp-plugin-update-results/verification.md`。
- PB-03 Session→HTTP/WS→CLI 已验收归档：真实 CLI 10、library 30、registry 2 项通过，Windows lib/tests cross-check exit0；Windows 未原生运行。见 `../2026-11-07-settle-pty-session-lifecycle/verification-service.md`。
- PB-06 raw 地址保真和明确不可自动符号化已验收归档：实际误归因旧红转绿，不同 ASLR slide 对照通过，最终全 crate 31 unit、9 integration、3 ownership、1 doc 通过。见 `../2026-11-07-avoid-cross-process-crash-symbols/verification.md`。
- announcements 依赖收缩已验收归档，13 项既有测试通过，不改变状态行为。

## 最终默认产物与真实退出

最终默认 CLI check/build task `01a1b4e5-2894-7420-9fec-433d7c48cade` exit0，SHA256 `9f7585d55f2a387bbebfd68cd16dbe59c5dfac0658df6fe8372a346fb8551472`，输出 `grow 2.6.2 (8ab486a6e) [alpha]`。二进制构建基线之外仍有本轮 dirty 源码，不能将 HEAD 号当作修复已提交证据。

父以此产物连续两轮真实 PTY resume→input ready 即双 Ctrl+D，task `01a1b4e7-b510-7ba2-a1c1-ebe3b468cee2` exit0，实际 wait0 为236.87/240.73ms、runtime125/107ms、两轮EOF true、原305.7MB prefix哈希不变，canonical checkpoint/tree与冷恢复关系均核对。见 `../2026-11-07-avoid-exit-history-replay/verification-final-cli.md`。较早OS watcher慢轮仍单列backlog，未作为这两轮的删除依据。

新证据：图片清理8+取消注册2项实际10/10；shared discovery watcher完整22/22；in-process Skills9+相邻MCP2实际11/11；SDK完整lifecycle1/1；CLI PB05恢复循环2/2。相应原始日志均位于 `/tmp/grow-262-*-final*.log`，各功能 change 保留具体路径和失败分析。真实 leader 配置文件、slash 与模型目录/custom roots全链亦已1/1通过（`/tmp/grow-262-leader-causal-final.log`，28.46s），全部已实施功能均完成验收及规范归档。最终格式检查 `/tmp/grow-262-final-format.log` exit0；workspace all-targets task `01a1b4f4-f9b4-73c2-bda9-3d4112fbd528` exit0，453.47s，原始 `/tmp/grow-262-all-targets.log`。诊断包括既有测试的未使用字段、未使用赋值、irrefutable if-let；未混入无关 warning 修复。

## 最终门禁结果

固定 Rust 1.93.1、locked/offline、JOBS2、INCREMENTAL0、STACK16MiB。默认 CLI check/build、workspace all-targets和最终格式均通过。隔离新HOME的 `--version`、`--help` 与 `agent leader --help` smoke和最终fmt task `01a1b512-bee3-7d51-b3dd-0cf47aab9819` exit0；实际输出仍为 `grow 2.6.2 (8ab486a6e) [alpha]`。真实leader与长历史恢复/退出smoke另见上列实际产品证据。

开发指南的八个核心库已完整执行。首轮两个fixture失败保留：minimal误判OSC8 target、cold authority fixture在root control handle发布前做就绪断言。只修测试观察后分别完整重跑minimal102/102、Shell4786通过/13既有ignored/0失败；其余六库完整通过，详情及feature区别见 [核心验证](verification-core.md)。未将首次批次exit101称为一次全绿。

分发配置实际检查 `cargo +1.93.1 check -p cli --bin grow --profile release-dist --features release-dist --locked --offline` task `01a1b510-c778-7f02-9b39-2ddb57597886` **exit0**，106.03s，日志 `/tmp/grow-262-release-dist-check.log`。这是分发profile与feature的编译检查，本轮没有生成或签名release-dist安装包。实际执行验证使用上列默认dev二进制，正式分发构建和其他平台native tests仍须官方workflow。

## 最终归档与交付状态

所有本轮实施项、范围收拢和发布准备均已归档。最后执行 `openspec validate --all --strict --no-interactive` **21/21 passed**，`openspec validate --archived --strict --no-interactive` **922/922 passed**，原始 `/tmp/grow-262-final-spec-validation.log` 与 `/tmp/grow-262-final-archive-validation.log`。`git diff --check` 通过；对本轮62份归档与入口Markdown的相对链接目标检查，missing targets为0。最后重新核对默认binary和source manifest的SHA，与本记录一致。没有活跃子任务或未审交付。

发布说明已定稿于 `crates/codegen/shell/changelogs/2.6.2.md`，索引已同步。归档保留旧失败、修正理由、真实验证与平台限制；本轮本地准备完成。

正式发布还需对最终提交执行官方同 SHA 发布工作流；本记录只准备本地候选，不代替其他平台的原生执行证据。未提交全部dirty、未push/tag、未创建Release。
