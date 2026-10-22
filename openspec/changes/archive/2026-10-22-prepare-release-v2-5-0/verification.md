# Grow 2.5.0 本机发布准备验证

用户要求全部当前修改纳入 2.5.0，并先提交、确认工作区干净，再测试。初始完整快照是 `567560b82f5e7d2f1420877f4364bf3ebaaefbcf`；后续夹具修正及同时完成的 `/btw` 图片描述 owner 变更逐项提交后重测。签名候选的产品代码提交为 `8c469e1af3e6f8a3daf6dce624e423fc243b82ff`；最终测试夹具提交为 `e4e969eddba3738dfc76f9783ef0596046e4f6cd`。两者之间只修改 tests、dev-only test-support、发行说明和验证记录，normal dependency closure 核对确认未改变产品编译输入（见 evidence/candidate-source-review.json）。后续仅归档与清理。

所有 Cargo、CLI 和回归命令使用独立 HOME `/private/tmp/gr25-9_pzf2cq/home`、GROW_HOME `/private/tmp/gr25-9_pzf2cq/grow`，各 TestSandbox 另有独立子目录；无个人模型配置、继承的应用 API key 或付费 provider 请求。显式复用 CARGO_HOME/RUSTUP_HOME，只用于工具链/源码缓存。固定 Rust/Cargo 1.93.1，macOS arm64；专用 `target/release-readiness-2-5-0`，CARGO_INCREMENTAL=0。核心测试保留 debug assertions，真实 CLI/PTY 指向本轮 release-dist 候选。

核心 15 library 的最终提交回归：18,369 passed、0 failed、36 ignored。按每个 library 最终 parent libtest summary 计数，不重复计算 tools/leader 的 nested test subprocess。/btw owner/ACK/cancel/failure 和三后端描述选择包含在本轮全量测试内。

完整结果、命令、提交号、耗时及失败原始日志见 evidence/results.json 与 evidence/logs；结果按测试族分别报告，不将重跑或不同特征图的相同用例叠加成独立覆盖。

对抗性审查核对：未放宽 Timeline/schema/hash/identity/ACK/历史排序或回复分类；旧 updates fixture 改由严格当前 writer 创建，legacy oracle 仅用于显式转换前比较；基本假持久化 sink 模拟成功 append 后 presentation delivery，其余真实 writer 与故障注入夹具不变。DoomLoop 的 retractable collector 与三次 total-attempt 预算只声明测试调用方的真实能力。Leader readiness 使用既有无连接探针，真实断开后明确断言 queue cleanup 和会话 eviction，保留消息不泄漏断言。tmux near-deadline 夹具改显式 /bin/sh 执行相同脚本，保持 1.2s delay、1.5s deadline、管道和成功输出断言，首次临时脚本执行开销及后续 debt 见 initial-findings.md。

范围限制：本机结果不能替代最终发行 SHA 的 Linux/Windows/其他架构 CI、托管发布门禁与匹配 annotated tag；未创建 tag、push 或公开发行。Apple clang ASan/UBSan 运行成功，显式 detect_leaks=0；本机未运行 LeakSanitizer，仍需 Linux CI 的 detect_leaks=1 门禁。PTY/单元校验覆盖图片身份、顺序、布局与 OSC52 复制协议，未改写用户系统剪贴板；不能替代真实终端 Kitty/iTerm2 解码器的像素级验收。旧 schema 明确拒绝，不自动迁移，版本说明已记录该边界。此次只做 release correctness 验收，不把历史不同工具链的基准或一次 PTY 结果当作新的加速保证。

同一目标重复编译仅在本机临时编排中消除：evidence/run_exact_reusing_compile.py 对原 runner 的 compile 阶段作受控复用。cache miss 仍完整编译；命中要求同一 clean commit、完整 Cargo argv、完整进程环境摘要与原 executable SHA，保留原 package cwd。原 runner 的唯一名称枚举、精确执行、非 ignored 的 (1,0,0) 结果、超时和进程树清理均不变，独立只读审查确认。该 cache 不额外摘要 CARGO_HOME 外部配置，适用边界只限本次固定离线验证，不能代替官方 CI 的编译门禁；没有修改仓库 runner/build script。results.json 保留原命令与实际 wrapper 命令。

