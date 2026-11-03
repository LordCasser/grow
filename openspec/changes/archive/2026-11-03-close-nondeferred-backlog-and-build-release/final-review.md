# 最终独立对抗性审查

审查者未参与本轮生产实现。结论：**当前集成源码静态审查未发现可行动的未解决生产阻塞；本地默认feature release实物、源码指纹及清理结果也已独立补核通过。** 活动backlog精简已核对，本审查无剩余阻塞。umbrella最终OpenSpec校验/归档与任务勾选仍由owner完成；下文保留静态审查时点的证据边界，文末release补核更新其待办状态。

## 范围与证据边界

依据用户授权的非明确暂缓backlog范围、主规范、本轮各独立change的delta/design及真实调用方审查当前dirty集成。参考全局起点`/tmp/grow-backlog-start.diff`和各增量verification指向的source snapshots，不把HEAD之上的Dashboard/runtime前序修复全算成本轮新增。ChatState mutations/request_builder/compaction_utils/timeline、sampling-types conversation和search在全局起点无预先diff，相关局部diff用于核对本轮改动；前序Dashboard行为以已完成的独立UX审查及当前owner路由复核。

本阶段没有修改生产/测试代码、运行Cargo、访问用户真实session、发布或安装命令。各归档review/verification用于交叉证据，不能替代当前源码审查；下列动态结果由实施/验证owner执行，本审查者没有重新运行全库或PTY。早先UX阶段曾独立重跑删除/prefix/search/child定向测试并闭环两个modal问题；本阶段未将那些旧二进制结果冒充最终集成验证。

## 实际源码审查

- **Dashboard/普通详情与生命周期**：当前AgentView仍独占普通/attached/child的编辑和局部interaction；overview只读preview，不接收回复或回答。独立search_query与新建composer保留各自草稿/附件/undo，异步paste completion仍归DashboardDispatch。单一SessionCommands使用当前可见owner；child Stop不走parent Close/Delete。导航清理root及可见child前缀，cycle独立于filter/group/pin。重复prefix仅Stop，C Close经runtime ACK，D/Delete经AppView目标绑定确认；闲忙切换不能授权另一后果。Close/Delete回执继续核对原AgentId/session/epoch，Stay退休匹配owner而不夺取其他页。全局Quit与runtime scope保留独立后果。先前独立审查发现的删除modal Resize更新及minimal退出notice优先级，当前实现均保留修复。
- **恢复依赖/同进程Retry**：`load_session`在生命周期guard下处理failed_suspended_restore，退休函数核对同cmd channel与Suspended phase；已有Restoring/prepared/运行owner不会被当作临时失败候选撤销。退休ACK后退出resident但thread仍有owner，取消drain/迟到ACK不会把它变成逻辑child Teardown或允许第二writer。aggregate runtime_source_hash目前只作诊断，实际目录能力、正式selector/plugin、checkpoint route/effort、Workflow frozen源、Hook未知结果及出生ceiling仍独立检查。
- **规则与MCP出生scope**：Existing恢复从有效typed ProjectInstructions取模型/classifier规则，缺规则不补今天磁盘文本；新Seeded初始化与既有恢复分开。共享permission manager的child不改root classifier。MCP在Spawn前用一个冻结的live client-binding样本求parent ceiling与durable服务器名；profile过滤后才采样，actor setup不重采样，后续调用继续与live上游incarnation求交。Cold仅按持久出生成员求当前可用子集，missing事实故意fail closed，不把已知empty与missing合并，也没有引入猜测迁移。
- **Task继承展示与Plan**：resume_from模型Surface与presentation来自同一validated source cut，Resumed保留源head从而维持Seed位置映射；初始化在staging写Seed/来源引用/继承facts后才发布。原response/cursor/ingress/current-control身份不进入新执行owner，隐藏的旧正文只作继承展示。Plan恢复持有一个captured BehaviorSnapshot和artifact正文；异步读失败只按exact snapshot归一化，stale pending位经既有durable Control边界更新，正文hash和snapshot在开interaction前复查。Malformed结束该attempt并保留candidate供显式恢复，不自动循环，决定仍按原snapshot/receipt CAS。
- **S2 read cache与replay**：worker只持有最近一次成功projection；每次新reader仍固定entity/frontier，认证旧complete_len全字节hash，不相信mtime/size。torn fragment不进checkpoint。共享range scanner保留原大小/budget/version-first/checksum/物理域序号/anchor/identity校验；所有suffix物理解码后才进入消费owned Timeline的原replay顺序，content-before-lifecycle未被live accept替代。失败candidate或SQLite失败不回存，旧doc保留；key miss先释放旧fold，完整正文提取仍在blocking边界。Cold bootstrap仍走原完整读取，未产生第二canonical authority。
- **C1/C2/C3/C4交界**：C1保存offset同时保留完整saved SurfaceIds、route/goal/description模式与Weak图像内容身份检查；repair/rewind导致坐标变化时不会继承旧选择。C2所有quarantine evidence在编辑前从原slice产生，retain/merge/delete均按原坐标，保留边界/顺序/计数及reasoning隔离。空repair报告仍意味着完整candidate值未变。C4因而仅复用健康owned copy；真正repair须等durable ACK后从已提交Surface取值，MemoryContext同一typed item也在ACK后进入request。Actor串行所有权避免两次ACK间另一个命令变更Surface；Goal/native/image继续是既定顺序的request投影。C3只在共同validated apply更新rewind anchor，prepared/失败不改变key。
- **T1/T2/T3**：T1深度+原ordinal共同确定heap优先级；ordinal唯一，path词典序不影响equal-depth来源顺序。未overflow保持原稳定sort，overflow仍完整遍历计数，budget0返回空集且总数准确；ignore/symlink/root边界未另改。T2只在同一次文件查询共享两侧借用行表，原单hunk formatter和缺Full文本的空侧/None语义保留。T3普通项复用lstat，symlink仍follow取目标metadata，原lstat失败fallback不丢失。

