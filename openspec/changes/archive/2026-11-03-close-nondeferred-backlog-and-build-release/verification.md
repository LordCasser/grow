# 集成验证与本地release记录

本change只收敛backlog和汇总验证；行为变更分别建立并验证/归档，不修改已发布tag。所有原始125行记录保持在backlog-before.md，文件内相对链接仍按其原始位置openspec/backlog.md解释；没有重写这份原始快照。逐项完整映射见[独立盘点](final-coverage-audit.md)、[算法及恢复闭合记录](algorithm-inventory.md)。

## 当前源码验证

| 边界 | 最终验证 |
| --- | --- |
| Dashboard/单页统一AgentView及CtrlX、fork close/delete/quit | 原交互change完成独立审查及真实PTY；当前最终Pager全lib7553 passed/15既有ignored，minimal102 passed，无失败。 |
| 恢复/持久化/search共享reader | S2搜索增量后Shell全lib4485 passed/9既有ignored；同次ChatState604/3ignored，之后增加同event错误优先级断言最终exact通过。 |
| 历史修复 C2 | sampling-types全320 passed；ChatState全606 passed/3既有ignored；旧JSON、完整report、幂等与边界差分及独立review。 |
| 健康请求副本 C4 | 最终ChatState608 passed/3既有ignored；真实request-time repair→memory ACK、exact retry、两取消点、Goal/memoryrepeat；独立review。 |
| watch候选有界选择 T1 | fsnotify库119 passed/15既有ignored、integration2 passed/6既有ignored、doc0；最后增加反字典序tie两case后exact1 passed；独立review通过。 |
| hunk行表 T2 | hunk-tracker全lib200 passed/1既有ignored，字节及actor内容状态差分、独立review。 |

命令均使用--locked、隔离GROW_HOME和RUST_MIN_STACK=16777216。客户端最终命令为`cargo test --locked -p pager -p pager-minimal --lib -- --test-threads=4`，日志/tmp/grow-backlog-client-final.txt；7553在148.80s、minimal102在0.32s。一次MCP热重载测试>60s提示经独立核查为全局serial排队，真正隔离子进程观察时7s，随后正常ok；没有据此改代码/加timeout。

忽略项是已有显式手工/平台/重负载入口，不把ignored冒充通过。临时诊断/benchmark probes已单独执行后从build移除、源码与raw留各归档；搜索allocator移除后lib.rs字节SHA256恢复。此前Task展示全量的唯一fixture误断言曾失败，已修并定向通过；后续上述Shell全量4485/0failed已验证当前集成，不伪称原失败命令通过。Sampler历史偶发当前未复现、根因未知，只增强失败capture并保留记录，不能称修复flake。

## 活动backlog精简

只保留六项原本明确暂缓：更强跨路由reasoning语义、Remote Control/Agent Host、真实provider cache/native epoch证据、recap共享前缀收益证据、多合法选区连续摘要契约、上游SDK门槛的MCP多轮elicitation。未把待故障注入/待定位当延期；原已完成条目及独立评估关闭候选均有可追溯证据。

## 最终独立审查与release

最终独立静态对抗性审查已通过，未发现剩余集成生产阻塞，见[审查记录](final-review.md)。默认feature本地release已编译并验证，产物和清理证据如下。没有发布、tag或全局安装授权，本轮只构建本地默认feature release。

### 最终静态门禁与构建启动

- `cargo fmt --all -- --check` 首次发现本轮`shell/src/session/actor/suspension.rs`一处参数/条件换行未对齐；只用rustfmt统一该文件，未改逻辑，随后全量fmt check通过。`git diff --check`通过。
- release前OpenSpec全量strict17/17、archived796/796通过。umbrella自身任务11/12仍待最终归档/构建完成，不提前勾选。
- `cargo build --release --locked -p cli --bin grow -j 4`：2026-11-03启动，全新target/release profile；默认features包含jemalloc+sandbox-enforce，没有改用禁用allocator的构建。24GiB/12core本机以4jobs控制编译并发。日志`/tmp/grow-backlog-release-build.txt`。
- `/tmp/grow-release-source-before.json`记录所有本轮变更/新增的127个Cargo/源码文件的最终SHA256集合指纹。构建启动后只做上述格式修正（Shell主crate尚未开始编译），最终产物后再核对源指纹和Cargo freshness。