| 验证族 | 最终结果 | 说明 |
| --- | --- | --- |
| 15 个核心 library | 18,369 passed / 36 ignored | 原始结果按顶层 summary，排除 nested subprocess |
| CLI unit | 35 passed | 包含关闭/teardown 边界 |
| Shell 32 integration targets | 143 passed / 71 ignored | ignored 不计通过；另行显式执行关键场景 |
| Pager 16 integration targets | 249 passed / 244 ignored | 其中 Settings 235 项全部通过 |
| runtime projection / model skill / MCP pagination | 9 / 6 / 5 passed | 现有周边功能专项 |
| 52 个显式 exact selections | 全部 passed / 0 ignored | CI 清单 30 + 额外 18 + manual feature-gated 4 |
| TestSupport 新入口 | 71 passed | dev-only，计数与核心覆盖有重叠，不相加 |
| Python 工具及发布契约 | 34 项，33 passed / 1 skipped | 平台跳过保留，不报告为通过 |
| 默认 CLI + workspace all-targets | passed | Rust 1.93.1，无新增编译警告 |
| 原生向量 C 边界 / ASan+UBSan | passed | -Wall -Wextra -Werror；未运行 LSan |
| OpenSpec strict / archived | 17 / 692 passed | 归档本 change 前；归档后另记结果 |

52 项包含协议失败后的 resident/cold/重启恢复、Goal 预算、Minimal transcript/计划/缩放/取消/退出、队列取消与草稿、工具标题复制、Leader 共享与重连、父子孙挂起/并发 claim、Skill 快照、计划审批恢复、完整 stdio 生命周期、EOF、headless 后台任务清理、图片 chip/预览、continue、现代 loading Ctrl+D、分离父子导出、孤儿子会话、工具 dispatch duration、46 类 wire DTO golden 和 DoomLoop header。具体名称、命令、提交、原始日志见 evidence/coverage.json，未将未执行的 ignored、外部凭证场景、soak/RSS bench 或实际终端图像解码检查算成覆盖。

现代 release PTY 的正常输出实际覆盖 128/512/2048 turns，慢输出覆盖 512/2048；5 个报告、原始 cast/phase JSONL 与图片预览前后屏幕文本在 evidence/trace-index.json、pty-traces、image-artifacts。曾把 128 用于慢 loading 探针，但末条历史已在输入探针前可见，前置条件不成立，故保留失败记录并改用长样本；没有放宽 loading/assertion，也没有把该失败解释为 SessionLoaded 已发生。这里只作当前构建正确性验收，不作优化前后统计比较。

候选：`target/local-release-2.5.0/grow`，`grow 2.5.0 (8c469e1)`，100,316,320 bytes；SHA-256 `6c5ed5cf88d07d636156d4466383da3a383d6fc731de7825958252628c5871ca`。aarch64 Mach-O，strip 后 ad-hoc 签名，回归结束再次 codesign --verify --strict 及摘要核对通过；otool 仅列 Apple 系统 frameworks/dylibs，不依赖 Homebrew 动态库。内置官方固定 ripgrep 15.0.0，下载包和 executable 摘要见 evidence/binary.json。

结论限定：所选本机回归未发现新的产品代码回归；本轮修复了架构说明的 schema guard、已退休 updates/模型目录假设、假持久化 delivery、通知顺序、mock 前后台请求匹配和启动选择等过时夹具。具体失败、原因、变更断言和后续通过见 initial-findings.md；原始失败日志均保留。没有通过退回旧存储、放宽严格 reader 或提前开放输入来换取通过。

归档与清理：本 change 以 skip_specs 归档，archive 时唯一未完成项是归档/清理/最终记录提交本身（4/5）；核心行为验证已完成。专用 Cargo target 已删除，清理前 allocated size 为 24471441408 bytes（约 22.79 GiB），保留候选与 manifest，未清理共享 target/debug、target/release-dist。五个 PTY 用例显式启动的 no-exit-on-disconnect Leader 在测试结束后仍驻留，按专用 binary、隔离 GROW_HOME 和启动时间确认归属后 SIGTERM 清理，最终剩余 0；这是测试清理 owner 的后续债务，未更改产品 Leader 常驻语义。见 evidence/cleanup.json、release-process-audit.json。独立只读审查核对 52 exact 的实际通过日志、manifest 全部 SHA、5 条现代 PTY 报告和候选/fixture 边界均一致。不同子集按各自记录的 clean fixture commit 执行，并非宣称全部在 e4 重跑。

归档后校验：16 项当前主规范 strict、693 项 archive 均通过。删除专用编译树后再次运行候选 --version 成功，摘要未变；五个测试 Leader 均通过 SIGTERM 退出，不需要 SIGKILL。最终提交仅归档、验证证据和 backlog 记录，工作区状态在提交后再次核对。