没有将现有Summary与ledger跨次读取边界、单entry长期内存、synthetic小K退化等已披露限制包装成新缺陷；也没有把这些限制省略。S2仍O(全prefix bytes)+完整正文投影、key交错会cold且idle保留最后Timeline/AppendState；C2存在小K成本/临时空间交换；C4没有证明所有含memory ACK请求的峰值内存下降。性能数字只沿各归档已说明的kernel/边界解释，不外推provider或整App提速。

## 覆盖与活动backlog

[阶段逐条盘点](final-coverage-audit.md)保留原125行中的新增事项、算法、21项已闭合债务和22项历史记录映射。该阶段尚未完成的C2/C4/T1现均有独立change/review/verification（T1已归档796），不再作为隐含延期。

实际读取当前`openspec/backlog.md`并用脚本逐字比较：只有六个bullet，且内容及顺序与frozen `backlog-before.md`对应六条**完全相同**。没有扩大延期集合。六项为reasoning更强语义、Remote Control/Agent Host、provider cache/native epoch真实路由证据、recap共享前缀收益、多合法选区摘要新契约、上游SDK门槛的MCP多轮elicitation。Sampler已移出延期，保留归档中的“当前未复现、历史根因未知、只增强失败capture”；本审查不将它称为flake根因修复。

原始快照当前SHA256：`dc2c785faa69b39803b50e667f23b8a739357fcfbadca74ffd3593fce6e3b81e`。没有改动原快照以方便清单闭合。

## 已有验证与本审查未运行部分

owner记录的最终相关结果为Pager7553/15ignored、minimal102；Shell4485/9ignored；C2后sampling-types320、ChatState606；C4最终ChatState608/3ignored；T1 fsnotify119/15ignored、integration2/6ignored及最终tie exact；T2 hunk-tracker200/1ignored。均保留原日志、失败修正及临时probe清理边界，不把ignored计作通过。最新客户端MCP热重载曾显示>60s，独立检查发现libtest包含serial锁排队，真正隔离child观察时仅7s，随后正常ok，未据此加timeout或改实现。