### 首次release失败与native工具修复

第一次默认构建exit101：tikv-jemalloc-sys的configure报告jemalloc_defs.h/internal_defs.h“unchanged”，文件实际不存在，随后C编译失败。当前PATH优先解析到DevEco OpenHarmony toolchains/diff；用两个临时文件受控验证，该diff对不同文件和missing文件都exit0，而/usr/bin/diff分别exit1/2。并非未证实的“stale cache”猜测，也没有改第三方源/手工伪造头文件/禁用jemalloc。

仅对本次命令把native工具放到PATH前（/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin），未改用户全局环境。执行`cargo clean -p tikv-jemalloc-sys --profile release`清理该依赖生成物后，用相同默认features、--release/--locked/-j4重建。第一次日志/tmp/grow-backlog-release-build.txt；重建日志/tmp/grow-backlog-release-build-native.txt。后续只把实际成功构建当release完成。

### 已完成release产物验证

重建成功：`Finished release profile [optimized] in3m04s`，exit0；同一默认feature命令再次执行0.78s完成，无新编译，确认最终源码freshness。全量fmt check和git diff --check通过。

- 产物：`/Users/lordcasser/workspace/projects/grow/target/release/grow`，Mach-O arm64，150,303,120 bytes。
- `GROW_HOME=<fresh mktemp> target/release/grow --version`：`grow 2.5.5 (bbd2c4dfa)`，exit0；同home `--help`正常输出Grow TUI帮助，exit0。
- `codesign --verify --strict --verbose=2 target/release/grow`：valid on disk / satisfies Designated Requirement。
- SHA256：`a7db05f60a0c6c9c8973afb275d19eca225a8efaa19f9c5c9929c62809a24423`。
- 最终127个本轮修改/新增Cargo与源码文件的集合指纹`b82a9ceb3a05cc254690d2a6c7352ac66103be92be84f2febbc5161f2df6fa31`与构建前最终记录一致；初次启动后仅修一处格式，Shell编译前已完成，最终Cargo freshness已重核。

这是当前dirty工作树的本地release，沿用workspace2.5.5，不是新tag或发布；没有安装/覆盖全局grow。性能probe数据仍是各自debug内核测量，不能拿release编译成功替代真实provider/生产负载验证。

### 清理与最终独立补核

`cargo clean --profile dev`仅选择本轮11个包(shell/pager/pager-minimal/chat-state/sampling-types/sampler/agent/mcp/fsnotify/hunk-tracker/workspace)，删除67,198个生成文件、132.2GiB；debug incremental67G→17G，release目录6.6G保留。日志/tmp/grow-backlog-debug-clean.txt；清理后release SHA256未变。

独立审查者实测产物arch/size/hash/signature、临时隔离home运行version/help，独立重算127文件source指纹并核对成功构建/freshness/清理记录，最终补核通过，无剩余阻塞；见final-review.md。没有把implementation owner的测试执行误写成reviewer重跑全库。

### 归档收尾

所有任务完成后，umbrella归档前strict全量17/17通过；`openspec archive close-nondeferred-backlog-and-build-release --yes`完成，无未勾任务。归档后strict全量16/16、archived797/797通过。新11月3日归档与live backlog所有本地Markdown目标检查通过；原backlog-before.md仍保持原始SHA256，未为路径迁移改写，原始相对链接按openspec/backlog.md位置解释。清理后再次核对release SHA256不变。最终没有新增待处理的非延期backlog项。