这些动态结果来自相关owner的实际运行，本审查者只核对记录和相应测试/调用方，没有重新跑全量、真实provider或跨平台release。当前静态范围未发现新增可行动阻塞；**本地默认feature release尚待实际编译、执行身份检查、产物hash及清理证据，最终OpenSpec/任务收尾也待补核**。完成后追加本记录，不提前声称整个umbrella已完成。


## Release实物补核（2026-11-03）

本节只补核实际产物和验证记录，不重跑Cargo或扩大源码审查。已读取`/tmp/grow-backlog-release-build-native.txt`：默认feature依赖`tikv-jemalloc-sys`、`tikv-jemallocator`及cli实际编译，最终`Finished release profile`用时3m04s；读取`/tmp/grow-backlog-release-freshness.txt`：0.78s完成、没有`Compiling`行。构建owner记录的命令为`PATH=/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin:$PATH cargo build --release --locked -p cli --bin grow -j4`，仅本命令调整工具查找顺序，没有禁用默认feature。

首次日志也已核对：configure将不存在的`jemalloc_defs.h`和`jemalloc_internal_defs.h`称为unchanged，随后报missing header；这次失败不计作release通过。DevEco diff与系统diff的受控exit差异、只清理该依赖release生成物后重建，来自owner的诊断记录，本审查者没有重复执行clean或工具故障注入。成功日志与最终产物为本节通过依据。

独立对当前`target/release/grow`执行`file`、`stat`、`shasum -a 256`和`codesign --verify --strict`，确认：

- Mach-O 64-bit executable arm64；150303120 bytes。
- SHA256 `a7db05f60a0c6c9c8973afb275d19eca225a8efaa19f9c5c9929c62809a24423`，与owner记录一致。
- codesign strict验证exit0。
- 在独立临时`GROW_HOME`运行真实`--version`和`--help`均exit0；版本为`grow 2.5.5 (bbd2c4dfa)`，help输出Grow TUI与完整usage。临时目录在检查后已删除，未访问用户session。

使用owner给出的原算法独立重算：从`git diff --name-only -z`与`git ls-files --others --exclude-standard -z`收集`crates`、`Cargo.toml`、`Cargo.lock`、`.cargo`范围的路径，排序后逐一hash `path + NUL + file bytes`（删除为`<deleted>`）。当前127条路径与JSON逐项顺序相同，集合及内容SHA256为`b82a9ceb3a05cc254690d2a6c7352ac66103be92be84f2febbc5161f2df6fa31`，与`/tmp/grow-release-source-before.json`最终值完全一致。JSON明确记录启动后唯一format-only文件`shell/src/session/actor/suspension.rs`；Shell在成功日志中随后实际编译，最终freshness也没有新编译。

读取`/tmp/grow-backlog-debug-clean.txt`确认清理移除67198文件、132.2GiB；实际`du`核到`target/debug/incremental`17G、`target/release`6.6G。清理后再算release产物hash仍为上述`a7db05...`，未清除或替换实物。本审查者没有运行Cargo或clean。

本地默认feature release实物、源码对应关系与本轮缓存清理已通过独立补核。没有更改版本、tag、发布或全局安装。此前静态结论保持：当前集成无未解决生产阻塞，本审查已闭环；umbrella最终任务/OpenSpec校验与归档由owner继续收尾，不把尚未完成的归档提前记作通过。

## 主代理归档收尾记录

上述独立补核后，umbrella已完成归档，任务全部勾选；主代理执行全量strict16/16、archived797/797与本地链接/快照hash检查通过。此段为主代理的实际收尾证据，不声称独立审查者重新运行Cargo或归档命令。
