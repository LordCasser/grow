# 阶段复验（2026-09-07）

本次在独立工作树 `grow-openspec-sdd`、分支 `codex/openspec-sdd` 验证，未修改运行时代码。

## 结果

- `openspec validate --all --strict --no-interactive`：15 项通过（14 个主规范、1 个进行中 change）。
- `openspec validate --archived --no-interactive`：2 个归档通过。
- `git diff --check`：通过。
- 重新枚举 crates 和 third_party 的 Cargo.toml：61 个 package，与 inventory 完全一致。
- 已审阅 12 个包的 41 份文件 SHA-256 与当前源码一致；32 项功能记录均有对应 delta requirement，来源文件和所列符号文本存在。此检查不能替代语义审阅。
- 130 个本地 Markdown 文件链接可解析；其中 29 个使用本机绝对路径（25 个来自冻结历史副本，含行号），依赖本机目录，不具备跨机器可移植性。历史副本保持原样，不作为当前契约证据。
- AGENTS.md、开发指南、OpenSpec 索引和 CI 一致要求使用 change 管理开发过程，验证后归档；docs 保留开发者说明。

## 已有测试证据

本独立工作树此前执行 `cargo test --locked -p paths -p prompt-queue -p token-estimation -p sqlite-journal -p version --lib`，本次复核完成日志：paths 21、prompt-queue 16、token-estimation 15、sqlite-journal 19、version 1，共 72 项通过、0 失败。本次未重复执行，未据此声称其他包或平台通过。原共享工作树上的测试不计入本记录。

## 未完成与验收边界

目前只有 12/61 个包完成逐包审阅，49 个仍 pending。32 项新增要求仍在本 change 中，尚未归档到主规范。14 个主规范仅是核心行为基线，不能声称覆盖全仓库全部功能。

因此本次结论是结构和已登记证据一致性通过，全量功能覆盖验收未通过。tasks 中剩余项目保持未勾选，本 change 不归档，总目标不标记完成。

## tool-runtime 补充验证

完成该包全部 src 和 tests 遍历后，进度为 13/61 包、45 项 delta 要求。`cargo test --locked -p tool-runtime` 在独立分支退出 0：114 项单元/集成测试及 2 项 doctest 通过，1 项 doctest 忽略。独立真实函数调用复现 UTF-8 tick 边界差异，详见包 review；此发现不被通过的测试掩盖。尚余 48 包，前面的 12/61 为此前阶段快照。

## 并行主分支整合边界

另一任务同步称 main 已归档 audit-goal-settlement-retry 和 fix-update-probe-process-lifetime，并在处理 fix-idle-controls-and-resume，可能影响 client-surfaces、behavior-goal、session-timeline。此为协作消息，尚未在本分支检查这些实现或测试，不作为当前证据。本文档工作树保持独立；后续合并必须检查实际主分支源码、归档及同名规范冲突，不能覆盖另一任务新增要求。

## tool-types 补充验证

已完整遍历 7 个模块与 manifest，进度为 14/61 包、58 项 delta 要求，尚余 47 包。`cargo test --locked -p tool-types --all-features` 退出 0，102 项测试通过、0 失败；prompt-render 条件渲染测试实际执行。新增条目均有源码符号证据，所有已审阅文件哈希仍匹配，change 严格校验和 diff whitespace 检查通过。未归档，完整性验收仍未满足。

## compaction 补充验证

已完整读取 14 个 Rust 文件、manifest 与 include_str 嵌入模板，进度 15/61 包、67 项 delta 要求，尚余 46 包。`cargo test --locked -p compaction` 退出 0，56 项通过、0 失败。测试覆盖裁剪预算/UTF-8、重试分类、摘要清洗和提醒输出；不证明实际 provider 超时、host 持久化或 trigger 正确，后者继续在所属 crate 核查。模板也登记 SHA-256，不仅登记 Rust 文件。

## shell-base 补充验证

已遍历全部 10 个 Rust 文件及 manifest，进度 16/61 包、78 项 delta 要求，尚余 45 包。`cargo test --locked -p shell-base --all-features --lib` 在独立 macOS 工作树退出 0，63 项通过。CPU profile 使用 fake engine 的生命周期测试通过，不代表真实 pprof 采样或 Windows API 已验证；实际运行平台边界见该包 review。已审阅证据哈希与全部 feature-map 符号检查无差异，严格格式校验通过。下一包 tty-utils 仅完成文件枚举、manifest 和 lib.rs 开头阅读，保持 pending。

主分支任务追加同步：fix-idle-controls-and-resume 已归档，称新增 behavior-goal 的 Idle control completion refreshes Behavior availability 与 session-timeline 的 Control context identity survives activation and replay，并更新两篇架构说明。此消息尚未在独立分支复验；后续整合保留实际新增要求与归档，检查源码差异，不将其所报 494 项测试计入本分支验证。

## tty-utils 补充验证

3 个模块全部遍历后进度 17/61 包、88 项 delta 要求，尚余 44 包。`cargo test --locked -p tty-utils` 退出 0：29 项主测试、2 项编译 doctest 通过；主测试内另 re-exec 运行一次 stderr body，不重复计作额外功能覆盖。Linux 与 Windows 路径未在其平台运行。来源文件哈希与符号映射复核通过，严格格式验证通过，完整性目标仍未完成。

## 当前阶段独立复验

在 `codex/openspec-sdd`、HEAD `1e1fda6d2f7b3990d2c0a34d0c0015b9b1edbb5c` 再次执行验证：

- 全量严格校验 15 项通过，归档校验 2 项通过，`git diff --check` 通过。
- 从磁盘重新解析全部 package manifest，61 个包的路径、名称和 feature 与清单一致。17 个 reviewed、44 个 pending，与 tasks 勾选状态一致。
- 17 个包的 99 份证据文件 SHA-256 全部匹配；已审阅包的全部 Rust 文件和 manifest 均登记。此检查不代替源码语义审阅。
- 88 项功能映射与 16 个 capability delta 的 requirement、正文、177 个场景一致；213 个来源引用的文件及符号文本存在，无重复要求、缺失映射或额外 delta 标题。
- 检查 OpenSpec 全部 Markdown 及项目/开发者入口的 229 个本地文件链接，均可解析；29 个绝对路径仍有跨机器可移植性限制。本次仅检查目标文件存在，未验证 Markdown 锚点定位。
- 复核已有独立工作树测试日志，结果与上述各包记录一致；没有重跑测试，也没有把其他工作树的结果计入。AGENTS、开发指南、OpenSpec 索引与 CI 的流程约束一致。

结论：当前结构与已登记证据的一致性检查通过，全仓库功能完整性验收仍不通过。44 个包待完成，88 项增量尚未归档；继续保持 change 和总目标未完成。mermaid 的部分阅读不计入已完成包。

主分支任务新增协作报告：已归档 `2026-09-07-fix-update-completion-process-lifetime`，新增 client-surfaces 的 Bounded post-update completion generation，称直接子进程具有 10 秒上限与取消清理，131 项 update 测试通过。尚未在本文档工作树检查其实现和测试；后续整合须保留实际规范与归档，不以本段报告代替验证。

## mermaid 补充验证

完成 6 个 src 模块、1 个集成测试文件、manifest 和字体资源核对，进度为 18/61 包、98 项 delta 要求、17 个 capability，尚余 43 包。`cargo test --locked -p mermaid` 在本独立工作树退出 0：57 个单元测试、7 个集成测试、1 个 doctest 通过。真实 mmdc/Chromium 和 Windows 清理未执行；CJK 测试允许无覆盖字体时提前返回，不扩大其保证。

全部已登记源码哈希与 feature-map 来源符号复核通过；10 项新要求区分 checked 调用和直接引擎调用、首选字体与系统 fallback、像素上限与全流程资源上限、group kill 与真正 wait。子进程误用与预算边界登记 backlog，不混入运行时代码修改。

主分支任务另同步 `2026-09-07-fix-update-cleanup-unknown-age` 已归档，新增 client-surfaces 的 Download cleanup requires known stale age，称修复未来 mtime 被误删并通过 132 项 update 测试。还有多平台旧版本按文件保留的 backlog。均仅为协作报告，后续整合须检查并保留实际文档，不计作本分支验证。

## ptyctl-cli 补充验证

6 个源码文件与 manifest 完整遍历，新增 8 项要求，进度 19/61 包、106 项 delta 要求、18 个 capability，尚余 42 包。默认独立构建退出 101（reqwest/query 缺失）；附加 `--features reqwest/query` 构建退出 0，8 项 mock HTTP/CLI 检查通过并复现已记录差异。本包无内置测试，不将 mock 检查称为完整 PTY 验证，真实控制库仍 pending。

主分支任务同步 `2026-09-07-fix-update-version-group-retention` 已归档，新增 Download retention groups platform artifacts by version，称保留当前与最高其他版本的全部平台产物并通过 133 项 update 测试，backlog 对应条目标记完成。此仍为协作报告，后续整合检查实际文档和实现，不重复恢复已解决债务。

## ptyctl 补充验证

8 个 Rust 模块与 manifest 已完整阅读，新增 15 项要求；进度 20/61 包、121 项 delta 要求、18 个 capability，尚余 41 包。`cargo test --locked -p ptyctl` 退出 0，15 项通过；额外真实 PTY/HTTP 检查复现 stop/timeout 无效、无 linger 时服务仍在、exit_code null 与单值行范围返回两行。测试 child 自行结束，server 显式结束并 wait。

所有已登记文件 SHA-256 与来源符号复核通过，OpenSpec 严格校验通过。通过的测试不覆盖完整生命周期保证，差异登记 backlog，未修改运行时代码。下一步仍须遍历余下全部包，当前增量保持未归档。

## extension-types 补充验证

完整读取 lib.rs 和 manifest，新增 9 项要求，进度 21/61 包、130 项 delta 要求、18 个 capability，尚余 40 包。`cargo test --locked -p extension-types` 退出 0，22 项通过。字段命名、缺省/拒绝未知字段、15 类 HookEvent、六类组件、清洗范围、全部管理动作均映射到源码；不将 DTO 类型声明当成领域 handler 已执行的证据。

已登记文件哈希、来源符号、delta 标题复核通过；严格规范校验和空白检查通过，增量尚未归档。

## crash-handler 补充验证

完整遍历 5 个 src 模块、集成测试、manifest 与 README，新增 8 项要求；22/61 包、138 项 delta 要求、19 个 capability，尚余 39 包。`cargo test --locked -p crash-handler` 退出 0：15 单元、7 集成、1 doctest 通过，1 个 re-exec 专用入口 ignored（由父测试显式运行）。macOS fatal signal 子进程捕获实际执行；Windows/其他架构和故障注入未覆盖。

全部已登记文件哈希与来源符号校验通过，OpenSpec 严格格式与 diff 检查通过。README 及注释偏差按代码记载；报告消费/符号地址身份问题登记为独立债务。

主分支任务同步 `2026-09-07-fix-lsp-untrusted-source-precedence` 已归档，新增 configuration-rules 的 Untrusted project LSP cannot shadow permitted sources，称 105 项相关测试通过、2 项真实 LSP E2E 忽略，shell README/backlog 同步更新；另有 R3 删除候选记录未执行。这些仍是协作报告，后续整合核对实际源码与规范，不计入本分支验证。

## config-types 补充验证

完整遍历 6 模块与 manifest，新增 13 项要求，进度 23/61 包、151 项 delta 要求、20 个 capability，尚余 38 包。RemoteSettings 全部 77 个字段登记包 review 并映射配置传输契约。`cargo test --locked -p config-types --all-features` 退出 0，48 项通过；另有 3 项直接调用真实类型的边界检查通过，包含显式 GC 禁用值随畸形兄弟字段被丢弃的复现。

文件哈希、来源符号、delta 标题复核通过，严格格式校验和 diff 检查通过。所有运行时 resolver、配置对应功能的实际生命周期仍在所属包继续核查，不以字段声明代替运行事实。

## config 中间阅读记录

已完整读取 manifest、lib/paths/fs_atomic/config_override/loader/version_overrides/campaigns/shell 共8个Rust文件，具体行为和指纹保存 reviews/config.md。尚余 global_hook_sources 和 managed_text 共8个Rust文件，全包仍 pending，不增加已完成数量，不提前生成完整覆盖声明。此阶段尚未运行config测试；下一步沿剩余模块完成阅读后汇总delta。

## 当前阶段再次验证（2026-09-07）

在 `codex/openspec-sdd` 独立工作树执行本轮验证：

- `openspec validate --all --strict --no-interactive`：15 项通过；归档校验：2 项通过；`git diff --check` 通过。
- 从磁盘重新解析 package manifest，61 个包的路径、名称、features 与清单一致。23 个 reviewed、38 个 pending，与 tasks 勾选一致。
- 141 份已登记证据文件 SHA-256 全部匹配；已审阅包的 Rust 文件和 manifest 均已登记，review 文件存在。
- 151 项要求、20 个 capability delta、294 个场景与 feature-map 一致；423 个来源引用的文件和符号文本存在，无重复要求或额外 delta 标题。
- 检查 OpenSpec 与开发者入口的 292 个本地文件链接，无失效目标。29 个绝对路径仍有跨机器可移植性限制；未验证 Markdown 锚点。
- 本轮未重跑 Rust 测试，也未重新逐行审阅全部源码；哈希、符号及文档一致性检查不代替行为验证。既有测试结果和失败限制仍按此前各包记录解释。

结论：已登记内容的结构与证据一致性通过；全仓库功能完整性验收尚未通过。38 个包仍待完成，config 的部分阅读仍不计入完成数量。当前 change 保持未归档，总目标保持进行中。

## config 补充验证

完成16个Rust文件、manifest及全部内置测试阅读，新增23项要求；进度24/61包、174项delta要求、20个capability，尚余37包。`cargo test --locked -p config`退出0：71项通过、0失败、0忽略，doctest为0。测试在本独立macOS工作树执行，日志为/tmp/grow-config-tests.log；未把Windows条件代码或恶意并发路径场景视作已动态验证。

规范覆盖路径、配置加载与覆盖、campaign、shell探测、Hook来源和托管文本的计划/格式/发布/恢复/validator生命周期。逐包review保留详细事实，缺陷单列backlog，不修改运行时代码。

config映射后重新检查：全部已登记源码SHA-256、来源符号及delta标题匹配；严格校验15项、归档校验2项和git diff --check通过。当前增量仍未归档。

## acp-transport 补充验证

完整读取10个Rust文件、manifest及全部内置测试，新增11项要求；进度25/61包、185项delta要求、21个capability，尚余36包。`cargo test --locked -p acp-transport`退出0，16项通过，0失败/忽略，doctest为0。内存字节流验证V1握手、batch、扩展映射与在线取消；没有真实IDE、Windows stdin、64MiB超限和任意异步handler顺序验证。具体边界写入review和delta。

另收到主分支协作报告：已归档2026-09-07-fix-inspect-lsp-fallback-display及2026-09-07-fix-inspect-lsp-plugin-status，后者称inspect按PluginRegistry enabled/trusted状态合并，仅active_plugins有效，保留禁用/未信任诊断，并通过10项inspect与98项tools LSP测试、忽略2项真实服务器测试。此为其他工作树报告，未计入本分支验证；后续整合保留实际规范与归档，核对已关闭债务。

acp-transport映射后全部已登记源码哈希、来源符号、delta标题一致；严格校验15项、归档校验2项与git diff --check通过。增量未归档，目标继续。

主分支另报告归档2026-09-07-fix-web-fetch-cache-model-budget：model-sampling新增Cached web content follows current model budget，称缓存完整文本按当前模型窗口经OverflowHandler处理，128项web_fetch测试通过，并记录零容量缓存仍插入一条的独立债务。后续整合须检查并保留实际文档，不计作本分支测试结果。

## workspace-types 补充验证

完整阅读15个Rust文件与manifest，新增17项要求并登记65个RPC关联和160个公开类型声明；进度26/61包、202项delta要求、22个capability，尚余35包。`cargo test --locked -p workspace-types --all-features`退出101：50项通过、1项失败、0忽略，doctest未执行。失败是Hook registry fixture缺少必需on_failure，保持按真实类型写规范；未修改实现或测试掩盖失败。

JSON默认与Rust Default差异、hunk空响应注释偏差、Option字段间未校验等均区分类型事实和实际handler行为。主目标未完成，增量保持未归档。

另收到主分支报告2026-09-07-fix-web-fetch-cache-capacity已归档：configuration-rules新增容量要求，称max_cache_entries=0不保留条目且更新已有URL不淘汰其他页面，130项web_fetch测试通过，相关backlog关闭。后续整合核对并保留实际规范与归档，不将协作报告算作本分支验证。

workspace-types映射后源码哈希、来源符号、delta标题复核一致；严格校验15项、归档校验2项及git diff --check通过。格式通过不改变上述Rust测试失败结论。

主分支另报告2026-09-07-fix-web-fetch-stream-size-limit已归档，configuration-rules新增Web fetch bounds response accumulation，称逐块检查解码正文并提前拒绝超限，真实loopback回归及131项web_fetch测试通过。此为协作报告，待后续核对并保留实际文档，不计入本分支测试。

## sqlite-vec 原生覆盖范围补充

核对FFI构建后发现src Rust行数不能代表扩展全貌：build.rs编译的主C文件及四个条件include共14951行。清单增加native_source_files/native_source_lines，保持pending；逐包review记录已完整读取文件、主C已读行段、注册入口和待审阅算法范围。本轮未运行动态测试，未新增完整覆盖声明，进度仍26/61包、202项要求。

## sqlite-vec 输入解析阶段验证

补读主C输入解析与部分标量运算；本地动态扩展探针16次SQL调用确认非严格JSON接受边界及int8范围/NaN拒绝。探针结果保存在reviews/sqlite-vec-parser-probe.json，构建为dynamic extension而非Cargo SQLITE_CORE，SQLite 3.53.4。完整包仍pending，进度不变。

## sqlite-vec 标量与schema阶段验证

继续读取主C 1847–2598行，补充切片、归一化、JSON转换和schema词法事实。8次动态扩展SQL探针确认零向量归一化产生NaN后被JSON转换拒绝、小数索引经整数转换、空切片拒绝与bit字节对齐。结果保存在reviews/sqlite-vec-scalar-probe.json。全包仍pending，不增加完成数。

收到其他工作树报告：main归档2026-09-07-fix-web-fetch-allowlist-path-identity，称仅规范化host而保留路径大小写和点，tools web_fetch132及workspace web_fetch10通过，另记同主机重定向授权范围问题。后续整合保留并核对实际文档，此报告不计入本分支验证。

## sqlite-vec 索引配置与逐元素查询阶段

新增读取2599–3480行，完整核对rescore/DiskANN参数与vec_each实现；4次SQL动态扩展探针复现bit顺序差异及缺输入约束错误。原生包保持pending，完整覆盖尚未达成；新增事实保存在review，相关差异单独登记backlog。

## sqlite-vec 距离辅助阶段

补读主C191–939，确认L2实际开根号、SIMD分派阈值、Hamming路径与动态数组错误传播；纠正review里将int8 L1 helper误记为i64的初步推断，其真实累计/返回为i32。当前主C1–3480和10580–10716已读，其他范围仍待完成，不扩大测试结论。

## sqlite-vec 存储状态与读取阶段

补读主C3481–4260，记录vtab缓存资源、动态隐藏列位置、TEXT主键映射及flat/rescore/IVF/DiskANN不同向量读取路径。纠正文档注释与真实返回码的差异；未验证完整CRUD或资源释放调用链，包保持pending，完成数不变。

## sqlite-vec 元数据与chunk阶段

补读主C4261–4895，记录metadata布局、partition chunk选择、rowid插入、chunk多步分配和三类cursor清理。确认flat/metadata创建显式绑定两种rowid；完整事务原子性仍待后续回调核查。本轮无新增测试，进度仍26/61包。

## 2026-09-07 用户要求的再次验证

本次在独立工作树grow-openspec-sdd、分支codex/openspec-sdd执行：

- `openspec validate --all --strict --no-interactive`：15项通过，0失败。
- `openspec validate --archived --no-interactive`：2项通过，0失败。
- `git diff --check`：通过。
- 清单61个包的Cargo.toml均存在；185份已审阅文件SHA256与当前源码一致，登记的原生C文件哈希亦一致。
- feature-map中202项要求均能找到对应delta标题，来源文件均存在；涉及22个delta capability。
- 根AGENTS.md、开发指南与OpenSpec CI均包含规范验证流程；docs保留开发者入口，过程资料由OpenSpec管理。

结论：结构与证据文件一致性通过，完整性验收未通过。当前26包reviewed、35包pending，sqlite-vec仍在原生实现逐段审阅中。主规范尚未吸收未完成的全包盘点delta，不得将格式校验通过解释为迁移完成。本次未重跑Rust测试；此前workspace-types全特性测试50通过、1失败的结果仍有效，不应报告全绿。

另收到main工作树报告2026-09-07-fix-web-fetch-redirect-authorization已归档：统一返回RedirectRequired并由新的工具调用授权目标，称tools web_fetch130及Shell ACP29通过。该报告不是本分支验证；后续整合时应核对并保留实际规范与归档。

## sqlite-vec 初始化与建表阶段

补读主C4896–5775，记录列数量/维数/chunk限制、隐藏命令列版本判断及各类shadow表布局。7例建表探针已保存，其中DiskANN语法失败不作为索引共存验证。连续原生主C审阅推进到5775行，完整包仍pending，进度26/61不变。

收到main报告2026-09-07-fix-web-fetch-content-type-matching归档：媒体类型分号前精确且大小写不敏感匹配，称web_fetch131通过。此为外部工作树报告，后续整合须核对保留实际client-surfaces与归档，不计入本分支测试。

## sqlite-vec 查询规划与文本过滤阶段

补读主C5776–7300，完成Destroy、BestIndex、bitmap/top-k helper及metadata过滤源码段。10例查询和DROP检查保存为sqlite-vec-knn-planner-probe.json，复现12字节TEXT metadata范围KNN失败，已独立登记债务。包仍pending，26/61完整覆盖不变。

main另报告纯审计归档2026-09-07-audit-feature-entrypoints（skip_specs），称10组开关入口已核对、17归档通过；未改主规范。后续整合保留实际归档，不将报告算本分支验证。

## sqlite-vec KNN执行阶段

补读7301–8490，记录flat候选求交/距离/top-k顺序、DiskANN独立分派、fullscan和point游标。12例索引对比查询保存到sqlite-vec-index-filter-probe.json，确认DiskANN忽略已下推distance与多值rowid IN、k边界与flat不同，独立登记backlog。更正探针语法为neighbor_quantizer，确认DiskANN可与auxiliary建表插入。未完成包仍pending，整体26/61。

## sqlite-vec 主C完成阅读

补读8491–10579，主C10716行已全部阅读；四个include仍待完成，26/61包状态不变。15步dynamic extension写入探针保存sqlite-vec-write-probe.json，复现失败插入仍可见rowid/vector、auxiliary更新类型与BOOLEAN32位截断差异，验证普通rename与空chunk回收。相关债务独立记录，不声称事务正确性通过。

收到main报告2026-09-07-fix-mcp-unpolled-recovery-claim归档：in-flight守卫于spawn_local前构造，称mcp_restart21通过、18归档通过，extension-runtime同步。后续整合核对并保留实际文档；报告不计本分支验证。

## sqlite-vec rescore与聚类阶段

完整读取rescore687行及kmeans214行；11步rescore探针记录距离过滤缺失、rowid IN正确、命令边界、零量化差异及更新/删除。新增独立债务，未改运行时代码。DiskANN与IVF主体仍待逐行核查，26/61保持不变。

## sqlite-vec IVF实验实现阶段

完整读取IVF1445行；显式实验宏动态库构建成功，10步探针记录过滤缺失和删除后插入覆盖其他存活向量，已登记独立债务。默认构建未启用IVF，不扩大默认能力声明。原生仅余DiskANN1889行，包仍pending，完整覆盖26/61。

## sqlite-vec DiskANN实现阶段

完整读取DiskANN1889行，五C共14951行已读；11步buffer/medoid探针保存sqlite-vec-diskann-probe.json。补记精确距离更新仅接受更小值的已观察差异。包还需能力映射与构建证据收口，暂保留pending，26/61不变。

main报告2026-09-07-fix-mcp-config-probe-cancellation归档，称四处配置探测增加取消select、mcp_restart22与19归档通过，extension-runtime同步。后续核对保留实际增量，不计为本分支验证。

## sqlite-vec完整映射收口

新增vector-storage能力30项要求，累计232项、23个delta capability，27/61包完成阅读与映射。源码覆盖五C14951行及Rust/build/manifest/扩展头；SQLite自带API声明按使用范围核对。cargo check --locked -p sqlite-vec通过，cargo test同参数退出101（非workspace成员需要dev依赖），未宣称Rust单测通过。dynamic extension各阶段探针与缺陷边界保留；本包源码哈希、映射来源符号、严格校验15项及git diff --check通过。其余34包继续盘点，不归档整个变更。

main另报告2026-09-07-fix-mcp-dispatcher-abort-cancellation归档，称dispatcher/e2e35通过、20归档通过，extension-runtime更新。后续整合核对并保留实际文档，不计本分支验证。

## cli入口与stdio重放阶段

开始cli包：完整读取manifest/build及main前650行，记录leader管理、日志、状态缓存和重放时序。发现pending_new单槽且响应未按ID关联、close缓存处理被forward预筛选排除的源码疑点，待后续调用/测试核查；没有执行进程管理命令。包保持pending，整体27/61。

## cli生产入口阅读完成

读取main651–2250，生产调度、runtime/allocator、命令路由、headless参数、更新协调已记录；剩余测试需读完后映射。未运行kill/update/授信类生产命令。包保持pending，27/61不变。

main报告2026-09-07-fix-mcp-stale-liveness-cleanup归档：旧watcher清理在锁内检查token，称MCP156通过、21归档通过，extension-runtime同步。后续整合核对保留实际增量，不计入本分支测试。

## cli完整映射及测试启动

全部main/build/manifest与内建测试已读，新增cli-entrypoints17项要求；累计249项、24个delta能力，28/61包完成阅读映射。来源符号和文件哈希一致，OpenSpec严格校验15项与git diff --check通过。关闭缓存/并发new关联疑点独立登记，现有测试范围如review所述。

启动cargo test --locked -p cli --bin grow，输出/tmp/grow-cli-inventory-tests.log；测试进程仍在编译，当前会话44562，未报告通过。后续须继续等待同一进程结果，不能重复启动。

## 用户要求阶段复验（2026-09-07）

在隔离工作树 grow-openspec-sdd、codex/openspec-sdd 分支执行本轮验证：

- `openspec validate --all --strict --no-interactive`：15 项通过、0 失败。
- `openspec validate --archived --no-interactive`：2 项通过、0 失败。
- `git diff --check`：通过。
- 从仓库 Cargo.toml 的 package 声明重新核对清单：61 包，无遗漏或多列；28 reviewed、33 pending。tasks 仍有 37 项未勾选，不满足全量完成或归档条件。
- 198 个已记录源码文件 SHA-256 全部一致；249 项要求、24 个 delta capability 的映射、要求标题、来源文件与符号以及重复项检查通过。该机械检查证明引用一致，不等同于重新逐行审核所有语义。
- 检查迁移入口和 OpenSpec Markdown 的 390 个本地链接目标，无缺失；未验证外部 URL 或页面内部锚点。
- 核对上一轮 CLI 测试最终日志 `/tmp/grow-cli-inventory-tests.log`：34 passed、0 failed、0 ignored。此为上次运行终态的补记，本轮没有重复运行 Cargo 测试。jemalloc profiling 测试存在条件提前返回，不能推断每个 profiling 分支均执行。

结论：当前阶段文档结构与证据一致性验证通过；全仓库逐包转化尚未完成，不能将本次结果表述为完整权威文档验收通过。既有 workspace-types 测试失败、ptyctl-cli 默认特性编译失败及 sqlite-vec Cargo 单测限制仍保留，未由本轮文档检查消除。docs/AGENTS/OpenSpec 索引对契约权威、过程记录和开发者说明的分工一致。

收到 main 工作树报告 fix-mcp-atomic-recovery-admission（称 MCP157、22 归档通过）和 fix-mcp-stale-tool-error-recovery（称 MCP158、23 归档通过）。这些是另一个工作树的报告，不计为本分支测试；后续整合须保留并核对实际 extension-runtime、README、backlog 与归档增量。

## workflow journal 逐文件阶段

完整读取 journal.rs 1067 行及测试，记录于 reviews/workflow.md。区分生产注入存储与 cfg(test) 文件实现，记录 operation 折叠重复完成和 prune 物理/逻辑顺序的待核对边界。engine、validate、example 尚未完成，workflow 保持 pending，整体 28/61 不变。本阶段未运行新的 Cargo 测试。

## workflow 执行实现与 dry-run 阶段

补完 validate 与 example，engine 阅读至1230行，生产实现已覆盖；剩余测试659行继续阅读。记录 await_user/pause 差异、并行 pending/预算释放、同步宿主等待及 stub 校验边界；根据 engine 的输入顺序持久化，收窄上一阶段的并行物理乱序推测。workflow 仍 pending，未生成不完整能力映射、未增加完成包数。

## workflow 全包阅读与测试启动

已读完engine剩余659行测试，全包8个Rust文件3845行及manifest阅读完成；能力映射尚未收口，28/61保持不变。启动 `cargo test --locked -p workflow`，日志 `/tmp/grow-workflow-inventory-tests.log`，会话44678，等待同一进程终态。

另收到main报告2026-09-07-fix-mcp-stale-tool-timeout-reset，称MCP159及24归档通过；超时reset按服务身份比较且不重放工具。该报告不计本分支验证，后续整合须保留并核对实际extension-runtime/README/backlog和归档。

## workflow 测试终态

会话44678退出0；`cargo test --locked -p workflow`：65 passed、0 failed、0 ignored，doc-tests 0。日志 `/tmp/grow-workflow-inventory-tests.log`。默认当前macOS配置与模拟宿主测试通过，不扩大为真实session或跨平台验证。

## workflow 映射收口

新增22项workflow-execution要求，全部来源符号与文件哈希核对通过；累计271项、25个delta能力，29/61包完成，剩32包。该包65项测试通过；OpenSpec严格校验15项通过、git diff --check通过。全量change仍未完成，不归档。

## client-support 初始模块阶段

开始该包，完成manifest、公共入口、session、stderr、ui_config及clipboard_probe示例阅读；placeholder读至250行。记录配置声明与真实resolver的差异，未接触用户剪贴板；尚未读完附件和clipboard主体，保持pending，整体29/61不变。

## client-support 图片恢复实现阶段

placeholder_images.rs推进至980行，生产加载与恢复已覆盖。确认本轮恢复不扩充去重集合、aggregate不含原附件且读后才检查、file URI优先percent decode；区分注释的prefix-first承诺与wrapper先canonicalize的实际顺序。记录于review，尚未读完剩余测试和clipboard，29/61不变。

## client-support placeholder测试与clipboard入口阶段

完成placeholder_images全1438行，clipboard推进至480行。记录静态路径测试的覆盖范围、MIME魔术识别与真实解码的区别、OSC52输出和子进程deadline语义；本阶段未读取系统剪贴板。clipboard平台主体仍待完成，29/61保持不变。

## client-support macOS读取阶段

clipboard推进至1130行，核对lazy AppKit、metadata/content锁、native kill switch和AppleScript fallback。发现固定临时路径、未限时读取与metadata多消息非跨进程原子性的源码边界，记录review；未运行真实剪贴板操作。包仍pending，29/61不变。

## client-support Linux能力探测阶段

clipboard推进至1795，完成macOS尾部及Linux worker/lease、data-control缓存与工具发现。区分worker deadline和真正取消，记录探测永久缓存与3次不确定重试边界；包继续pending，29/61。

## client-support 剪贴板生产代码完成

clipboard推进2445行，所有平台生产实现已覆盖，剩余907行测试待读。记录CLI reader join与child deadline界限、Wayland空文本回读、PRIMARY fallback及file-list错误阻断附件路径；29/61保持不变。

main另报告2026-09-07-fix-mcp-superseded-respawn-outcome，称Shell mcp_126和25归档通过；旧stdio恢复配置失效返回Superseded。此为外部报告，后续整合核对保留实际extension-runtime/README/归档，不计本分支测试。

## client-support 全包阅读与测试启动

补完剪贴板剩余907行测试，全包阅读完成；默认Cargo测试已启动，未开启4项真实剪贴板ignored测试。当前包仍需能力映射与测试终态，不增加完成计数。

## client-support 测试终态

会话38227退出0，默认macOS `cargo test --locked -p client-support`：97 passed、0 failed、4 ignored，doc-tests 0。ignored均为真实剪贴板测试，Linux/非macOS cfg测试未在此运行；不声称跨平台动态验证。git diff --check通过，能力映射仍待完成。

## client-support 映射收口

新增27项client-surfaces要求，源码符号与哈希核对通过；累计298项、25个delta能力，30/61包完成，剩31包。默认macOS测试97通过、4忽略；严格校验15项及git diff --check通过。全量change仍未完成。

## plugin-marketplace 起始模块阶段

完成manifest、lib/types/error/config阅读，记录source配置顺序、SHA政策、相对路径及GitHub字符串归一化边界。扫描/安装主体尚待读，包保持pending，整体30/61不变。

## plugin-marketplace 索引与扫描阶段

完整读取index/catalog/scanner共696行及测试，记录必需索引、可降级catalog、远程SHA展示门禁、本地manifest元数据与catalog覆盖顺序。安装/Git/matcher仍待完成，30/61不变。

收到main报告fix-mcp-liveness-client-ownership已归档：watcher检查间持Weak，称MCP160、规范15及归档26通过。后续整合须核对并保留实际extension-runtime/Shell README/归档，报告不计为本分支验证。

## plugin-marketplace 匹配与Git执行阶段

完成matcher并读取Git生产逻辑及前段测试至455行，记录URL-only cache/branch TTL边界、lease生命周期、reclone恢复与stderr等待限制。安装解析/执行及Git剩余测试待补，30/61不变。

## plugin-marketplace 目标解析阶段

完成install_resolve673行及Git剩余226行测试。记录qualified source并集歧义、bare-name唯一featured优先和字符串未trim边界；仅installer1502行尚待读，包继续pending，整体30/61。

## plugin-marketplace installer生产阶段

installer推进至820行，生产安装/更新/回滚代码全部覆盖。记录非事务重装、版本字段changed口径、远程update无cache deadline、symlink复制/子目录边界；剩余682行测试待补，包保持pending，30/61。

## plugin-marketplace 全包阅读与测试启动

installer测试剩余682行读完，全包4423行阅读完成。启动串行默认Cargo测试，日志/tmp/grow-plugin-marketplace-inventory-tests.log；能力映射尚待完成，30/61不变。

测试会话12623仍在编译依赖，已轮询同一会话确认运行中，后续继续等待而不重复启动。

main另报告fix-mcp-respawn-final-identity归档，称Shell MCP126、规范15/归档27通过，明确未做真实actor交错回归；后续核对保留实际extension-runtime/归档及HTTP恢复身份检查债务，报告不计本分支验证。

## plugin-marketplace 收口验证

测试会话12623退出0，串行默认测试100 passed、0 failed、0 ignored，doc-tests 0，日志/tmp/grow-plugin-marketplace-inventory-tests.log。新增19项要求，来源符号和哈希一致；严格校验15项及git diff --check通过。累计31/61包、317项要求、25个delta能力，剩30包，整个change仍不归档。

## diagnostics 初始模块阶段

完成manifest、入口、TLS、ID、枚举、Git context和prompt timing阅读，记录ID缓存与权限、typed enum和饱和延迟计算边界。日志输出/事件主体尚待读，包保持pending，31/61不变。

## 用户要求的阶段复验（2026-09-07）

在独立分支 `codex/openspec-sdd` 复验：OpenSpec 1.11.0 全量严格校验 15/15 通过，归档校验 2/2 通过，`git diff --check` 通过。Cargo manifest 枚举与 inventory 对齐，共 61 个包，31 reviewed、30 pending；228 份已记录文件 SHA-256 一致，317 项要求（25 个 delta capability）的清单映射、标题唯一性、源码路径和符号存在性通过。

检查 458 个本地 Markdown 链接；首次脚本将 `:行号` 当作文件名导致 25 个误报，修正解析后并将历史绝对仓库路径映射到当前独立 checkout，全部目标存在。该检查不证明锚点、行号语义或历史链接的跨机器可移植性。未重跑 Rust 测试；既有 workspace-types、sqlite-vec 等失败记录仍然有效。本次是结构、证据完整性及文档一致性复验，不等于全部场景动态验证；剩余 30 包未完成，change 不归档。

main 另报告 fix-mcp-http-recovery-identity 已归档，称 Shell MCP 127、规范 15/归档 28 通过；未做真实 actor 交错集成回归。此为另一工作树的报告，不计本分支验证，未来整合需核对实际变更。

## diagnostics 会话与辅助日志阶段

完整读取 session_ctx/appender/sampling_log/hooks_log/memory_events/memory_log/session_metrics/instrumentation，新增证据记录涵盖 context 缺省值、日志 feature/env 门禁、guard 生命周期、panic hook 和 Chrome 转换。尚未读取完 debug_log/events/unified_log，保持 31/61，未运行 diagnostics 测试。

main 另报告 fix-mcp-init-commit-cancellation 归档，称 MCP 161、规范15/归档29通过，保留锁占用及stdio取消收敛独立债务；属于另一工作树报告，不计本分支验证。

## diagnostics unified log 阶段

完成 unified_log.rs 1037 行及测试阅读，补充同步 writer、维护触发条件、首次打开失败不重试、原地 trim 并发与大小边界、客户端字段信任及快照语义。尚余 debug_log/events，包保持 pending，整体31/61；本阶段未启动测试。

## diagnostics debug log 阶段

完成 debug_log.rs 961 行及测试阅读，补充环境变量优先级、span创建时路由、sanitize碰撞和保留名、惰性sink及按mtime清理边界。只余events模块，包保持pending、整体31/61，未运行测试。

main另报告fix-mcp-stdio-init-cancellation归档，称MCP162、规范15/归档30通过；持续锁竞争仍待处理。另一工作树报告不计本分支验证。

## diagnostics 全包阅读完成

补完events1654行，记录91个事件名称绑定及载荷/枚举/CompactionScope边界。17份Rust源码和manifest均已读；尚待能力映射和测试终态，暂不增加reviewed计数。启动`cargo test --locked -p diagnostics --all-features -- --test-threads=1`，日志`/tmp/grow-diagnostics-inventory-tests.log`。

## diagnostics 测试终态

会话54209退出0，macOS all-features串行测试54 passed、0 failed、0 ignored，doc-tests0。日志/tmp/grow-diagnostics-inventory-tests.log；不代表其他平台执行结果。能力映射仍待完成，reviewed计数保持31/61。

main报告新建fix-mcp-init-cancellation-contention，当前新增持锁取消回归在旧实现确定失败，生产修复尚未完成；不得以其此前162全绿代表main当前状态。此为另一工作树报告，不影响或计入本分支测试。

## diagnostics 映射收口

新增28项local-diagnostics要求，保留完整91事件目录及逐模块证据；源码符号检查通过。累计32/61包、345项要求、26个delta能力，剩29包。macOS all-features串行测试54通过；OpenSpec严格校验15/15及git diff --check通过。全量change仍不归档。

## fsnotify 入口与锁状态阶段

完成manifest、lib/event/error/paths/state，source推进250行；记录共享registry竞争计数与锁状态合并边界。src外的集成测试/bench/example纳入后续遍历。包保持pending，整体32/61，未运行本包测试。

## fsnotify source生产逻辑阶段

source推进860行，生产事件循环已读完；记录HEAD token口径、锁采样、broadcast丢失边界和Cooldown丢弃语义。剩source测试及watcher主体，32/61保持不变。

main报告fix-mcp-init-cancellation-contention已修复归档，称MCP163、Shell MCP127、规范15/归档31通过；此前预期失败已清除，无负载性能验证。此为另一工作树报告，不计本分支验证。

## fsnotify source测试完成与watcher起始阶段

source1498行全部覆盖，watcher推进280行。补充真实/模拟测试范围、ignore缓存和merge重排/Removed优先语义；包仍pending，整体32/61，无动态测试启动。

## fsnotify watcher布局阶段

watcher推进930行，记录glob优先、ignore遍历范围、Git/Sapling发现、fanout/per-dir选择、arm批次、backfill和budget边界。启动callback/command loop及测试仍待补，包保持pending，32/61。

## fsnotify watcher生产逻辑完成

watcher推进1510行，生产callback/启动/command loop已覆盖；记录include覆盖差异、ready部分成功、预算非硬上限、timeout异步清理与shutdown区别。剩测试及example/bench，整体32/61保持不变，未运行动态测试。

## fsnotify watcher测试前段

watcher推进2500行，核对默认ignored与实际断言范围，记录.gitignore扩展名否定断言缺口及fallback测试平台策略依赖。未运行本包测试，包仍pending，整体32/61。

## fsnotify watcher阅读完成

完整覆盖watcher3885行，补充merge测试重投影遮蔽、selector与事件过滤差异、纯helper和真实watch验证区别。仍余集成测试/bench/example，32/61保持不变。

main报告fix-hook-command-output-buffer已归档，称210单元+13集成+1doctest、规范15/归档32通过；HTTP hook DNS/timeout/响应容量另记债务。另一工作树报告不计本分支测试。

## fsnotify 全包阅读与测试启动

完成独立integration370、benchmark169、example211行；本包10份Rust共6701行均已读。启动默认串行测试并nocapture，以辨别shared测试的条件性skip；日志/tmp/grow-fsnotify-inventory-tests.log，尚待终态与能力映射。

## fsnotify 默认测试终态

会话39352退出0，macOS默认串行测试：test result: ok. 117 passed; 0 failed; 15 ignored; 0 measured; 0 filtered out; finished in 3.20s；test result: ok. 2 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 0.33s；test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s。nocapture日志未出现skipping，shared scaling实际执行得到created1/reused49。ignored真实FS场景未运行，性能benchmark未运行；能力映射尚待完成，计数仍32/61。

## fsnotify 映射收口

新增24项filesystem-events要求，来源符号检查通过；累计33/61包、369项要求、27个delta能力。默认macOS测试119通过、21忽略，完整证据保留，剩28包，change仍不归档。

## hooks 入口阶段

完成manifest与lib/error/matcher/result/trust/runner入口，记录匹配缺值、禁用名单非原子持久化及gate/stop严格解析边界。env_expand批量读取截断，后续需分段补读，不计已完成。包保持pending，整体33/61。

main另报告fix-http-hook-response-limit归档，称Hooks211单元+13集成+1doctest、规范15/归档33通过；本迁移分支尚未整合，不能将其流式64KiB修复描述为本分支事实。

## hooks env展开阶段

补完env_expand856行及test_support122行，记录单次非递归、modifier扫描、sentinel和嵌套测试覆盖边界。配置和runner调用主体仍待核对，包保持pending，整体33/61。

## 用户请求的再次验证（2026-09-07）

在 `codex/openspec-sdd` 隔离工作树重新验证：OpenSpec 1.11.0 全量 strict 15/15 通过，历史归档 2/2 通过，`git diff --check` 通过。重新枚举 Cargo package 得到 61 个，与 inventory 的路径及名称完全一致；33 reviewed、28 pending。369 项 delta 要求分布于 27 个 capability，要求无重复，inventory 映射与规格标题、场景一致；257 份已审阅文件 SHA256 匹配，734 处来源路径和符号文本检查通过。

本轮检查了 510 个本地 Markdown 文件目标，无缺失；其中 28 个历史绝对链接按隔离工作树映射核验，未验证锚点或行号语义及跨机器可移植性。来源符号存在与哈希一致不等于重新人工证明所有行为契约。

结论：结构与证据一致性校验通过，但全仓功能提取尚未完成，不能归档全量 inventory change 或宣称迁移完成。本轮未重跑 Rust 测试，不覆盖既有 workspace-types、ptyctl-cli 默认构建及 sqlite-vec 测试限制；此前测试结果保留原记录。

## hooks event/config 阶段

补存此前完整读取的 event903 行记录；本阶段完整读取 config1369 行及测试，记录整层结构失败与单 handler 跳过的区别、默认超时、on_failure 字段存在性、serde matcher 丢失及加载时环境展开边界。剩 discovery/dispatcher/runner/集成与示例，hooks 仍 pending，整体33/61；未启动本包测试。

另一工作树报告：fix-http-hook-dns-binding 使用已校验地址绑定请求且禁用代理，称213单元+13集成+1doc、规范15/归档34通过，无真实代理/TLS集成；随后 fix-http-hook-total-timeout 覆盖校验/请求/正文的总预算，称214单元+13集成+1doc、Shell hook_25、规范15/归档35通过。这些是 main 的报告，未经本分支整合核对，不计入本分支代码事实或测试通过数。

## hooks discovery 阶段

完整读取 discovery974 行与共享文件名谓词，记录快照/动态禁用区别、精确去重键、serde恢复边界和目录读取语义。dispatcher推进290行，尚未完整。已写逐包证据；整体33/61、hooks pending，本轮未执行动态测试。

## hooks dispatcher 阶段

完成dispatcher1314行及测试阅读，记录准入短路、Stop聚合与实际短路区别、观察事件串行await及统计口径。剩command/http runner、integration与examples；hooks保持pending、整体33/61。未启动动态测试，git diff --check通过。

## hooks command runner 阶段

完成command runner1440行及测试阅读，记录shell启发式、输入/输出缓冲、deadline范围、进程组依赖scope、退出码与JSON优先级。剩HTTP runner、integration和examples，整体33/61，未执行动态测试。

main另报告fix-hook-command-cancellation-group已归档，称进程组独立于scope且RAII覆盖异常退出，215单元+13集成+1doc、规范15/归档36通过；Windows未运行，逃逸组/创建失败有边界。该报告尚未整合，不计本分支行为或验证。

## hooks HTTP runner 阶段

完成HTTP runner886行及测试阅读，记录地址范围、未绑定DNS/代理、双阶段timeout、正文无限量读取和固定elapsed、各gate的状态/JSON规则。剩integration与examples，整体33/61；尚未执行hooks动态测试。

## hooks 全包阅读完成与验证启动

完成integration774行及README/5JSON/5脚本，已区分断言不足与真实执行覆盖。Python示例self-test43/43通过；启动 `cargo test --locked -p hooks -- --test-threads=1 --nocapture`，会话80191，日志/tmp/grow-hooks-inventory-tests.log。尚待终态和能力映射，整体33/61不变。

main另报告fix-hook-allow-failure-precedence归档，称217单元+13集成+1doc、规范15/归档37通过；该分支仍按已读代码记录allow覆盖错误状态的现状，不将另一工作树报告计为本分支验证。

## hooks 测试终态

会话80191退出0：macOS串行nocapture报告209单元、13集成、1doctest通过，0失败、0标记ignored。其中/dev/tty测试实际打印skipping: no controlling terminal并提前返回，不能计为终端隔离行为已验证。日志/tmp/grow-hooks-inventory-tests.log。Python示例43/43通过；尚待能力映射，hooks仍pending。

## hooks 映射收口

新增26项hook-execution要求，源码符号存在检查通过；完整逐模块记录保留，Rust/manifest及11份示例文件纳入证据哈希。累计34/61包、395项要求、28个delta能力，剩27包。此前测试209单元+13集成+1doc通过，其中终端隔离测试条件性跳过；Python43项通过的语义局限已保留。OpenSpec strict15/15、git diff --check通过。全仓inventory仍未完成，不归档。

## sandbox 入口阶段

开始sandbox逐包审阅：manifest、types、logging完整，lib推进560行，记录apply Ok与applied区别、全局状态及日志丢失边界、bwrap构造入口。包保持pending，整体34/61；未运行本包测试。

## 主分支后续报告：Hook结构化输出

另一工作树报告fix-hook-structured-output-errors已归档：共享对象解析拒绝位置数组，结构化语法/schema错误返回Failed，Prompt/Tool exit2不被协议错误或未知decision降级；报告Hooks220单元+13集成+1doc、规范15/归档38通过。该变更尚未在本迁移分支整合核对，不计本分支测试，也不覆盖当前hook-execution中按现有代码提取的解析规则；整合时必须同步核对对应要求和证据哈希。

## sandbox lib与paths阶段

完整补读lib和paths，profiles推进250行，记录参数测试范围、read-only保留可写目录及配置加载/合并边界。整体34/61，sandbox仍pending，未运行本包测试。

## sandbox profiles阶段

完成profiles全部生产逻辑和测试，记录各内建路径表、仅内建继承、追加授权、设备筛选与条件性测试跳过。剩deny/hook写保护/network等，整体34/61，未运行本包测试。

## sandbox deny阶段

完成deny/mod458行及测试，glob推进240行，记录路径集合并非canonical、目录存在性、Seatbelt规则与祖先保护边界。整体34/61，sandbox pending，本阶段未运行测试。

## sandbox glob阶段

完整覆盖glob843行，记录Linux展开预算与snapshot局限、macOS规则构造及有限parity测试范围。整体34/61，sandbox仍pending，未运行本包测试。

## 主分支后续报告：Hook缺失raw去重

main报告fix-hook-dedup-missing-raw已归档：缺command_raw/url_raw时回退实际command/url，命令使用OsString；报告Hooks221单元+13集成+1doc、规范15/归档39通过，source_dir执行身份另记债务。本迁移分支尚未整合，因此不替换现有Hook registry deduplication契约，不计本分支测试。整合时需复核实际去重键与证据哈希。

## sandbox child网络阶段

完成child_net所有代码及测试，network_policy推进260行；记录11项syscall deny、架构与TSYNC边界、纯website模型不代表运行时实施。整体34/61，sandbox pending，未运行本包测试。

## sandbox network模型阶段

完成network_policy全部阅读，hook_write_deny推进280行。记录快照版本/哈希语义、纯建模边界及文件身份字段。整体34/61，sandbox仍pending，未运行动态测试。

## sandbox Hook写保护阶段

完成hook_write_deny、独立测试及普通integration，记录plan重验证与实际bind时间窗口、ST_RDONLY检查、OnceLock失败缓存及验证副作用。剩e2e与smoke，整体34/61，未运行本包测试。

main另报告fix-hook-relative-command-dedup归档，直接相对命令去重加入source_dir并共享shell路由判定，称Hooks222+13+1、规范15/归档40通过；仅注册矩阵未做双目录真实脚本集成。本分支尚未整合，不计本分支行为和测试。

## sandbox 全包阅读与测试启动

完成e2e1057与smoke169行，所有源码已读完。启动macOS默认feature串行nocapture测试，会话87326，临时GROW_HOME含固定slots与空sandbox配置，REQUIRE_ENFORCEMENT=1；日志/tmp/grow-sandbox-inventory-tests.log。尚待终态与能力映射，整体34/61。

## sandbox 测试终态

会话87326退出0，macOS默认enforce：55单元、8 e2e父测试、5普通集成、1doctest报告通过，1ignored为供父测试调用的subprocess_entry。REQUIRE_ENFORCEMENT=1，日志未出现support/profile软跳过；8个e2e中marker spoof在非Linux直接返回，实际macOS内核场景为其余7项。Linux seccomp/bwrap分支未执行，不将其计为跨平台验证。尚待能力映射，sandbox保持pending。

## sandbox 映射收口

新增25项sandbox-boundary要求并校验来源符号，保留全部审阅阶段和测试限制。累计35/61包、420项要求、29个delta能力，剩26包。macOS测试结果见前项；未执行Linux或smoke。OpenSpec strict15/15及git diff --check通过，全量change仍未完成不归档。

main另报告fix-hook-invalid-matcher-missing-value归档，Never缺值仍拒绝，称224+13+1、规范15/归档41通过；尚未整合，不替换本分支Hook matching values现状，需合并时核对规格及哈希。

## 用户请求复验：35/61 阶段

在 codex/openspec-sdd 隔离工作树执行 OpenSpec 1.11.0 全量 strict 校验：15/15 通过；历史归档校验 2/2 通过；git diff --check 通过。重新枚举全部 Cargo.toml 中的 package，61 个包的名称和路径与 inventory 完全一致，35 reviewed、26 pending。420 项要求、29 个 delta capability 与 feature-map、inventory 映射及规格要求集合一致，契约和场景文本均存在；299 份已审阅文件 SHA256、785 处来源路径与符号文本检查通过，无重复要求。

本轮属于规范结构和代码证据一致性复验，未重新人工证明全部行为、未重跑 Rust 测试，也未重新检查 Markdown 链接。既有测试失败及平台/条件跳过限制仍有效。全仓功能提取尚未完成，不归档 inventory change，不宣称迁移完成。

main 另报告 fix-hook-registry-matcher-boundary 已归档，称 append/dedup/serde 接纳入口统一重建 matcher 缓存，hooks226单元+13集成+1doc、workspace13、规范15/归档42通过；另报告 fix-idle-controls-and-resume 的旧实现失败及新实现494项回归通过。上述仅为另一工作树报告，未整合或复核到本分支，不计入本次验证结果，后续整合需保留并核对相应规格。

## update 版本与入口阶段

完整审阅manifest、lib、version537行、version_policy221行，auto_update推进500行，并补查Shell版本政策核心。已建立reviews/update.md，记录查询写缓存、独立OnceLock、PATH判断与注释不一致等事实；update仍pending，整体35/61，尚未运行本包测试。

main另报告fix-hook-registry-event-validation归档，serde拒绝事件键/HookSpec.event不一致并校验失败策略，称旧实现两回归失败、hooks228+13+1、workspace13、规范15/归档43通过。该报告未整合，不计为本迁移分支事实或验证。

## update 生产逻辑阶段

auto_update继续完整读取501–2880行，已覆盖全部生产逻辑并进入测试模块。补记后台/--check回退差异、probe缓存边界、下载限额差异、顺序回滚非事务保证及真实安装副作用。剩单元测试与integration；update仍pending，整体35/61，本轮未启动动态测试。

main另报告fix-hook-ignored-matcher-reconstruction归档，Ignored事件只保留原文不编译，Tested保留Never拒绝；称hooks229+13+1、规范15/归档44通过。尚未在本迁移分支整合，不替代本分支当前代码事实。

## update 阅读完成及测试启动

完成所有Rust及manifest阅读，记录测试自身重写freshness、second-pass未实际第二次调用、并行fallback可掩盖覆盖等限制。已检查/usr/local/bin/grow和agent均非symlink，full pipeline不会进入其写分支；集成内部隔离GROW_HOME/HOME/PATH。启动cargo locked串行nocapture，会话27616，日志/tmp/grow-update-inventory-tests.log。尚待终态与映射，整体35/61。

main另报告fix-hook-shell-whitespace-routing归档，命令判定新增tab/LF，称旧真实Unix执行/去重回归失败，hooks230+13+1、规范15/归档45通过。未整合本分支，不替代当前Hook shell事实。

## update 测试终态与映射收口

会话27616退出0：macOS默认feature，cargo test --locked -p update -- --test-threads=1 --nocapture，126单元+25 IO集成+17网络集成=168通过，0失败/ignored，0 doctest。日志/tmp/grow-update-inventory-tests.log。Windows专属测试未编译运行，未执行distro-pm feature；前述替代freshness模型、未实际第二次更新和range可回退的覆盖限制仍有效。

34项release-update要求已映射，8份manifest/Rust证据哈希核对通过，累计36/61包、454项要求、30个delta能力，剩25包。OpenSpec strict15/15与git diff --check通过；全仓尚未完成，不归档inventory change。

main另报告audit-session-recap-lifecycle纯审计归档及55测试通过，fix-recap-sampling-snapshot仍仅提案/静态证据待回归修复。未整合到本分支，不将其视为这里的完成或测试事实。

## codebase-graph 入口及基础类型阶段

核对剩余25包后进入codebase-graph，完成manifest/lib、全部types及interner阅读，记录两套FileEvent/Location入口区别、坐标端点语义和interner窄整数边界。已建立审阅记录，包仍pending、整体36/61；本阶段未运行动态测试。

## codebase-graph 语言查询阶段

完成五语言query、Registry/config及scope nodes/edges/mod完整阅读，graph推进310行。记录语言支持实际范围、query hash输入边界和scope查找/namespace比较语义；整体36/61，包仍pending，未执行测试。

## codebase-graph scope_graph阶段

完成graph1726行完整阅读，明确query构图只做全局定义及孤立引用、全局单跳alias、增量删除/rename边界和SGIX缓存格式实际校验范围。已补审阅记录；整体36/61，codebase-graph仍pending，未执行动态测试。

## codebase-graph manager阶段

完成builder/cache/lock/mod完整阅读，记录tracked优先遗漏untracked、实际全文件读取非mmap、query失败空替代、缓存非原子及跨进程锁边界。整体36/61，codebase-graph仍pending，未执行测试。

main另报告fix-recap-sampling-snapshot归档，称recap client/model/window同prepared config，旧交错回归失败，recap56/model_switch24、规范15/归档47通过；未整合本分支，不作为本分支事实或测试结果。

## codebase-graph navigation阶段

完成navigation全部844行及测试阅读，index_manager推进330行，记录磁盘token与缓存索引时序、byte列/end包含、定义插入顺序及snapshot失败语义。整体36/61，未执行动态测试，codebase-graph仍pending。

## codebase-graph actor命令阶段

index_manager推进1300行，记录Weak去重身份限制、初始化空索引回退、无界队列、事件触发保存与增量先删后读。整体36/61，包仍pending，未执行测试。

main另报告fix-side-question-sampling-snapshot归档，/btw client/model同prepared config，重试沿base_request，称旧交错回归失败和recap模块57通过；未整合此分支，不计本分支事实。

## codebase-graph actor阅读完成

完成index_manager2185行，明确background cached路径与root拼接缺口、扫描锁不覆盖应用、Modified+Removed取消导致潜在旧项保留等边界，完整记录已有断言覆盖。剩bin与integration；整体36/61，包仍pending，未执行测试。

## 用户要求的再次验证（36/61）

在 codex/openspec-sdd 工作树重新执行：OpenSpec strict 15/15、归档 2/2、git diff --check 均通过。扫描 Cargo manifests 的包名与路径，与清单 61 包完全一致。36 个 reviewed 包的 307 份证据文件 SHA-256 全部匹配；Rust/manifest 必需文件无遗漏，另外记录 21 份 C/示例/资源文件。454 项要求、30 个 delta 能力、819 处路径及符号文本引用、场景正文与规范标题集合均一致。初次临时检查错误要求证据集只能含 Rust/manifest，因此把额外资源误报；改为检查必需集合包含关系后通过，文档无需因此修改。符号文本检查不等于语义证明。本次未重跑 Rust 测试，完整盘点仍为 36/61，25 包 pending；inventory 不归档。

main 另报告 fix-suggestion-receiver-cancellation 已归档，AI/Prompt Suggest 经 Sender.closed 取消本地生成 future，称 recap60、Sideband取消终态1、suggest168通过；未整合或复核到本工作树，不计本分支验证，远端计算停止亦无保证。

## codebase-graph 完整阅读及测试启动

完成三个bin与两份integration，至此全部Rust/manifest已读；补记CLI force/JSON/cache差异、RSS可跳过及batch注释不符。开始本工作树 cargo test --locked -p codebase-graph -- --test-threads=1 --nocapture，日志 /tmp/grow-codebase-graph-inventory-tests.log；未获得终态前不计通过，整体仍36/61。

main另报告fix-question-worker-request-cancellation已归档，通知Hook阶段receiver关闭由break改continue，称旧回归第二问RecvError、修复后Shell3+tools64通过；尚未整合复核，不计本分支事实。

## codebase-graph 测试终态

会话11037退出0；macOS默认feature，cargo test --locked -p codebase-graph -- --test-threads=1 --nocapture：52单元+1独立增量内存集成+13内存集成+1 doctest=67通过，0失败，4 doctest ignored，三个bin测试目标各0项。日志/tmp/grow-codebase-graph-inventory-tests.log。实际取得RSS：增量27.9→29.1MiB；本次batch峰值21.0、unbounded45.0MiB，快照增量显示0.0MiB；这仅证明该环境样本通过既有宽松阈值，不推断所有规模性能或修复已记录边界。未运行基准CLI实仓压力测试或Linux目标。完整规范映射尚待完成，因此codebase-graph仍pending，整体36/61；git diff --check通过。

## codebase-graph 映射收口

新增36项codebase-navigation要求，全部manifest/Rust证据哈希、源路径/符号与要求映射检查通过；累计37/61包、490项要求、31个delta能力，剩24包。OpenSpec strict15/15及git diff --check通过。源码边界留在规范及详细review中，本轮未修改运行时代码，inventory仍不归档。

main另报告fix-question-duplicate-option-labels归档，同题重复label发送前拒绝、跨题允许，称旧回归失败及tools问答66项通过；未整合复核，不计本分支事实。

## fast-worktree 入口阶段

开始下一包，完整读取manifest/lib/plan，api推进1000/4673行。新增reviews/fast-worktree.md，记录feature边界、两个ignored并行度默认不同、copy-only取消及删除入口best-effort和路径信任边界。包仍pending，整体37/61；未启动动态测试。

## fast-worktree 删除恢复与GC阶段

api推进1001–2110行，记录metadata拒绝分支成功返回、orphan引用删除顺序、GC force和fresh recheck实际边界；开始测试模块阅读。整体37/61，包仍pending，尚未执行动态测试。

main另报告audit-question-alternate-format已归档，ID formatter开关serde/schema skip且未发现生产开启，新增删除候选R4未删代码；audit-cancel-rewind-lifecycle仍活动，仅改Pager队列回退注释，测试进程16953据报告仍构建。均未在本工作树整合或复核，不计本分支通过结果，也不把远端进程视为本地已确认存活句柄。

## fast-worktree API测试阅读阶段

api推进2111–3160行，核对纯predicate、错误链、真实Git清理与Linux伪mount测试的覆盖差异。发现部分metadata清理测试无局部GROW_HOME隔离，后续运行须在进程级临时GROW_HOME中执行。尚未启动测试，整体37/61，包仍pending。

## fast-worktree API阅读完成

完成api剩余3161–4673行，明确自定义DB测试遗漏注销一致性、pre-remove命名用例未真正抵达二次检查及真实跨进程CWD测试范围；包仍pending，整体37/61，尚未运行测试。

main另报告audit-cancel-rewind-lifecycle完成归档，Pager cancel_rewind3/cancel_75/rewind65通过但过滤重叠不可相加，修正文案以服务端队列为准，称规范15归档53通过。未整合或复核本工作树，不计本分支验证。

## fast-worktree execute阶段

完整读取execute.rs 1–1120行，记录snapshot忽略ignored策略/取消检查、reclaim失败移除metadata、linked ignored阶段取消边界及standalone早期线程退出路径。整体37/61，包仍pending，未运行动态测试。

## fast-worktree execute阅读完成

完成execute全部1731行及worktree/mod前100行。补记standalone实际同步stat更新、HEAD报告读取source、GitCheckout并行参数及取消/cleanup差异；整体37/61，未运行测试。

main另报告新活动fix-config-watch-late-project-directory，仅立项/设计：启动无.grow后需在共享start_config_reload维护层补挂，尚未实现。未整合本工作树，不视为当前代码已修复。

## fast-worktree worktree测试阅读完成

完成worktree/mod.rs剩余101–1317行。记录内容验证、Git状态与性能用例的边界，以及BTRFS集成允许错误/回退不失败的限制。整体37/61，包仍pending；未运行测试。

## fast-worktree copy引擎阶段

完整读copy中除gitdir外七文件，确认忽略错误/部分成功、取消只停止walker、glob不剪枝及Clean+Ignored Copy可能重复制dirty文件。整体37/61，包仍pending，未运行测试。

## fast-worktree gitdir与discovery阶段

完成gitdir及Git模块入口/discovery，明确全树并行与注释差异、linked .git拒绝、symlink平台语义及并行错误测试覆盖。整体37/61，包仍pending，未运行动态测试。

## fast-worktree Git索引与状态阶段

完成index/status/worktree三文件，补记固定SHA1/index空回退、split-index链接、dirty状态范围和stale注册限定清理。整体37/61，包仍pending，未运行测试。

## fast-worktree checkout生产逻辑阶段

完成checkout 1–850行，记录Git环境边界、scratch/index快照语义、ref迁移与恢复删除顺序及部分测试。整体37/61，包仍pending，未运行测试。

## fast-worktree Git审阅完成

完成checkout剩余851–1272行，区分真实迁移恢复证据与仅porcelain/parentless模拟覆盖；Git目录全部阅读完成。整体37/61，包仍pending，未运行测试。

## fast-worktree sync生产逻辑阶段

完成sync 1–790行，记录同HEAD不reset、预计算无版本身份、非UTF8/unmerged解析边界及staged非零仅警告。整体37/61，包仍pending，未运行测试。

main另报告fix-config-watch-late-project-directory已完成归档，shared lock与callback Weak维护重挂且unwatch不复活，称macOS旧首次创建回归失败、watcher7/reloader4通过（targeted包含在7内）。未整合或复核本工作树，不计本分支验证。

## 用户请求的再次验证（2026-09-07）

在 codex/openspec-sdd 工作树执行 OpenSpec 1.11.0：`openspec validate --all --strict --no-interactive` 15/15 通过，`openspec validate --archived --no-interactive` 2/2 通过，`git diff --check` 通过。独立遍历 Cargo.toml 的 package 名称/路径与清单 61 包一致；当前 reviewed 37、pending 24。核对已审阅包必需 Rust 源文件及 manifest 的覆盖、336 项文件 SHA256、859 个来源路径、490 项 requirement 与 31 个 capability 的映射、契约正文及场景文本，未发现不一致。

验证范围：结构、覆盖清单、证据文件完整性和生成文本一致性；未重新运行 Rust 测试，不代表逐条行为语义已经独立复审，也不代表全仓库迁移完成。fast-worktree 仍在审阅中，剩余 24 包未完成；本 change 保持活动状态，未归档。

## fast-worktree sync 完成与自动 GC 阶段

完成 sync.rs 剩余测试审阅，补齐精确 staged blob、skip_clean 和 Unix symlink 的证据边界；auto_gc.rs 分段读取至 960 行，生产逻辑全部读完。记录两层节流、部分失败仍 stamping、dry-run 写 stamp 和 raw options/环境覆盖差异。未修改运行时代码，未运行动态测试；fast-worktree 仍 pending，整体 37/61。

## fast-worktree 自动 GC 与 DB 审阅完成

分段完成 auto_gc 剩余测试及 db 四文件 1425 行，补记故障注入覆盖、标签/路径查询、SQLite 替换与 journal 模式边界。仍 37/61，fast-worktree pending。磁盘 df 实测余 178 MiB，未启动构建；继续进行不依赖构建的代码审阅。

main 报告 fix-discovery-watch-directory-replacement 尚未完成，仅 SDD 与回归测试，无 production 修复；macOS 删除重建旧实现通过，原子替换测试编译遇 ENOSPC。该报告未在本工作树整合或复核，不计本分支测试结果，也不将其 change 标完成。

## fast-worktree discovery 与挂载解析阶段

完整审阅 discovery399/mount_info531/util7/btrfs mod24，btrfs detect 推进至520。补记 fixed-depth unknown候选、重建非事务与路径检查边界、挂载根前缀与Unicode解码限制。未执行动态测试，未修改运行时代码，fast-worktree仍pending，整体37/61。

## fast-worktree BTRFS 审阅完成

完成 detect728 与 snapshot1048 全部代码/测试审阅，记录直接/符号链接创建、metadata三态、删除guard、失败补偿及真实BTRFS测试的skip/固定路径/错误不失败限制。未执行动态测试，整体37/61，fast-worktree仍pending。

main 随后报告 fix-discovery-watch-directory-replacement 已实施 Handle 缓存与非递归父监听，Shell 加既有 same-file 依赖；称独立 rustc harness include 实际 watcher.rs 旧回归失败、修复后10项通过。完整 Shell Cargo 集成因磁盘不足未完成，change仍未归档。未在本工作树整合或复核，不计本分支通过结果。

## fast-worktree overlay 与 CLI 审阅完成

完成overlay三文件及CLI185行，bench推进至285行。记录basename碰撞边界、mount/remove恢复顺序、固定/local扫描与CLI展示不等同实际strategy等事实。未运行Linux或Cargo测试；整体37/61，fast-worktree仍pending，剩bench与integration审阅后再形成完整规范映射。

## fast-worktree 全包源码阅读完成

完成bench808与overlay integration938，确认A/B串行/源Git配置修改及旧overlay布局夹具的验证限制。本包全部Rust源码和manifest已读，尚待正式功能映射及验证，status仍pending，整体37/61。磁盘复查313MiB，未运行构建、基准或Linux集成测试。

## fast-worktree 功能映射完成

新增worktree-lifecycle能力56项需求与场景，总计38/61包、546需求、32个delta能力。独立核对38项manifest/Rust文件哈希与源文件全覆盖、56项映射及场景正文一致。OpenSpec严格15/15、归档2/2通过，git diff --check通过。reviewed标记只表示源码审阅与映射任务完成；本包Cargo动态测试尚未运行，Linux特定测试未执行，磁盘限制和过时fixture边界保留，不作为通过结果。独立债务登记backlog，整个inventory change保持活动，剩23包。

main报告audit-discovery-reload-fanout和fix-pager-duplicate-discovery-advertisement已归档，后者删除Skills watcher额外AdvertiseCommands且保留Workflow分支；称仅格式/规范校验通过，未构建Pager。未在本工作树整合或复核，不计本分支验证。

## hunk-tracker 审阅开始

完成manifest、lib/types/commands/events/handle与actor state，记录内容六态、snapshot路径改写及channel关闭返回边界；其余actor/diff/LOC与测试待审。整体38/61，包pending，未运行测试。

main报告fix-skill-baseline-metadata-reconciliation已实施完整可见baseline比较与SkillInfo Eq，称独立实际源文件编译旧回归失败、修复85项通过但跳过parser集成；完整tools Cargo未执行，change未归档。未整合或复核本工作树，不计本分支结果。

## hunk-tracker actor/query/read 阶段

完整读取actor mod/queries/hunks/file_utils四文件，明确coalescing顺序、restore事件与缓存边界、hunk事件按重叠匹配及bounded read的短读/增长限制。整体38/61，hunk-tracker仍pending，未运行动态测试；继续diff/git/mutations/actions/LOC及测试。

## hunk-tracker diff 阅读完成

完整审阅diff876行，记录按Equal分块、内容/位置/重叠匹配、超限超时空结果及手工patch坐标/换行边界。未运行动态测试，整体38/61，包仍pending。

## hunk-tracker Git 审阅完成

完成actor/git513行，记录发现失败缓存、scoped缓存替换、HEAD基线、symlink canonical与blob分配边界。整体38/61，包pending，未运行动态测试。

main报告将因磁盘不足阻塞，尚未归档两项目录watcher/skill baseline修复，完整Cargo集成未完成；称audit-skill-runtime-consumers已归档、R5仅候选未删除。本分支保留报告，不整合或计作已验证结果，继续不依赖构建的源码审阅。

## 本轮验证复核（2026-09-07，38 包阶段）

在独立工作树 `codex/openspec-sdd` 执行：OpenSpec 严格校验 15/15、归档校验 2/2、`git diff --check` 通过。独立遍历所有 Cargo.toml，61 个 package 名称及路径与清单完全一致，38 reviewed、23 pending。核对已审阅包全部 Rust 文件与 manifest 无遗漏，374 项证据文件 SHA256 全部一致；921 项来源路径及符号存在，546 项需求、32 个 delta capability 的映射、契约及场景文本一致，无重复需求键。

核验脚本首次以 Rust/manifest 集合与全部证据集合严格相等，误报 hooks 脚本、mermaid 字体、compaction 模板和 sqlite-vec C 文件等额外证据；改用必需文件集合包含检查，并保留所有额外证据存在性及哈希检查后通过，没有修改清单来消除误报。

本轮验证未运行 Rust 动态测试、未逐条独立复审全部行为语义。38 reviewed 表示源码阅读和功能映射完成，不代表所有平台及动态测试通过。hunk-tracker 仍在审阅，23 包待完成，inventory change 保持活动，不能判定全仓库 OpenSpec 迁移完成。原 main 工作树的并行改动不属于本轮验证对象。

## hunk-tracker mutations/actions 阅读完成

完成 mutations719/actions635 全文审阅，记录基线刷新与 mode 转换、接受仅改内存、拒绝文件 I/O 和统计先更新的失败边界。没有修改运行时代码或运行动态测试。整体仍 38/61，hunk-tracker pending，后续继续 LOC 和 actor 测试审阅后生成完整功能映射。

## hunk-tracker LOC 审阅完成

完整读取 LOC 实现545行及测试782行，补齐 JSONL 归属、signed delta、接受保留、拒绝冲销、写入失败和取消 drain 边界。尚未运行动态测试；包仍 pending，继续 actor/tests 的5742行审阅，整体38/61。

## hunk-tracker actor 测试阶段一

逐行读取 actor/tests.rs 1–1900，核对基础 tracking、summary、局部接受/拒绝和归属断言，识别旧 bug 注释与当前断言的差异，以及模拟入口不等于 shell 集成验证。尚未运行动态测试，整体38/61，包仍 pending。

## hunk-tracker actor 测试阶段二

读取1901–3300行，补齐批量处理、previous_content、接受后恢复与内容状态断言边界。发现mixed_states测试意图与未知非diffable文件实现分支存在冲突，待动态复现；未把它计为通过。磁盘实测122MiB，未构建，38/61不变。

## hunk-tracker 全包阅读完成

完成actor/tests剩余3301–5742行，累计17个Rust文件及manifest全部阅读。明确真实Git reset/rebase/scoped-cache测试与模拟UI/内存字段检查的区别。下一步生成功能映射及delta并登记独立债务；包尚pending，整体38/61，动态测试未运行。

## 磁盘阻塞恢复

确认无Cargo/rustc构建进程后，使用cargo clean --profile dev --target-dir清理本独立工作树开发构建缓存，命令成功移除137651文件、报告24.3GiB。源文件、规范和测试日志未作为清理目标。旧构建产物已清理，后续动态验证须重新构建；hunk-tracker正式映射继续，仍38/61。

## hunk-tracker 动态测试与映射草稿

磁盘恢复后运行 cargo test -p hunk-tracker -- --test-threads=1，退出0：198单测通过、1性能测试ignored，doc-test 1 ignored、0执行。该结果覆盖当前macOS默认包测试，不包含ignored基准或其他平台。未知非diffable首次接纳分支疑点仍待定向复现，现有mixed_states通过不能证明该分支被覆盖。保存hunk-attribution-draft.json共24项需求及场景，所有来源路径和符号存在；尚待补齐快照、查询、匹配事件等功能后合入正式feature-map，包仍pending，整体38/61。

## hunk-tracker 功能映射完成

36项需求合入hunk-attribution delta，整体39/61、582需求、33能力。源文件哈希再次一致；198单测通过、1性能与1文档示例ignored。严格校验15/15、归档2/2通过。生成器完成主映射后，附加ledger恢复步骤因变量被覆盖失败；已修复审阅文件说明并补验证边界，未重跑追加生成器。独立债务已登记，change未归档，剩22包。

## markdown 审阅开始

完成manifest/lib/parser_policy与独立markdown-fuzz入口阅读，发现fuzz README覆盖说明与实际4条无syntect路径不符，已记录。两个包均pending，整体39/61。

## 本轮验证复核（2026-09-07，39 包阶段）

在 `codex/openspec-sdd` 独立工作树重新执行 OpenSpec 严格校验：15/15 通过；归档校验：2/2 通过；`git diff --check` 通过。独立枚举 Cargo manifest，61 个 package 名称与路径和清单一致。39 个 reviewed 包的 Rust 文件及 manifest 均有证据记录，392 项证据文件 SHA256 一致。957 项来源路径与符号存在，582 项需求及场景正文与 feature-map 一致，33 个 delta capability 无重复需求键，审阅文件存在。

本轮只验证规范格式、清单覆盖和证据/映射一致性，没有重新运行 Cargo 测试，也没有逐条复审行为语义。22 个包仍 pending，markdown 正在审阅；主规范尚未吸收此活动 change 的完整 delta。全仓库迁移尚未完成，inventory change 与总目标保持活动，不以本轮校验通过代替功能覆盖完成。

## markdown 链接审阅推进

补齐流式模块尾段审阅记录，完整读取 url_scan.rs，实现与测试边界已登记；hyperlinks.rs 大输出截断后按区间补读，尚未宣告全文件完成。记录代码内 URL、按位置重叠去重及 tail-relative 行号映射限制。未运行动态测试，整体仍 39/61，22 包 pending，继续该包其余模块。

## markdown LaTeX 核心完成与定向测试

完成 hyperlinks 全文补读与 latex/ 七文件全文审阅。cargo test -p markdown latex::tests -- --test-threads=1 退出 0，44 通过、0 失败、0 ignored、458 filtered out，日志 /tmp/grow-markdown-latex-tests.log。delimiter 读至 420 行、Mermaid 读至 210 行；parse/render 等仍待审，包保持 pending。整体 39/61，不以定向测试通过替代全包覆盖。

## markdown delimiter 全文与探针

完成分隔符 1305 行审阅，定向 Cargo 测试 36 通过、0 失败、466 filtered out。独立真实源文件探针确认 CRLF 围栏关闭、多行 inline code 与长待定尾部限制，记录审阅文件及独立 backlog，未修改运行时代码。Mermaid 继续读至 630 行，整体 39/61 不变。

## 构建缓存清理与磁盘约束

按用户要求，后续构建前检查磁盘余量、阶段测试完成后及时 cargo clean，避免重复积累。确认无 cargo/rustc 进程后执行 cargo clean --profile dev --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target，退出 0，报告删除 14551 文件、1.2GiB。本工作树 target 降至 8.0KiB，df 当前可用 44GiB；同期其他磁盘变化不归因于本次清理。保留源文件与 /tmp 测试日志，未清理其他工作树。

## Mermaid 解析阶段推进

源码审阅从 630 推进至 1820 行，补齐 flow edge、state/class/ER 支持与降级边界、Canvas 输出差异；记录在 markdown review。尚未完成布局及测试，不标包 reviewed，本轮未构建。整体保持 39/61。

## Mermaid 生产实现阅读完成

审阅推进至 3670 行，覆盖分组提升、rank/轨道、循环边、sequence 和 fallback，记录单画布上限与整体内存的区别、截断和窄宽度限制。测试区及 markdown 主 parse/render 仍待完成，本轮未构建；整体仍 39/61。

## Mermaid 全文审阅与 parser 起始

完成 Mermaid 5237 行全部源码和测试阅读，记录仅 contains fallback 文本导致的 ER 别名测试证据不足、元数据与实际形状差异、未测窄宽度等边界。未运行 Mermaid 动态测试。parse.rs 从开头读至 210 行；markdown crate 仍 pending，整体 39/61。

## Markdown 主解析器中段审阅

parse.rs 从 211 推进至 1390 行，完成引用候选、cell separator、事件路由及部分 tag 起始处理，记录数学回退、CRLF softbreak 和列表标记覆盖边界。Link/Image 后续、on_end/table formatting 及 render 尚待完成，整体 39/61，未新增构建。

## Markdown 主解析器全文完成

完成 parse.rs 2456 行，补齐 Link/Image、fence metadata、checkpoint、表格软宽度上限与映射边界。render.rs 读至 240 行，尚待完成全渲染及周边入口。本轮未构建、未运行新测试，整体仍 39/61。

## Markdown 渲染生产路径完成

render.rs 推进至 1230 行，生产 ANSI/ratatui 两路径已全文审阅，记录 ordinary pretty transforms、SourceMap、metadata 消费与 checkpoint 的实际边界；测试剩余部分待读，未构建。main 报告两项 watcher/skill change 已归档且 Cargo 集成通过，未在本工作树整合或复核，不计本分支验证。整体 39/61。

## Markdown 两包完成与全量登记复验

2026-09-07：markdown 全部 28 个所属 Rust 文件、manifest、theme，以及独立 markdown-fuzz 的 target、manifest、README 和 9 份 seed 完成阅读；新增 48 项 delta（markdown 45、markdown-fuzz 3）。当前 41/61 包 reviewed，20 包仍 pending；630 项要求分布于 34 个 delta capability。terminal-markdown 是本次新增 capability，内置文字 Mermaid 补入 diagram-rendering，fuzz 补入 developer-support。逐次阅读记录保留，但最终结果以两个 review 末尾为准。

- 本工作树全特性 markdown 测试退出 0：502 项通过、0 失败/忽略；两个 playground binary 各 0 测试；3 个 doctest 全忽略。实际命令禁用 incremental/dev/test debug 并显式指定本工作树 target，见 reviews/markdown.md；日志 /tmp/grow-markdown-all-features-tests.log。此前 LaTeX 44 与 delimiter 36 均是本次 502 的子集，不重复计算。
- 未运行 benchmark、playground 交互或独立 libFuzzer campaign。全特性编译与单测不能证明这些路径通过。fuzz README 的八组合与实际四条无 Syntect 路径差异已写入规范及 backlog。
- 测试完成立即 cargo clean --profile dev --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target，退出 0，删除 1572 文件约 407.9 MiB。复查 target=8 KiB、可用约 77 GiB；未清理其他任务工作树。
- 重新枚举 crates/third_party 的 61 个 Cargo package，与 inventory 的路径/name 完全相同。reviewed 的 434 份文件 SHA-256 全匹配；630 项要求与场景文本均有对应 spec，1071 个来源文件/符号引用可定位。此为结构和内容一致性检查，不替代语义审阅。
- 校正 markdown 统计为 28 Rust 文件/22736 行，排除独立 fuzz；markdown-fuzz 从仅扫描 src 得到的 0 校正为 1 文件/35 行。theme、README、seeds 计入证据哈希，不计 Rust 行。
- openspec validate --all --strict --no-interactive 为 15/15 通过；--archived 为 2/2 通过；git diff --check 通过。change 继续保持进行中，未提前归档到主规范，全仓库覆盖验收仍未满足。

另一任务本轮报告 main 已归档 fix-skill-baseline-dynamic-shadow 与 fix-skill-discovery-baseline-authority，以及自身 tools/真实文件链路测试结果。这些仅是协作消息，未在当前文档分支合入或复验，不计入本次 Cargo 证据；合并时需重新核对实际代码与规范。

## MCP 完成与登记复验

2026-09-07：mcp全部7个Rust文件及manifest已阅读，新增43项mcp-integration契约。当前42/61包完成、19包pending，673项delta分布于35个capability。进行中change不提前归档。

本工作树锁定依赖测试退出0：155单测、3 SSE集成、1 compile_fail doctest，共159通过、0失败/忽略；日志/tmp/grow-mcp-inventory-tests.log。禁用incremental及dev/test debug后target约1.7GiB，测试后cargo clean删除5041文件约1.7GiB。未改运行时代码、未清其它工作树。

复核所有已登记文件：442份SHA256全匹配、1114个source符号引用可定位；673项contract/scenario与delta逐项文本一致。openspec全量strict15/15、archived2/2通过。平台和测试断言边界见mcp review；不将159项通过当所有并发疑点或Windows真实运行已验证。

## 本轮独立复核（2026-09-07，42 包阶段）

按用户要求重新验证当前分支：`openspec validate --all --strict --no-interactive` 为 15/15 通过，`openspec validate --archived --no-interactive` 为 2/2 通过，`git diff --check` 通过。独立枚举所有 Cargo manifest，61 个 package 的名称和路径与清单完全一致；42 个 reviewed 包的审阅记录存在，442 份已登记证据 SHA256 一致。673 项需求无 capability/requirement 重复键，契约及全部场景文本与 35 个 delta spec 一致，1114 个来源路径及符号存在；各包 requirements 集合与 feature-map 完全一致。

本轮没有重新运行 Cargo 测试，也未声称重新逐条完成语义审计。仍有 19 包 pending，memory 正在阅读，活动 change 保持未归档。磁盘可用约 77 GiB，本工作树 target 为 8 KiB，前一阶段已清理，本轮无需重复 cargo clean；没有操作其它任务的构建产物。

另一任务报告 main 已归档 fix-skill-link-destination-span 及技能内容测试通过，属于未在当前工作树整合或复验的外部进展，不计入本轮证据。

## memory 完成与登记复验

2026-09-07：memory 的14个Rust模块及manifest全部审阅，新增51项memory-search契约；全仓43/61 reviewed，18包pending，724项要求分布36个delta capability。活动change不提前归档。

全特性锁定依赖测试296项通过，0失败/ignored，doctest0项，日志/tmp/grow-memory-all-features-tests.log。测试后立即cargo clean显式本工作树target，删除6539文件2.1GiB，target=8KiB，可用77GiB。watcher测试早退、模拟模型及接缝覆盖限制见review，不据0 ignored推导全部实际OS路径已执行。

登记后457份证据SHA256一致，724项契约及场景与spec文本一致，来源符号存在。严格校验15/15、归档2/2、git diff --check通过。运行时债务独立写入backlog，未修改实现。


## pager-minimal 完成与登记复验

2026-09-07：12个Rust文件及manifest全部审阅，新增36项minimal-terminal要求；当前44/61 reviewed、17包pending，760项要求分布37个delta capability。进行中change未归档，整个目标仍未完成。

本任务独立target的锁定全特性测试86通过、0失败/忽略、doctest0，退出0；禁用incremental与dev/test debug，日志/tmp/grow-pager-minimal-tests.log。真实终端交互、部分IO失败、窄屏边界与资源上限不由这些单测推定，边界及独立债务见review/backlog。

测试结束立即cargo clean --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target，退出0，删除8029文件3.1GiB；target目录已删除，当前可用约74GiB。未操作其它任务的target，同期其它磁盘变化不归因于本次构建。

重新枚举61个package名称/路径一致；470份证据SHA256、760项要求和场景文本、1202个来源路径/符号全部匹配，各包requirements与feature-map一致且无重复键。openspec strict15/15、archived2/2及git diff --check通过。

另一任务报告main归档fix-skill-toggle-identity；未合入或复验当前分支，不计为本次源码事实或测试证据。


## ratatui-inline 完成与登记复验

全部10份Rust文件4078行及manifest已阅读，新增27项要求；当前45/61 reviewed，16包pending，787项要求分布38个capability。校正原src-only统计，examples/benches/integration tests均纳入哈希。

默认及scrolling-regions全特性分别测试，均50 unit+2 differential+5 doctest通过，5 doctest忽略，退出0；详见review及两份/tmp日志。测试范围不含真实终端故障注入或benchmark，部分doc测试上游类型。完成后cargo clean删除1827文件467.2MiB，未清理其它任务target。

481份证据哈希、787项契约/场景、1229个来源符号全部匹配，包requirements映射一致、无重复key；strict15/15、archived2/2、git diff --check通过。目标仍未完成，活动change未归档。


## ratatui-textarea 完成与登记复验

全部13份Rust文件14611行及manifest已审阅；49项要求写入textarea-editing-runtime，当前46/61 reviewed，15包pending，836项要求分布39个delta capability。示例host行为与库能力分别记录，活动change尚未归档。

锁定依赖all-features测试351 unit通过、0失败/忽略，1 doctest忽略，退出0；日志/tmp/grow-ratatui-textarea-tests.log。禁用incremental与dev/test debug，测试后立即清理独立target，删除1164文件349.7MiB。未验证真实终端、系统clipboard和Windows AltGr。

495份证据哈希、836项契约/场景、1278个来源路径/符号匹配，包requirements映射一致且无重复；strict15/15与git diff --check通过。整体目标未完成。

## dagre_rust 登记验证

24份Rust源码及manifest完成审阅，新增32项要求，47/61 reviewed、14包pending，868项要求分布40个delta。520份证据哈希及全部要求/场景/来源符号、包映射匹配；strict15/15通过。现有1单测通过、doctest禁用，不能证明完整布局正确；测试后独立target清理25文件1.3MiB。日志/tmp/grow-dagre-tests.log。活动change未归档，目标仍未完成。

## Mermaid 部分读取后的复验

mindmap、gantt、kanban完整静态读取记录已追加reviews/mermaid-to-svg.md；本包其余文件未读完，未新增完成计数或提前归档。OpenSpec strict 15/15、git diff --check通过；520份已登记证据哈希重新核对一致。整体47/61包reviewed。当前工作树target不存在，无构建缓存待清理；磁盘剩余约74 GiB。本轮未运行Cargo测试，格式及证据校验不代表全项目功能验证通过。

## mermaid-to-svg 源码审阅及包测试

35份Rust源码静态审阅完成，事实记录在reviews/mermaid-to-svg.md。锁定依赖cargo test -p mermaid-to-svg退出0，79 passed、0 failed、0 ignored；doctest由manifest禁用，未进行真实SVG像素或浏览器验证。日志/tmp/grow-mermaid-tests.log，使用独立target且关闭incremental/dev-test debug，测试后立即cargo clean。正式契约映射与证据登记尚未完成，因此仍pending，整体47/61。

## mermaid-to-svg 正式登记复验

48/61 包 reviewed，13 包 pending；994 项要求、41 个 delta、556 份源码哈希、1436 个来源路径/符号与规范文本和包映射核对一致。Mermaid 的35份Rust源码及manifest已完整审阅，126项契约登记完成。严格校验15/15、归档校验2/2、git diff --check通过。79项包单元测试通过，测试后清理218文件54.5MiB；未做浏览器视觉验证，不能据此宣称全项目功能通过。活动change未归档，整体目标仍未完成。

## test-support 包测试

cargo test --locked -p test-support --all-features 使用本任务独立target，关闭incremental与dev/test debug，退出0：71 passed、0 failed、0 ignored；doctest0。日志/tmp/grow-test-support-tests.log。测试后立即cargo clean，删除2615文件695.2MiB，可用磁盘约72GiB。该结果只覆盖本平台包内测试，不证明Windows、真实grow/ACP端到端、极端输入和故障竞态均正确。16项基础契约暂存draft，仍需补齐其余模块映射，不提前标reviewed。

## test-support 登记复验

49/61包reviewed，1044项要求、42个delta；571份证据哈希、所有契约/场景/来源符号一致。strict15/15与archived2/2通过。整体目标未完成。

### sampling-types 登记复验

逐包完成 50/61；feature-map 共 1139 条合同、42 个能力 delta。全量 579 个已审阅文件 SHA256 与当前源码一致，crate requirement 映射与 feature-map 一致，合同正文存在于对应 delta，全部来源路径存在。OpenSpec strict 全量 15/15 通过，git diff --check 通过。sampling-types 现有测试 264/264 通过、doc-tests 0，完成后 cargo clean 释放 2.9GiB。仍有 11 个 crate 未完成，不能据此宣布全仓库审计完成。


## sampler actor 部分读取后复验

本轮复验：OpenSpec strict 15/15、git diff --check 通过；579 份已登记源码哈希、1139 项要求及场景、来源符号、逐包映射一致。当前 50/61 包完成；sampler 静态阅读推进到 18/23 文件，仍 pending。本轮未运行 Cargo 测试，target 不存在，磁盘剩余约 69 GiB。新增 actor ID 回收债务单独登记，未修改运行时代码。


## sampler 包测试

锁定离线all-features测试通过：218 unit、33 integration、doc0；测试退出0后立即清理独立target，删除5425文件1.9GiB，可用约69GiB。日志/tmp/grow-sampler-tests.log。测试清单揭示src外还有6份源码需补审，当前29份Rust只完成23份阅读，正式映射未完成，inventory保持pending。


## sampler 登记复验

当前51/61包reviewed，1235项要求、42个delta；609份源码哈希、全部场景与来源符号及逐包映射匹配。sampler29份Rust及manifest完整审阅，96项契约正式登记，251测试通过。strict15/15和git diff --check通过；测试后已清理1.9GiB。剩余10包，活动change未归档，整体目标未完成。


## 当前文档再次验证

OpenSpec 全量 strict 15/15、归档校验 2/2、git diff --check 通过。609 份已登记文件 SHA256、1235 项要求及场景正文、来源路径和符号全部匹配，无重复 capability/requirement 键。当前仍为 51/61 包 reviewed，10 包 pending，不能视为全仓库功能审计完成；agent 部分阅读尚未计入完成数。本轮未运行 Cargo 测试，独立工作树 target 不存在，无缓存待清理；磁盘可用约 69 GiB。


## agent 包验证启动

27份Rust已完整静态审阅，10项插件基础契约暂存agent-feature-draft.json，尚未正式登记。cargo test --locked --offline -p agent --all-features 已在独立target启动，关闭incremental及dev/test debug；日志/tmp/grow-agent-tests.log，进程session 94907，当前仍在编译，不能报告测试通过。启动命令在测试退出后自动cargo clean该独立target，不影响main工作树。


## agent 包测试完成

cargo test --locked --offline -p agent --all-features退出0：469 passed，0 failed/ignored/filtered，doctest0。日志/tmp/grow-agent-tests.log，session94907已终止。随后的cargo clean删除4687文件1.5GiB，独立target清理完毕。测试包含包内环境变量RAII和临时file:// Git案例；不代表跨平台或真实远程服务、未覆盖竞态均通过。agent契约草稿现25项，仍未正式登记完成。


## agent 正式登记复验

52/61包reviewed，1334项要求、42个delta；648份证据SHA256、全部契约/场景/来源符号、逐包映射及唯一键校验通过。OpenSpec strict15/15、archived2/2及git diff --check通过。agent99项契约已登记，469测试成功；先前cargo clean已释放1.5GiB。剩余9包，活动change未归档，全仓库目标仍未完成。


## 主分支协作消息待整合

收到main任务同步：技能cwd别名仓库边界、祖先链接环、注入根SKILL.md、描述读取预算、损坏/超限YAML、paths/调用开关严格校验、部分展开失败引用索引已调整；称tools103、agent96、shell slash97相关测试通过、规范archive101通过。allowed-tools仅展示，技能model/effort未接采样列R10，旧YAML repair helper列R9。以上仅为协作消息，尚未核对main实际提交与源码，不作为本分支已验证证据。合并前须重新比对agent/tools/shell/paths相关事实和同名规范；本工作树继续独立审阅，不清理main正在链接CLI的target。


## 主分支协作补充消息待核验

main任务称新增归档fix-plugin-skill-use-outcome、fix-skill-expansion-identity、audit-skill-slash-rewrite（删除候选R11但未删除实现）、fix-skill-envelope-attributes，以及verify-cli-skill-expansion；称shell98/tools104/agent96测试、CLI version/help、全量15与归档107通过，未替换用户安装CLI，main target9.8GiB/可用67GiB。此处仅保存协作消息，不视为本分支源码或测试证据；后续需读取实际归档规范与提交并重新比对受影响契约。未清理main target。


## chat-state 包测试

锁定离线all-features测试退出0：460单测通过、doc-tests0。日志/tmp/grow-chat-state-tests.log；session97732完成后cargo clean删除4474文件、1.5GiB。全部17份Rust及manifest已阅读，14项初始契约草稿仍待补全，不能将本次测试通过视为逐包规范转化完成。


## 主分支导出审计协作消息（待核验）

main任务报告归档fix-cli-export-clipboard-result、fix-session-export-relative-path、fix-clipboard-character-stats、fix-atomic-transcript-export、fix-export-during-history-load；称export_回归5项、统计7个表内案例通过，verify-cli-export-fixes已重建grow并通过version/help/export help，未替换已安装版本；规范15、归档114通过。其target约11GiB，可用66GiB，TUI导出同步IO响应性债务另记backlog。以上为协作消息，未核验实际提交/归档/实现，不计入本分支完成证据；后续审阅pager/shell时需比较源码行为与已归档契约。未操作main target。


## chat-state正式映射阶段复验

53/61包reviewed，1417项契约、43个能力delta；667份已登记文件SHA256、全部契约/场景/来源符号、逐包映射与唯一键校验通过。OpenSpec strict15/15及git diff --check通过。chat-state83项契约已登记，既有460单测通过且cargo clean释放1.5GiB；本次未重复构建。还剩8包，活动change未归档，目标未完成。


## 主分支复制审计协作消息（待核验）

main报告已归档fix-session-copy-relative-path、fix-atomic-copy-file、fix-copy-message-selection、fix-copy-delivery-notices、fix-copy-invalid-numeric-args，分别涉及cwd、0600原子提交、选定消息/零索引、未确认交付及负数溢出。另audit-restore-degree-cache登记R12无消费者Pager缓存；称verify-cli-copy-fixes构建及version/help/export help通过，规范15/归档121通过，未替换安装版本，target12GiB/可用65GiB。此处仅记录消息，未核验main实际源码与归档，不计本分支验证；后续整合需复查clipboard/pager事实及契约。未操作main target。


## 主分支异步 transcript 文件写入消息（待核验）

main任务报告已归档background-transcript-file-writes：显式/copy文件与/export文件通过既有Effect/TaskResult及spawn_blocking写入，应用级串行队列最多1执行+8等待，捕获目标路径和内容；旧session完成通知不污染重绑定视图，失败推进队列。称38项针对性测试通过、归档122项通过，未重建CLI；Markdown渲染、剪贴板及默认备份仍同步并另列债务。其target约12GiB、可用65GiB。此处仅保存协作消息，尚未读取main实际提交、实现与归档，不计入本工作树已验证证据；后续pager/shell审阅及整合时须核对，避免覆盖相关变化。本轮未操作main源码或target。


## 主分支协作消息补记（待核验）

主分支任务称 fix-private-pager-transcripts 已归档：Markdown/minimal ANSI 快照使用 NamedTempFile → TempPath，Unix 0600，pending 替换、正常 drop、分页完成和非重试错误随所有权清理；挂起超时保留 owner。称11项相关测试及 verify-cli-transcript-files 的三个只读 CLI 入口通过，重建二进制 SHA256 为 e7f923287c156354d28c8093e80d8c19a45a4f4af5e5a2875e5397f58e99e4b1，未替换安装版本；分页器启动错误/退出码问题另列 backlog。以上仅是协作消息，未检查 main 实现、归档或测试日志，不作为本工作树源码事实。后续整合必须实读差异。本轮无构建或 cargo clean，不触碰另一任务 target。


## 主分支分页器后续消息（待核验）

主分支任务称 fix-pager-process-feedback 在恢复终端后报告启动/失败退出原因（77测试）；fix-pager-quoted-arguments 复用shlex保留引号边界并直接Command、不做shell求值（79测试）；audit-transcript-reload-boundary 与 fix-transcript-reload-restart 使重连入口令minimal在途构建失效，等待后按最终正文重建，覆盖full replay/cursor/rollback与两帧间完成（12相关测试，旧回归先失败）。称都归档、累计128项，这三项未重建CLI；当前CLI仍verify-cli-transcript-files产物。以上仅协作消息，未在本分支检查源码/归档/测试，不计入当前已核验证据。target13GiB/可用64GiB是对方报告，未触碰其构建目录。


主分支消息补记（待核验）：称已归档verify-cli-pager-lifecycle，CLI重建含分页器失败反馈、shlex参数和minimal transcript reload三项，version/help/export help exit0且stderr空；二进制SHA256 b351c558bd616ca86c53cdd059622b4ad983ccb6c2c6d593ae828a20d51e58b9，459417536字节，未替换安装版本，归档129项。未检查其构建日志/源码/归档，不作为本工作树已验证事实。

### 主任务同步：verify-cli-diagnostic-recorders（待交叉核验）

主任务报告main新归档verify-cli-diagnostic-recorders：首次历史加载transcript保护22测试、input dump独立私有文件8测试、scroll log默认UUID路径5测试、失败后状态/单次重启6测试。input recorder审计新增R13（test-only原始按键formatter及自证测试），报告保留待确认，另修正Esc后d快捷键注释。报告CLI SHA256 `1f110ef0fb2f1ff087170c170ac194232b3d8bec53edb5618073d34671d82314`，459420736字节、三个入口冒烟通过、累计135项归档、main target约13GiB。以上仅为收到的同步信息；尚未检查对应提交/归档/二进制，不计入本分支crate验收或测试通过数量，不以此覆盖当前代码证据。

## pager-render运行验证启动

当前工具链没有cargo-nextest。为避免OnceLock/环境变量测试在同进程互相污染，先构建all-features测试二进制，后续按测试名独立进程执行；尚未运行或计入通过。构建命令：`CARGO_TARGET_DIR=/tmp/grow-pager-render-audit-target CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p pager-render --all-features --no-run`，日志`/tmp/grow-pager-render-audit-build.log`，活跃session 33350。使用任务独立target、关闭debug和incremental，完成后及时cargo clean，仅清该target。启动时磁盘可用62GiB，不触碰main工作树target。

## pager-render测试构建完成并开始隔离执行

构建session33350退出0，55.70秒完成，产物`/tmp/grow-pager-render-audit-target/debug/deps/pager_render-a39e567b6b2c662d`。二进制--list得到1036个测试名；已启动session76548按--exact逐个独立进程执行，每项60秒观察上限，不启用--ignored。日志`/tmp/grow-pager-render-isolated-tests.log`，完成后结构化结果`/tmp/grow-pager-render-test-results.json`。目前尚未取得全部终态，不计通过；独立进程用于隔离OnceLock及环境变量，不宣称真实终端/跨平台覆盖。测试完成后清理独立target。

## pager-render隔离测试终态与清理

session76548退出0，结构化结果核对1036项：1034 passed、2 ignored、0 failed、0 timeout。按测试名独立进程执行，当前macOS all-features单元测试范围；未执行ignored、doctest或其他目标平台，也不证明全部需求场景已被测试覆盖。随后独立target cargo clean成功删除6713文件、2.1GiB，磁盘可用62GiB。整包功能映射尚未完成，status仍pending，不因测试通过提前验收。

逐项测试明细已从临时结果保存为本change的`pager-render-test-results.json`，包含1036项名称及状态，避免临时目录清理后丢失明细。该文件是执行结果记录，不替代来源审阅或场景覆盖分析。

### 主任务同步：minimal FPS与清理（待核验）

主任务报告累计140归档、15 spec/active及140 archived strict通过；R14未注册ActionId::DumpInputLog、R15隐藏GBOOM均只列删除候选未删除。报告fix-minimal-fps-hud的8 FPS+86 minimal单测及verify-minimal-fps-pty一项CLI PTY回归通过；报告清理21282文件/15.0GiB后可用76GiB，target/debug/grow已删除，artifact.json仅保留清理前元数据，未改安装二进制。以上为协作同步，尚未核对main提交与归档，不替代本工作树证据，也不据删除候选省略当前GBOOM功能审计。

## pager-render部分登记的证据索引

包清单已登记全部67份Rust、manifest和3份tmTheme共71项已读文件SHA256；每个哈希先与逐文件阅读记录核对一致，再写入reviewed_files。requirements关联当前159项契约。status仍pending，53/61完成数不变，避免把源码阅读/测试通过替代完整功能覆盖验收。feature列表仍保留原Cargo显式声明格式，隐式notify由对应契约说明。

## pager-render 文件映射复核

当前 delta 共 1586 条要求，其中 pager-render 169 条。全清单 738 个已登记源码/资源文件哈希复核一致；pager-render 71 个文件的证据角色见 reviews/pager-render-coverage.md。文件覆盖不等于全部行为已验证，包状态仍 pending。本次只更新文档，无 Cargo 编译产物。

## 外部任务同步：main 第 142 个归档（未独立复核）

主任务报告 audit-transcript-feedback-origin / fix-pager-feedback-origin 已归档，以 PendingPager 保留原 root、AgentId、session 来源；报告 pager_ 16、minimal 86 通过，扩大 root suite 1269 通过/1 失败，word_select_tip_retires_on_prompt_divergence_and_accepts_before 单跑仍失败并已独立登记 backlog。报告规范 15、归档 142 校验通过，磁盘 71 GiB、target 5.2 GiB，未替换安装 CLI。上述为协调消息，未核对 main 源码、归档和测试输出，不用于本工作树的完成证明，也不将 main 修复描述成本分支已有实现。

## 外部任务同步：main 第 143 个归档（未独立复核）

主任务更正上一条 root 失败：报告定位到宿主 clipboard_image_tip 干扰过宽断言，fix-word-select-tip-retirement 仅测试关闭 image_input 并按具体 tip key 断言，保留 Ctrl+Y 行为验证；报告 root 1270 全通过、spec/active 15 与 archive 143 strict 通过、磁盘 71 GiB。此为外部协调结果，未独立核对，不作为本工作树测试证据。

## PTY 控制器测试覆盖静态复核

pty.rs 1094 行（含 14 个测试函数）完整阅读并登记源码哈希。审计区分纯状态分支、Unix 实际进程 fixture 与仅 CommandBuilder 环境投影。尚未运行本包测试，不把测试存在当作通过证据。本轮没有 Cargo 构建产物。

## 外部任务同步：main 第 144 个归档（未独立复核）

主任务报告 fix-clipboard-tip-probe-contention：macOS 剪贴板元数据改 try_lock，忙时未知；未知分类不提交去重，无图按分类版本提交。报告两个状态回归先红后绿、client-support 持锁测试 1 通过且不接触真实剪贴板、pager clipboard_ 68 通过，规范 15/归档 144 校验通过，target 6.0 GiB/可用 70 GiB。保留原生调用总超时与跨进程快照一致性债务。未独立核对这些 main 变更，不用于本分支完成证明。

## 外部 main 同步 145–148（未独立核对）

主任务报告 145 剪贴板元数据前后版本一致性、146 私有探测目录隔离与0700、147 osascript 路径改 argv、148 测试专用 clipboard helper 可达性审计已归档；报告规范15/归档148严格通过。报告在确认无编译进程后 cargo clean 清除12011文件/6.8GiB，可用77GiB。以上只记录协调信息，不作为本工作树测试、磁盘实测或修复已合入的证明；本任务没有因此重建缓存。

## pager-pty-harness 库测试阶段验证

独立target、jobs=2、debug=0，locked/offline运行 --lib --test-threads=1：96通过、0失败、0忽略。详见 pager-pty-harness-test-results.json。全部40文件完整审阅且哈希复核无漂移，文件契约表见reviews/pager-pty-harness-coverage.md。此结果不覆盖真实pager集成、doctest、基准或其他平台；crate仍pending。

验证后已cargo clean独立/tmp/grow-pty-harness-audit-target：移除3959文件、1010.0MiB；当时磁盘可用72GiB。未清理其他工作树target。

## Shell bundle 静态核对

完整阅读bundle.rs 604行并登记SHA256，新增5项application-maintenance契约；feature-map现1701项，shell 32项。活动change strict校验及git diff --check通过。仅核对源码与四个测试函数，未执行Cargo，不新增编译缓存；磁盘实测可用75GiB。全仓库仍53/61包reviewed，shell保持pending，不能将本模块完成视为整个shell或网络下载链路完成。

## Bundle 扩展与HTTP客户端静态核对

extensions/bundle.rs与remote/client.rs完整阅读并登记hash，新增4项契约，累计1705项，shell 36项。活动change strict与git diff --check通过；未运行12个已读测试，未构建Cargo产物。坏manifest重同步无法直接自愈及成功响应体无应用层大小上限已登记backlog。53/61包reviewed不变，agent后台同步完整生命周期仍待审。

## Shell 配置阶段映射复验

803份已登记源码SHA256全部匹配；1729项capability/requirement键无重复、delta标题存在、来源路径存在。inventory旧包采用capability/title，新审阅包采用title：首次直接比较产生53包假阳性，按两种既有格式规范化后全部映射一致，未修改数据以迎合检查。OpenSpec全量strict 15/15与git diff --check通过。本轮未重新逐项语义审计，不能据此归档；53/61包reviewed，8包pending。磁盘可用74GiB，未构建。


## MCP 测试证据补充

新增11处测试源码引用，关联既有5项契约；共1763条要求，53/61包reviewed不变。具体测试适用范围、环境依赖与未执行限制见 reviews/shell.md「MCP 配置测试续审」。本轮未构建，不产生 Cargo 缓存。


## MCP 配置阅读闭合

mcp.rs 2049行已完整阅读并登记源码hash，补充五处测试来源至现有两项契约；1763条要求、53/61包reviewed不变。测试均为源码核对，本轮未执行Cargo或创建构建产物。


## util辅助模块源码核对

完整阅读并登记4文件hash，新增6项要求，累计1769项。未执行Cargo测试，无新增构建缓存；shell仍pending，53/61不变。


## 生产可达性核对

修正3项辅助模块契约的生产作用范围，新增bootstrap要求并登记init.rs全文件hash；累计1770项，53/61包reviewed不变。本轮未构建或执行Cargo测试。


## Auth provider源码阶段

新增4项要求，累计1774项；auth_provider.rs已读hash登记，独立测试模块与token_output仍待审阅。53/61包reviewed不变。本轮无Cargo构建/测试。


## Token输出解析审计

新增2项http-credentials要求，累计1776；登记token_output.rs完整阅读hash。3个内嵌测试仅源码核对，未执行Cargo。整体53/61包reviewed不变。


## Auth provider测试证据第一批

14处测试源码引用补入4项现有契约，1776条要求与53/61包reviewed不变。未运行测试；并发、环境、serde适用范围见shell审阅记录。无新增构建缓存。


## Auth provider测试证据第二批

13处测试来源补入3项既有契约，保持1776条要求及53/61包reviewed。已记录Unix取消fixture与生产启动差异、环境依赖和无HTTP验证边界。未编译、未运行测试。


## 认证测试闭合及全量证据一致性复核

auth_provider_tests.rs 955行完整阅读；全清单826份已登记文件SHA256与当前工作树一致。1776个capability/requirement键无重复，所有delta标题和来源路径存在；按旧capability/title与新title格式规范化后，61包inventory关联与feature-map一致。本检查不替代行为语义审计或测试执行；53/61包reviewed，8包pending，未达到归档条件。本轮无Cargo构建。


## Auth配置绑定阶段

新增1项要求，累计1777项，登记auth/mod.rs全文件hash。配置trusted来源仍待调用链核实，未据类型注释宣布完整信任链验证。53/61包reviewed不变，无Cargo构建。


## Catalog认证查询阶段

新增2项要求，累计1779项。认证事实状态与辅助模型keyless拒绝由源码确认；53/61包reviewed不变，无Cargo构建。


## 认证查询配置来源核验

以加载链源码闭合两项按id查询的配置来源，修正既有契约的trust描述，补3处来源；保持1779项和53/61包reviewed。结论限定当前查询入口，不扩展为所有配置构造链已验收。未构建。


## Provider catalog生产部分

新增2项model-sampling要求，累计1781项；53/61包reviewed不变。provider_catalog测试仍待读，无Cargo构建。


## Provider catalog阅读闭合

登记provider_catalog.rs全文件hash，新增准入校验要求，累计1782项。八个内嵌测试未执行；53/61包reviewed不变，无Cargo构建。


## Sampler与ACP投影审计

新增2项要求，累计1784项；53/61包reviewed不变。仅源码核对，无Cargo构建。


## Session动态认证附加

新增2项要求，累计1786项，确认方法指针形式的生产resolver调用及预刷新revision边界。53/61包reviewed不变，无Cargo构建或测试执行。


## 会话认证恢复分支续审

补充现有认证刷新契约的上层触发条件与源码来源，保持1786条要求、53/61包reviewed。重试预算与memo失效caller仍待核查。无Cargo构建。


## 认证重试预算源码核验

新增1项要求，累计1787项；auth_retry.rs完整阅读hash登记。两内嵌测试未运行；53/61包reviewed不变，无Cargo构建。


## 认证预算及key刷新边界续验

更新2项既有契约，明确预算重置事件与普通key重读route一致性要求；保持1787项和53/61包reviewed。未运行Cargo。


## 工具定义可见性审计

新增1项要求，累计1788项；53/61包reviewed不变。核对源码而未运行测试，无Cargo构建。


## 图像描述路由阶段

新增1项要求，累计1789项；53/61包reviewed不变。仅核对路由准入，图像shadow生成链未闭合。无Cargo构建。


## 图像投影提交边界

新增1项要求，累计1790项；明确不完整描述拒绝提交与分阶段timeout范围。53/61包reviewed不变，无构建产物。


## 图像缓存身份审计

新增1项要求，累计1791项；53/61包reviewed不变。仅源码核对，无Cargo构建。


## 图片资产阶段

新增1项要求，累计1792项；53/61包reviewed不变。源码核对，无Cargo构建。


## 图像描述模块闭合

完整阅读并登记image_describe.rs hash，18个测试已核对、17处新增来源；1792项要求及53/61包reviewed不变。未运行Cargo。


## 持久描述恢复选择审计

新增1项要求，累计1793项；53/61包reviewed不变。源码核对，无Cargo构建。


## 恢复Result引用校验

以固定数组类型及Sideband状态机源码消除恢复索引长度疑点，更新现有契约并补2处来源；1793项、53/61包reviewed不变。未构建。


## 跨账本投影验证

新增1项要求，累计1794项；53/61包reviewed不变。源码核对，无Cargo构建。


## Sideband完成与中断恢复

新增1项要求，累计1795项；53/61包reviewed不变。仅源码核对，无Cargo构建。


## 轻量会话加载审计

新增1项要求，累计1796项；53/61包reviewed不变。无Cargo构建。


## Writer租约阶段

新增1项要求，累计1797项；53/61包reviewed不变。未执行会话清理或Cargo构建。


## 会话隔离删除审计

新增1项要求，累计1798项；53/61包reviewed不变。仅源码阅读，未删除用户数据或运行Cargo。


## 会话身份查询阶段

新增1项要求，累计1799项；53/61包reviewed不变。无Cargo构建。


## 普通会话打开核对

修正既有身份契约，补open_session证据，明确目录缓存不等于Summary缓存；保持1799项及53/61包reviewed。未编译。


## updates读取阶段

新增1项要求，累计1800项；53/61包reviewed不变，无Cargo构建。


## Update封装审计

新增1项要求，累计1801项；53/61包reviewed不变。无Cargo构建。


## Update追加边界

新增1项要求，累计1802项；53/61包reviewed不变。无Cargo构建。


## 追加锁与债务登记

补充既有追加契约锁等待范围，独立记录提交状态不确定性债务；保持1802项及53/61包reviewed。未编译。


## 会话发布事务阶段

新增1项要求，累计1803项；53/61包reviewed不变，无Cargo构建。


### Shell JSONL版本读取补充

新增 Shell versioned JSONL read admission，核对普通读取与版本化读取及Timeline/Sideband调用方；当前功能映射共 1804 项。证据为源码阅读，未执行Rust测试或构建。


### Workflow恢复存储入口补充

核对load_workflow_runs_sync完整函数并写入候选接纳与失败边界，功能映射累计1805项；源码证据，不代表运行时测试通过。


### Workflow manifest权威补充

核对Timeline seed优先验证、sidecar冻结字段准入及恢复状态协调，功能映射累计1806项。仅源码验证，未构建。


### Workflow写入及codec补充

新增注册/队列回执和manifest codec/hash两项契约，累计1808项。源码阅读验证，未执行Rust构建或测试。


### Workflow持久化consumer补充

核对actor、JSONL adapter、manifest写入与tombstone完整调用链，累计1809项契约；运行时测试未执行。


### Workflow store测试证据复核

完成1300行store.rs全文件阅读与hash登记，为4项已有契约补充16个测试引用，需求数量仍1809。测试仅源码核对，模拟回执/两线程竞争/缺失Timeline的覆盖限制已记录在reviews/shell.md。


### 全量一致性复验（1809项）

本轮重新检查功能映射唯一性、各crate需求集合与映射一致性、delta Requirement标题、源码路径存在性及已登记源码SHA256。结果：1809项需求，43个能力，831个已登记文件hash全部匹配。未登记文件不在hash覆盖内，路径存在不证明每个符号语义。初次检查脚本因待审crate无requirements字段中断，改为缺省空列表后完整通过。OpenSpec全量strict：15 passed、0 failed；git diff --check通过。当前磁盘可用70GiB，本轮无Cargo构建或清理。全仓库逐包审计仍未完成。


### 完整session加载补充

新增完整load与rewind读取/追加/替换边界，累计1810项契约。源码核对，未构建或运行测试。


### Rewind intent存取补充

新增事务codec和带回执存储边界，累计1811项；源码阅读证据，未执行测试。


### Pending rewind恢复补充

新增重放分支、执行顺序及文件补偿范围，累计1812项契约；源码证据，未运行测试。


### Rewind延迟历史加载边界修订

补充已有恢复契约的跨crate调用证据：排序已确认，读失败无Result信号且可返回部分内存集合，风险登记backlog。需求仍1812项；未执行故障注入或构建。


### 历史修复shell入口补充

新增resident history repair准入契约，累计1813项；源码验证，未构建。


### Rewind picker补充

新增checkpoint列表与文件元数据投影，累计1814项。源码证据，未运行测试。


### Rewind正常提交补充

累计1815项契约，actor/rewind.rs已全读并登记hash；正常提交与恢复补偿差异明确。源码验证，未构建。


### Rewind测试证据补充

为三项契约补充完整读过的测试函数，需求仍1815项；明确区分临时目录roundtrip与模拟actor错误回执，不作为崩溃一致性证明。未运行测试或构建。


### Pending rewind测试范围复核

补充同actor手动intent恢复和版本错误分类两项测试源码证据，需求仍1815项。真实进程重启、lazy ledger失败和逐持久化阶段故障仍无本轮测试证据。


### 跨compaction派生状态测试补充

为sampler重建和工具定义可见性契约补充3个测试/辅助函数引用，需求仍1815项。Dynamic(true)为测试显式前提；未执行测试。


### 跨compaction测试完整阅读

完成测试文件全读及SHA256登记，补充4个场景/初始化函数引用。需求仍1815项，测试未执行；手动Timeline seed不等于真实compaction provider或跨进程恢复验证。


### JSONL初始化补充

新增初始化准入与发布后lease边界，累计1816项契约；源码阅读，未执行构建或测试。


### Summary patch字段补充

新增字段合并与锁定读取规则，累计1817项契约；源码阅读，未构建。


### Summary写回证据补充

修订summary patch持久化同步边界并补充并发测试引用，需求仍1817项。未运行测试或构建。


### Contained原子发布补充

新增两平台write_atomic主体契约，累计1818项。目录同步失败和Unix临时名清理边界来自源码，未故障注入或构建。


### Summary patch全文件闭合

完成summary_write.rs全文件阅读并登记SHA256，为既有契约补充4项测试证据；需求仍1818项，测试未执行。


### Fork复制主体补充

新增新lineage、权限证据清除和control/announcement继承边界，累计1819项契约；源码阅读，未构建。


### Fork update helper补充

修订update计数截断、顶层session_id转换和源投影过滤精确范围，需求仍1819项。源码验证，未构建。


### Fork路径转换跨crate补充

补充cwd字面替换范围及surface截断入口分流证据，需求仍1819项；未构建或运行测试。


### Surface截断helper闭合

补齐legacy/progressive/markers-only全部分支的源码依据，需求仍1819项；未构建或执行测试。


### Authority缓存补充

新增authority句柄复用和parent定位规则，累计1820项；源码阅读，未构建。


### Cwd marker补充

补齐marker字节上限、no-replace冲突读回和Explicit目录名来源，需求仍1820项；未构建。


### 会话枚举补充

新增枚举错误分层、重复身份与排序规则，累计1821项契约；源码阅读，未构建。


### Timeline派生summary补充

新增事件追加后summary更新范围和错误边界，累计1822项；源码阅读，未构建。


### Timeline追加主体补充

新增prefix guard与tail retry契约，累计1823项；源码验证，未构建。


### Timeline prefix验证补充

补齐逐记录校验、原始字节哈希和内存范围，需求仍1823项；源码阅读，未构建。


### Sideband追加补充

新增尾部序列/重试与完整历史校验边界，累计1824项；源码阅读，未构建。


### Ledger尾部扫描补充

补齐固定buffer与总IO范围、末行分配上限及空末行语义，需求仍1824项；未构建。


### Prefix stamp补充

补充两平台stamp字段及metadata缓存判定限制，需求仍1824项；源码阅读，未构建。


### Strict update export补充

新增envelope严格导出与Timeline错误分类，累计1825项；源码阅读，未构建。


### Prefix cache与测试边界补充

补充cache key和测试wrapper重建prefix限制，需求仍1825项；源码阅读，未构建。


### Dormant标题与锁helper补充

补充User标题追加入口和测试锁命名限制，需求仍1825项；未构建。


### 阶段全量一致性复验（1825项）

重新验证需求唯一性、每crate需求集合与feature-map匹配、对应delta标题及引用路径存在、全部登记源码hash。1825项需求、834个hash全部通过；53/61 reviewed、8 pending不变。路径/标题检查不代表所有源码已覆盖，未登记文件仍待审计。本轮未运行Rust测试或构建。

本次OpenSpec全量strict结果：15 passed、0 failed；git diff --check通过。待完成crate为pager、pager-pty-harness、pager-render、shell、tools、workspace、nix、nono，pending包含已进行部分审计的包，不表示尚未开始。


### Lineage patch补充

补齐已有summary patch的lineage字段语义与时间戳来源，需求仍1825项；源码阅读，未构建。


### Summary格式与展示补充

新增标题结构、显示和hidden规则，累计1826项；源码阅读，未构建。


### Summary隐藏测试补充

补充3个已完整读过的hidden测试引用，需求仍1826项；未执行测试或构建。


### Summary decoder补充

补齐版本优先、重复解析与调用方字节预算边界，需求仍1826项；未构建。


### Resume profile选择补充

新增本地profile选择和错误降级边界，累计1827项；源码阅读，未构建。


### Prompt blob冻结补充

新增引用识别/去重/有界读取/hash验证与冻结规则，累计1828项；源码阅读，未构建。


### 初始blob集合验证补充

补齐精确集合匹配、逐项写入与完整Timeline Messages验证范围，需求仍1828项；未构建。


### 请求prompt物化补充

新增临时导出与路径版差异、读回验证及部分改写边界，累计1829项；源码阅读，未构建。


### Immutable blob writer补充

补齐no-replace冲突、逐字节比较和新旧发布同步错误差异，需求仍1829项；未构建。


### Summary字段默认值补充

补齐serde未知字段拒绝、必填/缺省/省略条件，需求仍1829项；源码阅读，未构建。


### Local resolution补充

新增候选顺序与错误返回语义，累计1830项；源码阅读，未构建。


### 模型选择恢复补充

补充模型切换连续性、字段封闭校验、summary协调和控制回执提取边界，累计1831项。源码审阅，未构建。


### 模型恢复测试证据补充

1831项需求保持不变，补入四项既有测试源码引用及明确覆盖限制；没有运行Cargo。上一阶段OpenSpec全量严格校验15/15通过、git diff --check通过。本轮磁盘检查可用71GiB；无新增构建产物。


### 持久化错误分类补充

累计1832项，补入类型化错误链与ACP分类及四项既有测试的覆盖边界。本轮没有Rust构建或运行测试。


### Worktree活跃刷新补充

累计1833项，补入session打开与actor流量驱动刷新契约。源码审阅，未运行Cargo。


### Light resume绑定来源补充

累计1834项；补入rewind文件预先打开和writer/noop分支。源码审阅，未构建。


### 删除与TTL入口补充

累计1835项，记录Once错误不重试、索引异步通知及TTL转换边界。源码审阅，未运行删除操作或Cargo。


### 清理调用链复核

1835项不变，补齐两处生产调度与Once参数边界、adapter时间等值保留规则。未运行清理或Cargo；风险留在独立backlog。


### Agent名称测试与映射检查

1835项需求的capability/title组合无重复，全部source路径存在且对应delta需求标题存在。六项agent_name测试完成源码审阅，未运行；以上映射检查不证明功能覆盖已完整。


### 本地查找测试证据补充

1835项需求不变，补入10项查找测试源码所在模块并记录不同ID跨cwd不能证明重复ID规则。未运行Cargo或真实会话操作。


### Persistence尾部测试证据

1835项不变，补入四个测试模块（17项测试）及覆盖限制。完成文件尾部阅读不等于全文件审计完成；未运行Rust测试。


### Durable句柄接口补充

累计1836项，补齐noop、入队失败、确认丢失及错误传播边界。源码审阅，未构建。


### 通知合并与pending补充

累计1837项，记录metadata层级差异和提交分类驱动的pending恢复。源码审阅，未构建。


### Actor分派错误路径复核

1837项不变，补充普通ACP失败不恢复、Grow/Timeline绕过pending及flush确认限制；独立债务已登记，未运行故障注入或Cargo。


### Timeline通知补充

累计1838项，追加后索引/标题通知及确认边界已补入。源码审阅，未构建。


### 标题生成采纳补充

累计1839项，补入route生命周期与Sideband完成后标题采纳的非原子边界。源码审阅，未构建。


### 标题helper补充

累计1840项，完整helper源码hash已登记，14项测试只审阅未执行。


### 标题调度调用链补充

1840项不变，补入直接命令调度和手工标题route消费边界。源码核对，未构建。


### 已审阅文件证据复核

新增actor/summary.rs完整文件hash，当前836条已审阅文件hash逐项复算均匹配工作树。1840项需求保持不变；hash一致仅证明证据版本一致，不证明未审阅文件覆盖或场景执行。未构建Rust。


### Persistence测试文件覆盖

1840项不变，完整审阅六项测试及fixture并登记文件hash。源码中的故障probe未在本轮执行；不计为动态故障验证通过。


### Summary中段测试补充

1840项不变，新增title projection与relocation测试源码证据及断言范围。未运行Cargo。


### Summary构造补充

1840项不变，补入构造初值与时间戳边界。源码审阅，未构建。


### Actor消息尾部补充

1840项不变，补齐metadata/rewind分派及自然关闭的一次flush边界。未运行Rust测试。


### Prompt blob构造补充

1840项不变，补入路径与引用构造不执行写入或预算验证的边界；本轮未运行Cargo。


### Timeline桥接补充

累计1841项，新增完整文件hash和两项测试源码覆盖说明；未运行Cargo。


### ACP SDK桥接补充

累计1842项，新增完整文件hash及注册/反向调用契约；未运行Cargo。


### MCP启动wrapper补充

累计1843项，完整文件hash已登记，补入override和pending组装顺序。未运行Cargo。


### NotificationSender补充

1843项不变，完整模块hash及Sideband错误包装边界已补入。未运行Cargo。


### Goal通知补充

累计1844项，完整模块hash已登记，投影与发送边界写入delta。未运行Cargo。


### TurnCompleted构造补充

累计1845项，完整文件hash和四项测试源码覆盖已记录。未运行Cargo。


### Extension结果补充

累计1846项，完整模块hash及结果封装规则已登记。未运行Cargo。


### Replay wire标签补充

累计1847项，完整文件hash和惰性标签生成边界已登记。未运行Cargo。


### Replay事件补充

累计1848项，完整文件hash及窗口/flush请求边界已登记。未运行Cargo。


### Replay窗口调用方审阅推进

1848项不变；定位并审阅ReplayBuffer前155行，确认session隔离先于合并且发现默认值/计数边界待下段核对，尚未登记完整文件hash。未构建。


### ReplayBuffer计数续读

1848项不变，完成主阈值分派和flush源码核对，剩余具体merge/估算/测试待续。未登记全文件hash或运行Cargo。


### Replay metadata阅读推进

1848项不变，补齐metadata合并及chunk范围构造的审阅记录；剩余协议分支与测试待续，未构建。


### Replay合并契约补充

累计1849项，生产合并分支已映射，测试和初始化配置调用仍待续。未构建。


### Buffering配置调用链

1849项不变，补入initialize解码失败降级及空对象默认行为。未运行Cargo。


### Replay定时flush补充

1849项不变，补入生产计时任务与FlushReplay分派证据。未运行Cargo或计时实验。


### ReplayBuffer首批测试证据

1849项不变，首批六项测试源码已审阅，明确越窗测试的等长输入限制；未构建。


### Chunk范围测试续读

1849项不变，新增六项测试源码审阅记录，未运行Cargo。


### Streaming与Grow测试证据

1849项不变，新增四项测试源码审阅及实际输入限制；未构建。


### ReplayBuffer文件覆盖完成

1849项不变，完整文件hash和17项既有测试源码覆盖已登记。没有将文件审阅当作crate完成或运行测试通过。


### Buffered通知发送链补充

1849项不变，补入ACP与Grow发送/持久化差异及flush确认边界。未运行Cargo。


### UI压力与flush wrapper审阅

1849项不变，补齐压力通知及flush_to_disk调用顺序的审阅证据；后续映射将沿已有flush契约补充。未构建。


### 临时通知与flush映射完成

累计1850项；将此前已读updates.rs:355–414的压力通知与flush wrapper结论同步到feature-map、crate requirements及delta。未运行Cargo。


### 更新日志与Grow入口续读

1850项不变，补充日志构造与Grow处理前段证据，发现注释与入队顺序不一致待完整调用链核对。未构建。


### Grow处理链续读

1850项不变，确认SubagentProgress返回发生在入队之后，retry延迟通知入口已读，剩余完成分支待续。未构建。


### 延迟采样失败通知补充

累计1851项，owner匹配和失败通知消费契约已写入。未构建。


### Grow durable重试审阅

1851项不变，完成精确通知重试和passive发送顺序源码阅读，剩余forward和响应投影待续。未构建。


### Grow构造转发补充

累计1852项，额外metadata覆盖与hook阻断边界写入delta。未构建。


### Durable Grow通知映射闭合

1852项不变，将此前718–801与forward实现证据合并入既有Grow契约，包含固定重试、取消分类及passive Ok不保证live送达。未运行Cargo。


### Sampling失败测试证据

1852项不变，新增一项三组合actor测试源码引用及边界，未运行Cargo。


### Actor事件标识与模式FIFO测试审阅

保持1852项需求，本轮补入updates.rs三个完整测试的实际断言、手工事件泵与队列观察边界；不将测试名中的persisted解释为落盘证据。未运行Rust测试，无Cargo构建产物。


### 行为与Goal测试文件尾部审阅

保持1852项需求，补充updates.rs末尾六项测试证据，明确8秒过期未测、完成Goal仅normal/ask两种切换、tracker restart不等于进程恢复，以及合成唤醒实际传Plan的覆盖限制。未运行Cargo，未新增构建产物。


### 出站事件入队契约

新增1项要求，累计1853项；updates.rs全文件阅读完成并登记hash，shell整体仍pending。未构建Rust。


### Grow入站与响应投影映射

新增2项要求，累计1855项，补齐普通入队先于hook及响应usage饱和投影。仅文档更改，未构建Rust。


### Event ID依赖核验与hash复查

1855项不变，补入入站契约shell-base重导出证据。重新读取全部已登记源码并核对SHA256，848项全部匹配；未登记文件不在此覆盖内。未运行Rust测试。


### Subagent usage归属契约

新增1项要求，累计1856项，补入actor命令ack和sticky部分成功边界及首个归属测试证据。未运行Rust构建或测试。


### Usage drain下层阅读

1856项不变，补充报告与ledger标记分离、查询先于deadline检查、后台标记消费时机及sticky ack等待边界。查询和projection下层仍待核对，不提前声明完整时限契约。未构建Rust。


### Usage查询等待契约闭合

新增1项，累计1857项，明确默认120秒仅drain轮询预算，oneshot查询无本地timeout及清sticky无ack。未执行Rust测试或构建。


### PromptUsage输出契约

新增1项要求，累计1858项，记录ACP完整input与headless分桶、费用可信门控、空报告判定及原地赋值边界。未构建Rust。


### Headless解析失败证据

1858项不变，补入解析失败契约和三个测试来源；核对pager普通JSON两个生产入口使用新对象，reducer入口仍待续。未构建Rust。


### Messages usage适配

新增1项，累计1859项；pager首次登记requirements时脚本遇缺省字段中断，已使用setdefault补齐inventory、delta及审计记录。未构建Rust，pager仍pending。


### Messages最终wire核验

1859项不变，补入成功/错误最终ResultLine与有限费用序列化证据。1859项映射capability/title唯一、delta标题存在、引用源码文件存在检查通过；这不代表所有符号语义或全crate覆盖完成。未构建Rust。


### Messages wire结构

新增1项，累计1860项；wire.rs完整阅读并登记hash，pager仍pending。严格校验与差异检查随后执行，未构建Rust。


### Messages状态审阅

1860项不变，完整阅读state.rs并登记hash，补充reducer前329行块整理、ID回退与default=None覆盖stop reason边界。后续事件分发待读，未构建Rust。


### Messages coordinator阅读完成

1860项不变，coordinator全文件登记hash，补入init门控、工具配对、stale completion和finish优先级审计证据。partial与独立测试未覆盖，pager仍pending。未构建Rust。


### Partial framing契约

新增1项，累计1861项，partial.rs完整阅读并登记hash。独立测试待审，未构建Rust。


### Messages工具分组契约

新增1项，累计1862项，明确结果排序、未完成补齐及重复ID边界。测试入口已定位但未执行；无Rust构建。


### 工具测试证据

1862项不变，补入四处直接测试来源，完整阅读六项工具测试及公共fixture并登记hash；未执行Rust测试或构建。


### Messages init契约

新增1项，累计1863项，完整审閱六项init测试并登记hash。未构建Rust。


### 命令元数据映射补充

1863项不变，补入scope/path存在性筛选、工具数组字符串过滤及顺序规则。未构建Rust。


### 公共reducer映射契约

新增1项，累计1864项，公共reducer全文件登记hash，明确工具字段和格式选择。未运行Rust测试。


### ACP流式输出契约

新增1项，累计1865项，ACP reducer全文件登记hash，未构建Rust。


### ACP测试证据映射

1865项不变，补五个ACP reducer测试来源并登记文件hash。未运行Rust测试，未产生构建缓存。


### Assistant frame契约

新增1项，累计1866项；内容测试前九项阅读完成，其余待续。未构建Rust。


### 内容测试与响应边界

1866项不变，content.rs十八项测试完整阅读并登记hash，补响应切换和compact边界契约证据。未构建Rust。


### Result usage测试前段审阅

1866项不变，完成九项测试源码核对并记录命名与实际覆盖差异；后续测试待续。未构建Rust。


### Result usage测试中段审阅

1866项不变，新增七项完整测试的覆盖记录及边界，后续矩阵待续。未构建Rust。


### Result usage测试完整审阅

1866项不变，21项测试完整阅读并登记hash，补入保留stop reason被异常结束覆盖及轮数计量来源。未构建Rust。


### Partial测试前七项

1866项不变，补充partial/frame ID不一致的直接测试证据与签名时序覆盖；测试文件尚未读完，不登记hash。未构建Rust。


### Partial测试中段

1866项不变，新增七项完整测试的覆盖记录，身份清理及空响应行为已有直接断言。未构建Rust。


### Partial测试审阅闭合

1866项不变，16项partial测试完整阅读并登记hash，补五个关键测试来源。未运行Rust测试。


### 引用归属及Emitter阅读

检查client-surfaces中每个映射的源码引用都存在于对应Requirement段，缺失数0。1866项不变，继续读取HeadlessEmitter前265行并登记审计边界；未构建Rust。


### stdout错误契约

新增1项，累计1867项，核对输出latch和最终错误优先级及四项测试源码。未构建Rust。


### Headless启动辅助审阅

1867项不变，记录认证门控、初始化提示、MCP状态展示与Load错误折叠事实。尚未完成启动流程，不登记文件hash。未构建Rust。


### Headless会话与模型辅助

1867项不变，记录显式新ID与加载区别、fork成功后load失败无本地回滚以及catalog缺失的effort处理。主调用链待续，未构建Rust。


### Headless主启动顺序

1867项不变，补齐配置stamp、trust、认证后materialize和恢复cwd顺序；发现早退并非都经过stdout最终错误检查，已明确记录边界。未构建Rust。


### Headless prompt与退出循环

1867项不变，主函数阅读完成，补充hard stdout退出循环、后台等待起点、连接错误优先及wire错误不等于Rust返回Err边界。辅助drain/reap仍待审，未构建Rust。


### 后台回收与排空审阅

1867项不变，补串行逐项10秒、响应payload未核验及drain预算边界。未构建Rust，消息handler待续。


### Headless消息分发契约

新增1项，累计1868项；headless.rs全文件阅读并登记hash，外置测试及ext协议待读。未构建Rust。


### Grow扩展解析契约

新增1项，累计1869项，ext_protocol.rs全文件登记hash；字段默认和载体检查写入delta。未运行Rust测试。


### Ext协议测试前段

1869项不变，完成八项解析测试阅读，补入不匹配reason仍保留stop_sequence的直接证据。未构建Rust。


### Ext协议测试完整审阅

1869项不变，16项测试完整阅读并登记hash，补五处关键引用。未构建Rust。


### Headless后台等待契约

新增1项，累计1870项，补等待/回收边界和四项追踪测试审阅。未构建Rust。


### 回收请求测试证据

1870项不变，补四个回收/排空测试引用及子会话权限隔离审阅；明确reaped测试名不代表真实回收。未构建Rust。


### Headless上下文及结构化测试

1870项不变，新增会话上下文、materialize四组合、权限解析及三项结构化输出测试阅读记录。未构建Rust。


### Headless CLI输入契约

新增1项，累计1871项；cli.rs及headless_tests.rs全文件阅读登记hash。测试仅源码审阅，未运行Rust构建。磁盘可用72GiB。


### CLI权限解析失败策略

累计1872项，补strict与lenient区别及spawn前错误传播。测试只核对源码，未执行Rust测试；本change严格校验和diff空白检查通过。


### CLI agent定义归一化

累计1873项，补文件/名称识别、内联字段处理、map key覆盖及TUI差异。源码核对未执行Rust测试。


### Headless emitter输出契约

累计1874项，补输出格式与meta保留/清空边界；仅源码审阅，未构建Rust。


### Headless会话打开与fork

累计1875项，补Load错误折叠、fork后加载失败边界及辅助判定。未运行Rust测试。


### 共享会话启动分类

累计1876项，补纯标志分类及延迟动作转移；仅源码阅读，未执行Rust测试。


### 会话启动materialize边界

累计1877项，补本地解析及worktree移交，澄清此函数未执行远端恢复。session_title_resolve.rs全文件登记，外置测试待审。未编译Rust。


### 标题选择与歧义报告

累计1878项，补标题大小写边界、manual消歧和提示来源。测试源码阅读至240行，未执行Rust测试。


### 标题pin未命中与目标消失测试证据

1878项不变，补两项materialize测试引用，明确顺序fixture与实际sandbox/并发测试的区别。测试文件阅读推进至425行，未运行Rust测试。


### 恢复目标pin与profile保留

累计1879项；标题测试文件全阅读登记hash，补cli局部调用依据。未运行Rust测试。


### 共享会话启动全文件审阅完成

1879项不变，session_startup.rs登记全文件hash，新增七项测试源码引用。未构建Rust。


### Headless模型与effort调用审阅

1879项不变，补模型设置、Unsupported先于token检查及prompt前失败边界。model_state.rs仅读225–290局部，未登记全文件hash；未运行Rust测试。


### ModelState目录与能力前段

1879项不变，阅读推进到model_state.rs:290，补目录刷新、图像能力与context override边界；共享菜单解析入口已核对，option细节待续。未编译Rust。


### Headless模型与effort契约

累计1880项，补完整调用分支与共享菜单解析依据；ModelState实现读完，内联测试待审。未编译Rust。


### ModelState目录状态契约

累计1881项，补目录刷新和选择行为，测试阅读推进至548行。未编译Rust。


### ModelState测试收尾

1881项不变，ModelState实现与16项内联测试全部阅读登记hash，补两项共享effort测试证据。未编译Rust。


### 模型能力调用方核对

1881项不变，明确图像能力当前直接用于提示资格，context override生产进度入口过滤0；补notices局部审阅。未编译Rust。


### 模型能力契约与源码指纹复验

累计1882项；871项reviewed_files哈希逐项与当前工作树一致，0差异。未运行Rust测试。


### UI提示生命周期中段

1882项不变，notices阅读推进至383行，记录tip遮挡、resize、toast与行为确认状态，底层和测试待续。未编译Rust。


### notices全文件阅读完成

1882项不变，notices实现与六项内联测试读完登记hash，核对清理helper及prompt键盘入口。未编译Rust。


### Toast生命周期契约

累计1883项，补瞬态与sticky生命周期，未构建Rust。


### Toast到期证据补齐

1883项不变，补dispatch测试源码依据并明确键盘段是直接状态赋值；确认active视图维护门控。磁盘72GiB，未运行Rust测试。


### URL打开回退调用链

1883项不变，定位重导出并复核agent与无agent回退差异；未执行浏览器或剪贴板操作，未编译Rust。


### 浏览器失败UI回退契约

累计1884项，补活动agent与无agent路径及spawn成功判定边界。未编译Rust。


### EphemeralTip底层状态阅读

1884项不变，完成ephemeral.rs实现至280行，核对同key刷新优先于cap与暂停到期顺序；测试待审。未运行Rust测试。


### EphemeralTip测试完整审阅

1884项不变，ephemeral.rs含12项测试全部阅读登记hash；明确暂停恢复尚无本文件测试覆盖。未编译Rust。


### 临时提示状态契约

累计1885项，单槽、外部计数及计时策略写入delta；未运行Rust测试。


### 提示计数所有者核对

1885项不变，确认AppView共享计数与成功展示后才提交诊断/clipboard cooldown；未编译Rust。


### 提示触发与接受分层

1885项不变，补word-select键入口与dispatch校验差异，word_select构造文件需重读截断部分。未编译Rust。


### Word-select构造文件完成

1885项不变，word_select.rs全文件登记hash，明确设置仅排出持久化effect而非同步保存完成。未编译Rust。


### 设置持久化异步结果链

1885项不变，补PersistSetting排程及普通/best-effort失败处理差异，实际持久化与rollback helper待审。未编译Rust。


### Word-select保存与缓存回滚分支

1885项不变，定位Enum保存调用和canonical缓存回滚，未将其当作磁盘事务回滚。未编译Rust。


### Word-select接受与保存边界契约

累计1886项；快捷键、dispatch与异步保存分层落盘，未运行Rust测试。


### 三类提示构造审阅完成

1886项不变，small_screen、ssh_wrap、send_now共三文件及12项测试阅读登记hash。未编译Rust。


### 首绘提示触发核对

1886项不变，补small-screen与SSH的defer/consume区别及配置gate顺序，clipboard底层待审。未编译Rust。


### ClipboardFocus状态机实现审阅

1886项不变，完成轮询/去重/冷却实现阅读，测试及native probe待审。未编译Rust。


### ClipboardFocus测试前段

1886项不变，完成六项测试源码审阅，确认fake probe覆盖及UI拒绝模拟边界。未编译Rust。


### ClipboardFocus全文件审阅完成

1886项不变，clipboard_focus.rs及10项测试完整登记hash，区分poll与上层冷却gate、未知count predicate与cheap读取路径。未编译Rust。


### Clipboard提示轮询契约

累计1887项，写入节流、冷却、成功展示提交与资格门控。未编译Rust。


### Plan nudge关键词审阅

1887项不变，plan_nudge.rs及四项测试完整登记hash，明确whole-word字节规则与测试命名边界。未编译Rust。


### PromptWidget计划提示触发点

1887项不变，核对编辑前后keyword边沿、paste排除和信号消费；测试待审。未编译Rust。


### Plan提示编辑测试证据

1887项不变，完成七项输入测试阅读，第八项待收尾，未编译Rust。


### Plan提示输入契约

累计1888项，八项输入测试局部审阅完成，关键词与触发契约落盘。未编译Rust。


### 撤销提示检测器全文件

1888项不变，clear_detector.rs及八项测试阅读登记hash，明确resync仍可触发和字符计数单位。未编译Rust。


### Undo提示输入级证据

1888项不变，完成六项undo提示测试及两项图片编号测试局部审阅，明确实际undo未执行边界。未编译Rust。


### 撤销提示检测契约

累计1889项，补阈值、重同步、图片保护与展示条件；未编译Rust。


### tips渲染文件审阅

1889项不变，render.rs与mod.rs完成登记，补三项buffer测试边界与高度估算限制。未编译Rust。


### 提示布局消费核对

1889项不变，补agent banner普通tip与ephemeral覆盖顺序及多行预留边界。未编译Rust。


### 横幅提示绘制优先级

新增Agent banner tip rendering precedence，累计1890项。补读mode banner前置分支，明确窄区域无回退、公告占位与ephemeral全矩形覆盖；不把底层buffer测试当完整UI验证。未进行Cargo构建。


### 启动提示评估边界

新增Small screen and SSH startup tip evaluation，累计1891项。核对minimal分支、先后顺序、尺寸延后、配置关闭消耗及SSH占槽延后；构造测试仅源码审阅，本轮无Cargo构建。


### 图片提示原生证据闭合

1891项不变；扩充已有图片提示契约，补平台支持/加载失败、类型过滤、进程内锁边界及event loop调用证据。仅代码审阅，未访问用户剪贴板或构建Rust。


### Send-now提示消费契约

新增Queued follow up send now tip and acceptance，累计1892项；区分本地展示/接受与远端确认，补调用方来源。未构建Rust。


### 队列steering确认边界

新增Queue row steering admission and optimistic confirmation，累计1893项。记录单槽等待、自然drain胜出、无foreground确认及本地先删除边界，未将局部Action测试当端到端发送证明。无Cargo构建。


### Steering失败处理契约

新增Steering dispatch and failed payload review queue，累计1894项；完整登记interject dispatch文件，明确新ID队首复核、composer保留与通知错误的不同策略。无Cargo构建。


### Steering响应与恢复测试证据

1894项不变，补acp_send等待响应而非仅enqueue的精确边界；turn_pipeline176行六项测试完整源码审阅并登记hash，未执行测试。失败复核可编辑附件由测试断言支持，未宣称端到端验证。无Cargo构建。


### Queue全文件及全局证据复核

queue.rs694行完整审阅，四项steering/编辑与三项watcher测试仅源码审阅未执行。首次逐crate脚本未处理旧capability/title格式，在acp-transport断言中断；修正比较规则后完整通过。1894项需求唯一性、delta标题与来源路径存在检查通过；逐crate集合按既有capability/title或title两格式归一后匹配，885个登记文件SHA256全部匹配。OpenSpec全量strict15/15、git diff --check通过，磁盘72GiB，无Cargo构建。该检查不证明未读文件覆盖或全部符号语义正确，逐包目标仍未完成。


### 后台工作计数投影

新增Background work watcher count projection，累计1895项。补状态判定、workflow子项排除、awaitable子集及标签格式，明确快照计数不等于外部存活探测。无Cargo构建。


### Turn status可见性契约

新增Turn status visibility and startup seed expiry，累计1896项。核对非Idle状态范围、MCP seed30秒严格边界及进度可见性与状态行可见性的差别。五项相关测试源码已读未运行；无Cargo构建。


### 状态行优先级与布局调用方

新增Idle and parked status rendering precedence，累计1897项。补control状态额外占行与Idle/parked分支优先级、窄宽度及点击区域条件。无Cargo构建。


### 运行态计时和控件

新增Running turn status timers and controls，累计1898项。补按钮状态门槛、Ask计时例外、queued后缀舍弃与右区样式清理，明确未保证任意窄行边界。无Cargo构建。


### 状态行测试证据补充

1898项不变，补七个测试来源至三个既有契约。区分40列idle标签截断与未验证的running右侧窄屏边界；阅读测试不等于执行通过。无Cargo构建。


### Turn status全文件收口

1898项不变；turn_status.rs1655行源码与测试阅读完成并登记SHA256。修正helper parkable与AgentView实际parked之间的Subagent排除边界；完整文件阅读不代表整个pager包完成。无Cargo构建。


### 队列编辑入口契约

新增Queued edit entry focus lock and draft restoration，累计1899项；覆盖optimistic拒绝、文本dirty锁、hold effect和幂等恢复，保存/删除剩余分支继续审阅。无Cargo构建。


### 队列保存与hold顺序

新增Queued edit save payload and hold ordering，累计1900项；queue_edit.rs624行完整阅读登记。服务器保存文本与本地结构化载荷处理分开记录，未宣称通知已成功应用。无Cargo构建。


### 队列编辑回归源码证据

1900项不变，补simple-mode rollback释放effect和ignore PTY liveness测试来源。PTY233行完整审阅，不把含图片类型的宽泛谓词说成精确附件验证；测试未执行，无Cargo构建。


### 队列镜像合并契约

新增Queue pane merged row identity and visibility edges，累计1901项；明确server先local、仅过滤running、hash选择ID和长度驱动自动显示。无Cargo构建。


### 队列文本投影契约

新增Queue row summary and full text projection，累计1902项。区分摘要、行数后缀与完整复制/搜索内容，注明极窄Line预算边界。无Cargo构建。


### QueuePane键盘动作和选择

新增Queue pane action keys and selection repair，累计1903项。区分搜索接口与关闭的UI搜索、registry优先与keycode分发、删除选择及高度转换边界。无Cargo构建。


### QueuePane鼠标和绘制

新增Queue pane hover actions and preview rendering，累计1904项。全文件阅读登记hash，区分按钮呈现与业务准入、hover按钮与selected预览，并保留empty旧hit待调用方审计边界。无Cargo构建。


### Queue鼠标调用方边界

1904项不变，补缓存PaneAreas先决条件、按钮路由顺序、删除drain差异和编辑锁；明确cached rect不等于实时布局。无Cargo构建或动态点击测试。


### 即时路由和本地drain门槛

新增Immediate server routing and local drain admission，累计1905项，核对本地FIFO门槛和behavior latch副作用。未运行Rust或构建。


### 本地合并出队载荷

新增Local combined dequeue payload projection，累计1906项；补首项保护所在层、follower停止条件、chip字节偏移和原始分段保存。九项相关测试仅源码审阅，无Cargo构建。


### 合并出队剩余测试证据

1906项不变，补两项图片follower和skill ranges测试来源；十一项相关测试完整源码审阅，未执行测试。开始追踪出队后TurnSubmitting与scrollback echo，剩余分支继续审计。无Cargo构建。


### Prompt drain wire投影

新增Local prompt drain echo and wire payload selection，累计1907项；明确回显先于执行、快照条件、Some空wire及图片/ranges处理。无Cargo构建。


### 合并回显身份规则

1907项不变，扩充既有契约，记录按message ID和条数复用、不按文本去重及部分匹配追加边界。继续读取server turn-start shim，未运行Rust。


### 服务端回合接管投影

新增Server turn adoption echo and rewind projection，累计1908项。补同ID复用、文本恢复/附件缺失、tracker activity清快照与待处理能力消费。无Cargo构建。


### Queue dispatch文件收口

新增Composer attachment transfer to newest queued row，累计1909项；dispatch/queue991行完整阅读登记，明确chip写入与拒绝图片的顺序。无Cargo构建。


### QueueChanged调用方准入

1909项不变，扩充接管契约并登记queue通知入口全文件201行，补Root限定、字段fallback与同IDSubmitting条件。无Cargo构建。


### 共享队列echo合并

新增Shared queue snapshot optimistic echo reconciliation，累计1910项；记录广播替换与未确认echo追加、重复ID不刷新和版本不比较的局部行为。无Cargo构建。


### 队列路由证据与全局复核

补Idle shared queue路由测试来源，1910项不变。1910项映射唯一、delta标题/来源文件存在、逐crate需求集合匹配，891份登记SHA256全部一致。检查不证明所有符号语义或未读文件覆盖；Rust测试未执行，无Cargo构建。


### Usage弹窗与结果身份

新增Usage modal opening and usage result identity，累计1911项。核对三入口、三effect共享nonce及usage结果session/nonce门槛；Context/SessionInfo接收路径继续审计。无Cargo构建，未执行Rust测试；磁盘可用71GiB。


### Context与会话信息接收范围

新增Context and session info result projection boundaries，累计1912项；记录成功与失败路径不同身份门槛、实时状态先于nonce更新及测试断言边界。未运行Rust测试或Cargo构建。


### Usage交互与复制

新增Usage modal content navigation and copy selection，累计1913项；核对chrome优先路由、tab状态复位、滚动选择及跨agents复制查找。相关测试仅源码审阅，无Cargo构建。


### Usage渲染与字段投影

新增两项契约，累计1915项；usage_modal.rs全文件阅读登记。源码测试审阅不能替代运行验证；未构建，无新增Cargo产物。


### Status辅助入口

新增Status auxiliary surface dispatch admission，累计1916项；登记status.rs全文件hash。严格验证与差异检查用于规格结构，不代表运行时验证；无Cargo构建。


### Queue与Tasks输出投影

新增静态快照契约，累计1917项，核对分组排序、结束项保留、行数与多行description边界。无Cargo构建。


### Usage账本展示

新增Session usage ledger display and cost absence，累计1918项。完整阅读status_blocks.rs及两份快照；未运行Rust测试或Cargo构建。


### 全局复核与视图身份

新增前1918项映射唯一、delta标题/来源存在、逐crate集合一致；894个已登记源码SHA256均匹配。当前53 reviewed、8 pending，不将指纹一致性当作全包完成证明。新增视图解析契约后累计1919项，无Cargo构建。


### Agent切换镜像与临时状态

新增契约后累计1920项，ctx.rs全文件hash登记；记录debug断言、同target早退、hint消费及root-only睡眠检查。无Cargo构建。


### CTA候选和安装回报

新增契约后累计1921项，核对候选过滤、匹配后dismiss及安装结果分支。无Cargo构建，无外部插件操作。


### CTA后续探测

新增契约后累计1922项，cta.rs全文件hash登记，核对reload/MCP/catalog/debounce及两个定时effect。无Cargo构建或外部插件操作。


### CTA动作链补充

1922项不变，扩充连接动作准入、MCP预期判定与Installed timeout接收门槛。未运行Rust测试，无Cargo构建。


### CTA发送侧与测试范围

1922项不变，补generation通知和连接测试来源，明确fixture与测试名边界。无Cargo构建或插件操作。


### CTA横幅及命中

新增契约后累计1923项，核对渲染各phase与宽度阈值，测试仅源码审阅。无Cargo构建。


### CTA测试和输入证据补充

1923项不变，补横幅四测试、键盘两测试及输入分支来源，记录dismiss持久化失败的本地行为。无Cargo构建或配置写入。


### Follow-up生命周期

新增契约后累计1924项，记录身份分支顺序、16-key FIFO及reload保留规则。未运行Rust测试，无Cargo构建。


### Follow-up入站约束

新增契约后累计1925项，入口全文件hash登记。核对保留数量、字节/字符差异与后台更新返回值；无Cargo构建。


### Follow-up路由与测试边界

1925项不变，补启动期session=None回退、root active判断及畸形单项导致整包拒绝，登记相关测试来源。无Cargo构建。


### Follow-up过渡与FIFO证据

1925项不变，补viewer分发测试和三项缓存源码测试，入口测试文件全读登记。源码审阅不代表动态执行；无Cargo构建。


### Follow-up reload调用链

1925项不变，补load局部准入、adopt顺序及四项源码测试来源；明确不等于完整SessionLoaded动态验证。无Cargo构建。


### Follow-up chip投影

新增契约后累计1926项，核对前缀停止、显示截断与提交文本映射；无Cargo构建。


### Follow-up literal提交

1926项不变，补literal提交入口和四项源码测试来源，明确reconnect/未绑定session的chips清理边界。无Cargo构建。


### Follow-up排队与保留草稿

1926项不变，补发送尾部及三项源码测试，区分无SendPrompt与无入队。无Cargo构建。


### Agent建议门槛

新增契约后累计1927项，agent_view/cta.rs全文件阅读登记；无Cargo构建。


### Prompt suggestion状态与模型hint

新增契约后累计1928项，核对generation、前缀与实际resolve_model；无Cargo构建。


### Prompt suggestion测试证据

1928项不变，控制器14项测试全读、文件hash登记；明确未动态执行及配置解析覆盖缺口。无Cargo构建。


### Prompt suggestion请求结果链

新增契约后累计1929项，核对完成尾段准入、RPC投影和result routing，无Cargo构建或网络请求。


### Prompt response准入

1929项不变，补完整response handler前置条件与建议请求关联；无Cargo构建。


### 未确认输入恢复

新增契约后累计1930项，核对composer与review队列两分支；无Cargo构建。


### Compact请求完成投影

新增契约后累计1931项，核对前后台反馈、收尾与未知结果分类，无Cargo构建。


### Compact证据和全局结构复核

1931项不变，补两项源码测试及直接分发来源。1931映射唯一、delta标题/来源存在、逐crate集合一致，900份登记SHA256全匹配；不证明未读文件或所有场景语义。无Cargo构建。


### Shell建议解析

新增契约后累计1932项，核对严格结构与range解析边界；无Cargo构建。


### Shell建议状态与清理

新增契约后累计1933项，核对下拉选择和ghost清理门槛；无Cargo构建。


### Shell completion替换边界

新增契约后累计1934项，核对range/context验证与Tab决策，不宣称已覆盖调用方实际文本修改。无Cargo构建。


### Shell suggestion输入与landing

新增契约后累计1935项，生产mod全文件hash登记；独立tests仍需审阅。无Cargo构建。


### Shell ghost测试证据

1935项不变，补18项接受/progressive源码测试覆盖与代表来源，未动态运行，无Cargo构建。


### Shell建议失效测试证据

1935项不变，补输入抑制与debounce源码证据，记录测试因果与环境前提。无Cargo构建。


### Shell建议landing测试证据

1935项不变，补controller及JSON解析测试来源，记录fixture预清理与绕过parser的边界。无Cargo构建。


### Shell wire与splice测试核对

1935项不变，补五项解析和七项range源码测试证据，明确混合非法字段样例及多候选拒绝的覆盖限制。未运行Rust测试，无Cargo构建。


### Shell suggestion测试文件完整审阅

完成tests.rs剩余源码测试，补规范来源并登记全文件SHA256；1935项不变。明确fixture预清理、手动锚定及controller级pipeline的覆盖边界。未执行Rust测试，未生成Cargo产物。


### Completion dropdown源码审阅

新增渲染契约后累计1936项；完整文件hash登记，测试证据为静态审阅，无Cargo构建。


### Shell completion view契约

累计1937项，已核对生产executor与请求构造，后续继续输入路由和atomic element边界。未执行Rust测试。


### Prompt补全写入边界

新增契约后1938项，核对元素重叠判定和八项源码测试；未动态执行，无Cargo产物。


### Shell completion键盘证据

1938项不变，完整文件hash及28项测试来源已登记。按键、effect和手动landing边界明确，未声称RPC端到端通过；无Cargo构建。


### Shell异步请求投影

累计1939项，补传输、debounce和landing边界；失败pending债务单独记录，未混入实现，无Cargo构建。


### CLI completion生成

累计1940项，静态脚本生成和Zsh修正规则已入delta，完整文件hash登记。无Cargo构建。


### 全局映射与源码校验值复核

核对1940项映射键唯一、对应delta标题和source路径存在、每crate条目集合一致（尚无requirements字段的待审crate按空集合处理）；905份登记SHA256全部匹配。该结构检查不证明未登记文件覆盖或所有契约语义，无Cargo构建。


### Root回合收尾投影

累计1941项，完整root文件hash登记；通知与drain顺序按代码记录，无Cargo构建。


### Terminal marker hook投影

累计1942项，明确stash身份匹配和无marker时独立展示行为，finalizer其余规则继续核对，无Cargo构建。


### Agent terminal生产实现

累计1944项，完整生产文件hash登记；身份、收尾、输出优先级和late metadata已入delta，专用测试继续核对，无Cargo构建。


### Terminal测试首段

1944项不变，补11项完整源码测试的规范来源，后续marker mapping与metadata测试继续。未动态运行，无Cargo构建。


### Terminal完整测试源码审阅

1944项不变，补六项测试来源并登记657行文件hash；响应payload、冲突字段和通知覆盖限制已记录，未动态执行，无Cargo产物。


### ACP终态通知源码测试

1944项不变，补通知分发层的exact/stale终态测试来源，96行文件hash登记；queue snapshot两个测试边界另记review，无Cargo构建。


### Trajectory与Export入口

累计1946项，两文件完整hash登记；记录会话选择、回放投影和输出分支边界，未动态运行。


### Sessions CLI静态审阅

累计1947项，列表分组、搜索参数与删除分发入delta，文件hash登记；未访问真实会话或执行删除。


### Memory CLI清理契约

累计1948项，scope、确认和部分失败返回规则入delta，完整文件hash登记；未执行任何清理操作。


### Trace CLI归档规则

累计1949项，完整文件hash登记；压缩大小检查时机、metadata及本地输出契约入delta，未生成实际归档。


### Startup warning选择

累计1950项，完整文件及五项源码测试核对、hash登记；无Cargo构建。


### 教程目录契约

累计1951项，目录与四项测试源码已核对、文件hash登记；无Cargo构建。


### How-to目录与提取

累计1952项，完整文件hash登记；八项源码测试及磁盘写入/清理边界记录，未实际提取文档。


### Async wake与模型列表

累计1954项，三份小文件完整hash登记，生产调用和单项wake测试源码核对；未动态运行。


### TOML编辑边界

累计1955项，八项测试源码及完整文件hash登记；读取失败与解析失败区别明确，未执行真实配置写入。


### Motion帧时间契约

累计1956项，完整文件hash及五项测试源码核对，记录采样测试与字符串扫描的证明范围。无构建产物。


### 内存释放接口

累计1957项，完整文件hash与单项测试源码已核对，明确hook和trace分离；未触发实际allocator purge。


### Memory trace采样接口

累计1958项，平台gauge和provider安装边界已入delta；启用与sink主体继续，未动态运行。


### Memtrace阈值与写入

累计1959项，hysteresis与旧计数轮转条件已入delta；不将轮转参数声称硬磁盘上限，无构建产物。


### Memtrace启动与dump契约

累计1960项，环境值、后台线程与dump报告边界入delta，测试支持和测试待审，无Cargo构建。


### Memtrace完整源码测试审阅

1960项不变，五项测试来源和728行文件hash登记，修正注释与断言覆盖的区别；无Cargo构建。


### OSC8 route投影

累计1961项，完整文件hash登记及16项测试静态审阅，未发OSC序列或打开链接。


### Unified log基线审阅

累计1962项，当前分支完整文件hash登记；与其他任务报告的main修复隔离，未宣称本分支已修复或已验证交付，无Cargo构建。


### 1962项全局结构复核

1962项映射键唯一、delta标题和源码路径存在、逐crate条目集合一致；926份已登记SHA256全部匹配。只覆盖登记文件及结构，不证明所有场景语义或全crate完成。无Cargo构建。


### 输入诊断ring

累计1963项，完整文件hash和七项测试源码核对；raw与导出脱敏边界明确，无Cargo构建。


### Git cwd缓存契约

累计1964项，TTL预占、刷新结果及通知覆盖边界已入delta，具体Git发现与测试继续核对。


### Git发现与字体规则

累计1965项，分支/标签/路径缩写与glyph政策入delta，源码测试待继续，无Cargo构建。


### Git info完整测试源码

1965项不变，10项测试来源和537行文件hash登记；缓存与glyph用例的证明范围已明确。


### Pager根模块与剩余范围

1965项不变，lib.rs完整hash登记；核对pager-render重导出和minimal路径映射，记录13份顶层未完整审阅文件。目录内部覆盖仍需继续，不以顶层列表替代crate完整审计。无Cargo构建。


### 测试辅助隔离复核

1965项不变，test_util.rs完整hash登记；记录全局env/OnceLock及断开channel的fixture边界，为后续选择动态验证范围提供依据。无Cargo构建。


### Wrap启动入口

累计1966项，完整文件hash登记；shell/direct/fallback选择入delta，独立测试及PTY实现继续。


### Wrap引用测试证据

1966项不变，11项测试来源和完整文件hash登记；实际shell往返测试仅读源码，未动态执行。


### PTY启动边界

累计1967项，启动顺序及单writer队列入delta；输出/退出和错误回收仍待核对，未执行子命令。


### PTY输出与恢复边界

累计1969项，pty_wrap.rs完整hash及两项等待测试源码登记；未执行Cargo构建或PTY动态测试。


### Wrap模式跟踪与恢复字节

累计1970项；wrap_restore.rs完整hash及17项测试源码已登记。未运行Cargo或真实终端验证。


### Wrap图片协议

累计1971项，图片协议完整hash和10项测试源码登记；请求与响应及压缩失败边界入delta，未运行Cargo或剪贴板交互。


### Wrap输出过滤完整审阅

累计1972项；完整文件hash和28项测试源码登记。无Cargo构建，无真实OSC输出或剪贴板访问。


### Wrap图片调用方

累计1973项；请求失败提示与接收当前输入归属入delta，部分源码证据登记，无Cargo构建或动态粘贴。


### 本地草稿存储

累计1974项，存储事实入delta；local_drafts.rs仍待测试部分和调用方核对，无Cargo构建。


### 本地草稿runtime完整审阅

累计1975项，local_drafts.rs完整hash及9项测试源码登记，调用方片段核对。无Cargo构建或动态文件测试。


### Scrollback统计模块

累计1976项，完整文件hash及9项测试源码登记；聚合范围与未发现外部调用明确区分，无Cargo构建。


### Tracing生产路径

累计1977项，生产路径初审事实入delta；测试待继续，无Cargo构建。


### Tracing完整审阅与证据复核

1977项不变，tracing.rs的41项测试源码（含2项ignore）已审阅。全局映射键唯一、所有delta标题和来源路径存在，939份登记SHA256均匹配；这不替代语义或动态验证。无Cargo构建。


### MCP CLI初审

累计1978项，list/add事实入delta，remove/enable/doctor与测试继续，无Cargo构建或真实配置写入。


### MCP CLI完整审阅

累计1979项；mcp_cmd.rs完整hash和18项测试源码登记，无Cargo构建或真实MCP变更。


### Plugin CLI清单

累计1980项，清单事实入delta；安装与管理路径继续，无Cargo构建。


### Plugin安装管理

累计1981项，安装及管理结果边界入delta，tag/marketplace继续，无Cargo构建。


### Plugin源管理

累计1982项，tag与marketplace事实入delta，测试待继续，无Cargo构建。


### Plugin CLI完整证据

1982项不变，plugin_cmd.rs完整hash与8项测试源码登记；pager顶层剩diff.rs，子目录范围仍未完成，无Cargo构建。


### Diff构建初审

累计1983项，构建与拼接事实入delta；提取与测试待继续，无Cargo构建。


### Diff提取与patch

累计1984项，提取优先级与patch编码边界入delta，测试待继续，无Cargo构建。


### Diff完整测试源码

1984项不变，diff.rs的29项测试源码及完整hash登记。未运行Cargo或git apply；顶层覆盖不代替crate完成。


### Diff调用方

累计1985项，相邻Edit与copy契约入delta，未运行Cargo或实际剪贴板复制。


### ACP metadata

累计1986项，meta.rs完整hash与7项测试源码登记；子目录覆盖重新核对，无Cargo构建。


### ACP worker生命周期

累计1987项，spawn.rs完整hash和4项测试源码登记，无Cargo构建或agent启动。


### Leader bridge

累计1988项，完整hash与6项async测试源码登记，未运行动态重连。


### ACP connect

累计1989项，初始化与认证事实入delta，测试待继续，无Cargo构建。


### ACP connect完整测试源码

1989项不变，mod.rs完整hash及17项测试源码登记；未执行初始化RPC或Cargo。


### Tracker活动投影

累计1990项，tracker部分源码事实入delta，无Cargo构建。


### Tracker切流与结束

累计1991项，生命周期事实入delta，无Cargo构建。


### Tracker消息块

累计1992项，消息/思考及tool start事实入delta，无Cargo构建。


### Tracker工具更新

累计1993项，工具更新事实入delta，无Cargo构建或动态工具调用。


### Tracker用户回声

累计1994项，新增用户回声、组合展示、隐藏检查顺序及meta解析契约。源码审阅，未执行动态测试；无Cargo构建。


### Tracker工具结果投影

累计1996项，新增两项源码契约及六个场景；本轮仅静态审阅，未执行工具调用或Cargo构建。


### Tracker分类与解析

累计1999项，新增三项契约及九场景，均基于源码审阅；未执行Cargo构建或动态测试。


### Tracker测试源码第一段

审阅2402–3199的36个测试及辅助构造器，覆盖范围与断言不足已写reviews/pager.md。1999项契约不变；这是静态测试源码核对，不是36项测试运行通过。本轮无Cargo构建。


### Tracker测试源码第二段

新增核对13个测试源码，累计49个。特别区分production_execute_sequence注释的第二次Completed与实际只发送一次的实现；UTF8测试为decoder单测，不构成tracker增量流集成验证。未运行动态测试，契约1999项不变。


### Tracker测试源码第三段

新增15个测试源码核对，累计64个。Edit合并、replay高亮队列、手动折叠及切流覆盖与缺口已写审计记录；未运行测试，1999项契约不变。


### Tracker测试源码第四段

新增21个测试源码核对，累计85个；切流与活动等待的断言范围已入reviews/pager.md。未运行动态测试，契约1999项不变。


### Tracker测试源码第五段

新增10个测试源码核对，累计95个。等待清理、ids更新和标签断言已登记；未运行动态测试。


### Tracker测试源码第六段

新增14个测试源码核对，累计109个；子Agent等待及能力快照的覆盖范围已登记。未运行动态测试，契约1999项不变。


### Tracker测试源码第七段

新增15个测试源码核对，累计124个；补齐meta-less保留tools的直接断言证据。未运行动态测试，契约1999项不变。


### Tracker测试源码第八段

新增7个测试源码核对，累计131个；后台Execute及用户消息收尾证据已登记。未运行动态测试，契约1999项不变。


### Tracker完整源码核对

末段新增12个测试，累计143个，与文件测试属性数量一致；6003行生产代码和测试源码逐段核对完成，登记SHA256。仅静态核对，未运行143个测试。契约1999项不变，无Cargo构建。


### Notification progress与tmux

累计2000项；两文件完整源码及20个测试源码已核对并登记hash，未运行测试或终端输出验证。


### Notification焦点状态

累计2001项；focus.rs完整源码及17个测试源码核对登记，未运行动态焦点或recap请求测试。


### Notification协议

累计2002项，protocol.rs完整源码与24个测试源码核对，登记hash；未运行桌面通知或动态测试。


### Notification配置

累计2003项，完整配置源码及10个测试源码核对登记；未运行动态测试。


### Notification hooks

累计2004项，hooks.rs完整源码与9个测试源码核对登记；未执行hook或动态测试。


### Notification服务入口

累计2005项，生产服务行为入delta；测试模块待审，无动态通知验证。


### Notification服务完整核对

mod.rs全文件及30个测试源码完成登记，补齐load配置整体回退事实。2005项不变，未运行动态测试。


### Notification休眠抑制

累计2006项，sleep.rs完整源码与平台条件测试源码已核对，未运行实际休眠抑制或动态测试。


### Notification标题生产逻辑

累计2007项，title生产行为入delta，测试模块待审；未发送标题序列或运行动态测试。


### Notification标题测试

title.rs完整文件及35个测试源码核对完成登记；notifications九文件源码登记齐全。未运行动态测试，契约2007项不变。


### Search基础模块

累计2008项，search两文件及8个测试源码核对登记；未运行动态测试。


### Project picker

累计2009项，两文件及3个测试源码核对登记；未读取用户session历史或运行动态测试。


### du命令生产代码

累计2010项，三文件源码已核对登记；测试文件待审，未运行真实磁盘扫描或Cargo构建。


### du测试核对

8个测试源码核对登记；du四文件完整审阅。记录Windows测试helper条件编译疑点，未运行跨平台编译，契约2010项不变。


### Worktree CLI生产入口

累计2011项，生产分发契约入delta；测试与展示待审，未运行worktree操作。


### Worktree CLI测试

mod.rs全文件与16个测试源码核对登记；未运行动态测试。展示文件仍待审，2011项不变。


### Worktree展示

累计2012项，display全文件及10个测试源码核对登记，未运行动态测试或worktree操作。


### 全量已登记证据一致性复核

当前966条reviewed_files SHA256全部匹配本worktree；2012项feature-map的crate/requirement组合无重复，所有requirement标题存在于对应delta。此检查不证明未登记文件已覆盖，也不代替动态测试。下一步继续doctor_cmd未登记源码。


### Doctor CLI入口

累计2013项，入口源码完整登记；未运行doctor修复或动态测试。磁盘本轮检查剩余68GiB。


### Doctor人类输出

累计2014项，human formatter完整源码登记；测试待审，未运行动态探测。


### Doctor JSON

累计2015项，JSON formatter全文件登记；测试待审，未运行动态诊断。


### Doctor测试第一段

前4个测试及fixture源码核对，shared view与human精确输出证据已登记；未执行测试，2015项不变。


### Doctor测试完整源码复核

完整核对doctor_cmd/tests.rs的17个测试并登记SHA256，累计2015项契约不变。精确文本/JSON结构、取消与非TTY保护、映射和writer失败的断言及缺口已写入reviews/pager.md；未运行动态测试。


### Minimal hook

累计2016项契约；完整登记hook源码及核对调用点，未运行Cargo或终端绘制测试。minimal/api.rs仍待分段审阅。


### Minimal BTW生命周期

累计2017项；核对请求关联、悬挂恢复及调用方，记录匹配但非Loading时take副作用。仅源码核对，未执行Cargo测试。


### Minimal请求与按键分流

累计2019项；补充transcript请求/展开重试和Ctrl+O所有权，核对一个六状态谓词测试源码。未执行Cargo，完整api文件仍待核对。


### Minimal API完整证据登记

累计2020项；minimal/api.rs完整1148行已分段读取并登记SHA256，未执行Cargo测试。wrapper审阅不替代共享实现覆盖。


### Settings registry搜索和模型选择

累计2021项；注册、搜索、模型选择及模式规范化已核对源码，未运行测试。registry完整文件尚未登记，下一段从481行继续。


### Settings值来源

累计2022项；完整核对current/default生产映射并记录cache与snapshot边界。registry测试仍在阅读，未运行Cargo。


### Settings registry测试复核完成

完整登记registry.rs，19个测试源码核对，2022项契约不变。记录测试真实覆盖与名称过度概括之处；未运行Cargo。


### Settings静态选项目录

累计2023项；defs.rs前440行已审，目录声明与行为执行分开记录。下一段从441继续；未运行Cargo。


### Settings目录完整登记

累计2024项；defs.rs完整审阅并登记hash，全部40项metadata列入pager审计表。settings三文件均完成源码审阅，modal和dispatch仍需独立核对，未运行Cargo。


### Pager-owned设置写入证据

2024项不变，新增respect_manual_folds从setter到异步effect及blocking持久化入口的源码审计证据，确认成功toast在写入完成前显示。底层文件写入和失败回滚仍待追踪，未执行Cargo或真实配置写入。


### 设置回滚完成处理

累计2025项；新增无版本回滚和best effort错误语义，失败并发风险登记backlog，未实施修复或运行Cargo。


### Settings rollback映射

累计2026项；完整核对rollback函数分派，记录无法恢复、未知枚举及类型错误边界，未运行动态回滚测试。


### Settings窗口快照归属

累计2027项；核对open/refresh，确认模型使用app新session模板，非agent当前catalog。ui.rs 275之后reset段待审，未执行Cargo。


### Settings reset确认

累计2028项，完整核对reset确认分支；源码证据已记录，未执行动态测试或配置写入。


### Settings快捷切换证据

累计2029项，补齐compact/vim/timestamps/mouse入口语义，未运行Cargo或改变终端状态。


### Settings UI完整登记

累计2030项；ui.rs全文件已读并登记hash，reset action转换边界补入delta。未运行Cargo测试。


### Settings setter幂等语义

累计2031项；核对显式覆盖、effective判等和vim递归边界，未执行Cargo。


### Settings分组失效范围

累计2032项；核对thinking/group/suggestion/selection和timeout setter的状态修改边界，未运行Cargo。


### Scroll设置

累计2033项，新增clamp、重建配置与None回滚丢失语义；仅源码核对，未运行Cargo。


### 审批光标和显示setter证据

2033项不变，新增609–825行源码审计证据，记录timestamps/timeline判等来源差异及审批光标注释待核对点。未运行Cargo。


### Simple和hint设置

累计2034项，新增输入模式协调、hint显式覆盖及队列合并setter边界；未运行Cargo。


### Theme设置

累计2035项，核对theme preview/commit与auto_dark commit，未执行Cargo或真实主题预览。


### 自动主题分支补齐

2035项不变，扩展theme契约包含全部auto_dark/light commit与preview源码。未运行Cargo或实际系统主题探测。


### 模型设置目录归属

累计2036项，核对默认与fork模型设置/清除及目录差异，未执行Cargo。


### Settings setters全文件证据

累计2037项；完整登记setters.rs，CLI默认值与回滚helper差异已记录，未运行动态测试。


### Show tips测试覆盖边界

2037项不变；核对六项show_tips测试源码并登记settings dispatch mod文件，测试未执行。现有局部断言不覆盖registry默认值差异。


### Settings分派测试首段

2037项不变，完整核对前五项测试并记录真实覆盖；发现同文件含CTA与cancel测试，后续按实际内容追踪。未运行Cargo。


### Settings测试第二段

2037项不变，补充六项完整测试的静态覆盖说明，区分RPC接纳与权威模型确认、resize状态与effects断言。未执行测试。


### Compact和显示测试覆盖

2037项不变，完整追加十项测试源码覆盖记录。真实热加载、磁盘写入和渲染不由这些状态断言证明。未运行Cargo。


### Settings窗口测试覆盖

2037项不变，新增六项窗口测试的静态审计记录，明确debug/release分支、CreateSession effect与实际创建差别。未运行Cargo。


### Preview撤销测试边界

2037项不变，六项完整测试证据入审计；明确转发恢复action不等于显示已恢复。未运行Cargo。


### Reset守卫真实覆盖

2037项不变，核对七项测试，明确rollback守卫只排除缺arm提示，不证明完整恢复。未执行测试。


### Reset测试前置条件审计

2037项不变；完整核对move-away helper，确认部分条目没有建立非默认前置状态，不能把reset守卫视为全设置有效恢复证明。未运行Cargo。


### Simple回滚companion测试

2037项不变，九项测试源码证据追加，确认QueueReleaseEdit效果被保留的精确断言与未执行RPC边界。未运行Cargo。


### Multiline作用域测试

2037项不变，六项完整测试的覆盖范围入审计，包括Dashboard专用slash路径。未运行Cargo。


### Pager settings tests 1900–2380 静态复核

补入十三项完整测试的断言覆盖与限制：Vim、selection、thinking、tool grouping及布局header/高度变化。未运行Cargo，不把静态审阅计为测试通过。OpenSpec strict all验证15/15通过，git diff --check通过。迁移工作树无target目录，本段无构建产物可清；磁盘剩余69 GiB。settings测试文件仍未读完，不登记整文件hash。


### Settings测试文件静态审阅完成

settings.rs全部3331行已分段阅读，新增整文件SHA256；清单979个已审阅文件hash逐个与当前工作树复核，0不匹配。剩余测试断言与theme fixture局限写入pager review；未运行Cargo。OpenSpec strict all 15/15通过，git diff --check通过。此结果只证明文档结构及已记录源码快照一致，不代表所有crate审阅完成或运行时测试通过。


### Settings render 1490–2055静态复核

补入整数步长、String/Int编辑渲染与max_thoughts_width实时预览契约。未运行Cargo，render.rs尚未完整阅读，因此不登记整文件hash；本段完成后重新运行OpenSpec strict all及git diff检查。


### Settings modal tests首段静态复核

tests.rs 1–620已逐行阅读，记录类型映射守卫的真空与弱断言边界、顶层行顺序覆盖及group测试范围。未执行测试，文件尚未完整阅读且不登记hash；这些测试只能作为现有契约的静态证据，不能计为运行验证。

tests.rs 620–1181已逐行阅读，补充基础键盘、关闭键、filter、鼠标hover和状态转换测试的实际断言边界。仍未执行Cargo测试，文件未完成，因此不登记hash。

tests.rs 1182–1748已逐行阅读，记录滚轮、restart pill、合成String编辑器与Int箭头命中区域的Buffer断言范围。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 1775–2220已逐行阅读，记录整数步长边界、键鼠步进、提交取消及theme Esc动作映射的断言范围。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 2221–2811已逐行阅读，记录合成picker导航、真实theme深链接动作、Buffer布局及提交标记测试的证据边界。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 2812–3213已逐行阅读，记录枚举初始值回退、零尺寸/短视口与长描述测试的弱断言和过时注释；ignored视觉输出不计验证。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 3214–3870已逐行阅读，记录多行choice、hit rect、可变高度滚动、截断和overflow提示测试的断言强弱。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 3871–4315已逐行阅读，记录顶层picker路由、空geometry鼠标、合成String编辑、过滤与隐藏selection恢复测试的覆盖边界。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 4315–4824已逐行阅读，记录分类间距、row rect纵向对齐、description wrap、单双行阈值及Group展开测试的覆盖边界。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 4825–5384已逐行阅读，记录footer空行、palette样式、过滤清理、粘贴过滤、Unicode search及Bool配色测试的覆盖范围。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 5385–5913已逐行阅读，记录chevron跨类型/单双行对齐、docs footer三档宽度、Tip上方空行及picker长subtitle测试范围。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 5914–6480已逐行阅读，记录breadcrumb几何/退出/reset、hover状态及搜索divider测试范围，并与render调用顺序交叉核对出hover绘制偏差。未执行Cargo测试，文件仍未完成且不登记hash。

tests.rs 6481–7482已逐行阅读并完成全文件登记，补全最大思考宽度预览的样式、30列/5行预算边界、夹宽提示、pending实时重排，以及仅该编辑态触发的terminal减8扩宽和退出恢复规则。该文件SHA-256为`fa94d5ed521354ed894fd2f9b79581be215bd791a1399d2b4a435d42d189ccf8`。无Cargo构建；工作树无target目录。

完整静态读取pager的Cargo.toml、build.rs、actions/defaults.rs与actions/mod.rs并登记hash，新增4项包feature、构建provenance、registry精确路由及终端/模式键位契约。验证范围不含真实终端探测、键盘协议、构建脚本执行或Cargo测试。

完整静态读取pager `app/actions.rs` 2632行并登记SHA-256 `6dc6ac412073a8d3a10c5dda218bab326841cf239762edf6044919756c9512ec`。新增3项Action/Effect/TaskResult协议、clipboard completion优先级与permission mode类型策略契约；未把枚举注释单独当作各effect执行成功的证据，无Cargo构建。

完整静态读取pager bundle、roster及display-refresh startup共481行并登记3个hash，新增3项严格bundle schema、result-envelope roster解析及同步/异步刷新探测计划契约。测试证据仅为文件内现有单测和穷尽分支阅读，未运行Cargo或真实终端探测。

完整静态读取pager signal_handler与external_editor共850行并登记hash，新增2项TUI双阶段signal shutdown及外部编辑器所有权/文件上限契约。验证依据为源码分支与文件内测试；未发送真实信号、启动editor或运行Cargo。

完整静态读取pager csi_filter 741行并登记SHA-256 `14a87455938b0b4a83ce5358e201fd95ff4e96e9d837a6b16574fc351c717890`，新增SGR/focus fragment过滤契约，并把跨batch focus泄漏登记backlog。未运行Cargo或真实SSH/terminal输入验证。

完整静态读取pager xt_filter 610行并登记SHA-256 `53535903d6349039e4f86eb287c390b36b4cfd3bc72a1fd8897ebff6cf1293df`，新增XTVERSION arm、语法、FIFO与bounded hold契约。文件内测试只作静态审阅，未运行Cargo或真实terminal probe。

完整静态读取pager session/activity 310行并登记SHA-256 `a19835bb760b2fc7738c85a0130f76a035cc693585d63c11040ba246aa1442da`，新增只读activity聚合、working与animates差异契约。未运行Cargo。

完整静态读取pager screen_mode_relaunch 884行并登记SHA-256 `1fe81dae635fd724f3fc2ec9c3718c0c8151ebadeecdbebc2a36f37b4a339c6a`，新增argv重建与one-shot mode/process replacement两项契约。未实际exec、spawn Windows child或运行Cargo。

完整静态读取pager leader_cluster的mod.rs 617行与scenarios.rs 390行并登记两项SHA-256，新增in-process真实leader/agent夹具生命周期及四项多client场景实际断言范围两项契约。审阅确认名为both_ways的用例第二轮仍由原driver发送，缺少viewer反向驱动，已单独登记backlog。四项测试均为Unix-only ignored，本轮未运行Cargo；工作树target不存在，无构建缓存可清。

完整静态读取pager app/cli.rs 1173行并登记SHA-256，新增CLI命令与冲突拓扑、agent transport/plugin参数、启动路径锚定和恢复sandbox/prompt准入四项契约。既有resume pin与completion条目保留且未重复；文件内Clap/helper测试仅作静态证据，本轮未运行Cargo。

完整静态读取pager slash/commands下27个小文件共859行，逐文件登记SHA-256，新增基础Action/session gate、显示偏好切换、side request/memory/compact payload、本地参数校验/release notes与effort dropdown五项契约。验证范围只含源码分支和极少量文件内测试的静态阅读，未运行命令或Cargo。

继续完整静态读取pager slash命令8文件共609行并登记SHA-256，新增help/feedback/navigation/status入口与workflow暂存/default guide两项契约。文件内测试只作静态阅读，未运行Cargo或真实browser/workflow操作。

完整静态读取pager slash命令6文件共556行并登记SHA-256，新增modal/transcript/minimal expansion gate、workflow-run selector/host route及permission建议/执行gate三项契约。未运行Cargo或实际修改permission、启动pager与workflow。

完整静态读取pager slash命令6文件共661行并登记SHA-256，新增minimal editor/extension tab路由、direct permission shortcuts、multiline快照toggle及mouse reporting配置gate四项契约。Auto直接命令是否受统一执行层gate仍待mod/registry审阅；本轮未运行Cargo。

完整静态读取pager slash命令4文件共513行并登记SHA-256，新增dashboard location/find输入与agents/screen-mode relaunch路由两项契约。路径处理、外部feature gate和无session-ID Action边界均按源码记录；未运行Cargo或进程relaunch。

完整静态读取pager slash命令4文件共763行并登记SHA-256，新增announcement首token、copy ordinal/path、Plan deferred prompt及Agent discovery/switch四项契约。未运行Cargo或实际clipboard、文件、behavior及agent操作。

完整静态读取pager slash命令4文件共889行并登记SHA-256，新增debug profile可见性、docs别名/guide路由、doctor live report/fix语法及export path completion四项契约。未运行Cargo、live probe、browser或文件导出。

完整静态读取pager slash behavior与fork共664行并登记SHA-256，新增Behavior availability/selection及fork leading flags/directive两项契约。fork注释与尾随空白实现差异已登记backlog；未运行Cargo或真实fork。

完整静态读取pager slash loop_cmd.rs 372行并登记SHA-256，新增provisional compact interval解析、共享scheduler instruction及required tool契约。未运行Cargo、模型或scheduler任务。

完整静态读取pager slash effort.rs 378行并登记SHA-256，新增current model先验、菜单驱动effort建议/解析及PatchEffort路由契约。未运行Cargo或实际sampling配置变更。

完整静态读取pager slash model.rs 549行并登记SHA-256，新增catalog-only模型候选、最长ID优先的二阶段effort建议、完整ID优先解析及菜单约束的可选effort切换两项契约。未运行Cargo或实际模型切换。

完整静态读取pager slash theme.rs 584行并登记SHA-256，新增auto/具体主题候选及canonical SetTheme提交、瞬时preview/cancel恢复两项契约。未运行Cargo、终端主题预览或配置持久化。

完整静态读取pager slash/commands/mod.rs 787行并登记SHA-256，新增71项builtin有序目录、零alias、隐藏命令精确注册、Shell名称保留及registry工具/服务可见性集成契约。该文件没有统一执行gate；未运行Cargo或实际registry执行。

完整静态读取pager slash/registry.rs 1192行并登记SHA-256，新增hard/menu/tier/tool分层lookup、ACP replacement/collision qualification及trigger投影两项契约。确认Auto hard gate覆盖typed dispatch；工具集unknown注释与fail-closed实现差异已登记backlog。未运行Cargo或真实ACP同步。

完整静态读取pager slash/command.rs 451行并登记SHA-256，新增trait元数据默认/分类/完整性位、suggestion与execution context所有权及CommandResult/host invocation词汇三项契约。未运行Cargo或dispatch链。

完整静态读取pager slash/mod.rs 3066行并登记SHA-256，新增controller默认/MRU、command建议排序/tag/ghost、leading解析与静态完整性、inline token/args/highlight及surface/mode offering五项契约。两处注释与实现差异已登记backlog；未运行Cargo或真实composer交互。

完整静态读取pager slash/matcher.rs、mode_support.rs及mode_support_tests.rs共432行并登记SHA-256，新增Nucleo Smart rank/highlight状态与screen mode支持/refusal矩阵两项契约。未运行Cargo、relaunch或真实按键。

完整静态读取pager slash/mru.rs 381行与acp_command.rs 875行并登记SHA-256，新增MRU load/decay/cap、snapshot/serialized persistence、ACP skill/taxonomy投影、host/raw-skill执行及Goal状态建议五项契约。ACP注释漂移与MRU跨进程写风险已登记backlog；未运行Cargo或真实磁盘/host/skill操作。

集合校验确认`crates/codegen/pager/src/slash`根目录8个Rust文件及`slash/commands`目录66个Rust文件全部存在于pager reviewed_files并通过当前源码SHA-256复核，slash模块共74个文件无漏审。该结论只覆盖slash模块，不扩大为pager crate或全仓完成。

完整静态读取pager input的mod.rs、macos_modifiers.rs、terminal_support.rs及scroll_log.rs共448行并登记SHA-256，新增modified Enter/macOS救援、scroll recorder enable/lazy failure isolation及transition schema/timing三项契约。同秒日志路径风险已登记backlog；未运行Cargo或真实OS/input/log IO。

完整静态读取pager input/key.rs、keyboard_normalizer.rs及line_editor.rs共1147行并登记SHA-256，新增shortcut normalize/match/display、paste/ShiftTab/AltGr/text predicates、modifier rescue、single-line sanitation/paste/grapheme及key outcome/viewport五项契约。BackTab compact label重复风险已登记backlog；未运行Cargo或真实input/editor交互。

完整静态读取pager input/mouse.rs 1450行及mouse/tests.rs 1879行并登记SHA-256，新增终端/复用器profile与设置投影、流分类和边界、逐事件加速计价与独立时钟、全类型flush cap、coast/finalize/carry及只读诊断/recorder集成五项契约。测试文件仅作静态交叉证据，未执行Cargo或真实terminal/multiplexer/trackpad输入。

集合校验确认`crates/codegen/pager/src/input`的9个Rust文件全部存在于pager reviewed_files并通过当前源码SHA-256复核；该结论只覆盖input模块，不扩大为pager crate或全仓完成。

完整静态读取pager `src/bin`下6个playground共1646行并登记SHA-256，新增共享终端生命周期、Mermaid、raw mouse、Question/Todo、scrollback search与selection六项开发工具契约。它们无断言且本轮未执行，不能计作运行或视觉验证；终端恢复缺少RAII及Mermaid退出键注释漂移按已有backlog开发工具项保留。

集合校验确认`crates/codegen/pager/src/bin`的6个Rust文件全部存在于pager reviewed_files并通过当前源码SHA-256复核；该结论只覆盖playground binaries。

完整静态读取pager `benches`下4个Rust文件共1069行并登记SHA-256，新增edit highlight、render/reveal、resize与search四项Criterion workload契约。未运行基准；源码没有性能pass/fail阈值，search异步结果交付不在相应timed closure中，因此不声称性能达标。

集合校验确认`crates/codegen/pager/benches`的4个Rust文件全部存在于pager reviewed_files并通过当前源码SHA-256复核；该结论只覆盖Rust benchmark入口，不包含bench.md数据文件。

完整静态读取pager tests顶层11个较小入口共623行并登记SHA-256，新增GROW_HOME helper、Mermaid ignored real-child gate、PTY family拓扑、unknown SSH clipboard和selection public API五项契约。未运行Cargo；PTY子模块仍须逐文件审阅，入口名称及注释不能算子测试证据。

完整静态读取pager tests/pty_xtversion.rs 360行并登记SHA-256，新增9项ignored built-pager PTY矩阵契约。未执行；只能证明测试定义与断言范围，不能声称真实terminal probe已通过。

完整静态读取pager tests/scripted_scenarios.rs 566行并登记SHA-256，新增wrapper执行、场景目录、raw OSC8/Kitty artifact及普通YAML parse gate四项契约。未执行；40个ignored wrapper不算通过，且45份YAML中仅29份由本文件普通parse tests具名覆盖，YAML内容仍待逐份读取。

完整静态读取pager tests/doctor_early_dispatch.rs 726行并登记SHA-256，新增early doctor/du隔离、fix写入安全边界、tmux containment夹具及non-TTY wrap四项契约。14项测试均ignored且未执行。本轮非Cargo隔离探针确认fake-bin PATH下/bin/sh找不到sleep并退出127，反证三项timeout/descendant夹具的活进程前提；该债务已登记backlog。

完整静态读取pager tests/pty_e2e下8个wrap子测试共301行并登记SHA-256，新增direct/shell routing、explicit path failure、OSC52 sink与terminal mode restoration四项契约。全部Unix-only ignored且未执行；仅记录实际断言，不把测试名扩大为完整byte transparency或进程reap证明。

完整静态读取pager tests/pty_e2e下5个folder trust与2个MCP menu子测试共331行并登记SHA-256，新增trust decision、HOME/subdir key及MCP cwd wrapper三项契约。全部ignored且未执行；MCP两项只证明fixture委托，待common.rs读取后才能记录实际菜单断言。

完整静态读取pager `tests/pty_e2e/common.rs` 1267行并登记SHA-256，新增共享语料/trust fixture、MCP menu drive、queue/screen helper、双协议tool-call fixture、OSC52/mouse、Minimal生命周期及wrap exit/诊断七项契约。common helper确认MCP wrapper实际要求菜单标题与`cat-mcp`，但全部相关wrapper仍ignored且未执行；不声称真实MCP、PTY、clipboard、mouse、Minimal或wrap已通过。screen列宽、scrollback footer错误丢弃及首目录session发现三项弱边界已登记backlog。本轮没有Cargo构建或测试。

完整静态读取pager `tests/pty_e2e`五个小用例共214行并登记SHA-256，新增resize触发render、交互/位置prompt到mock response、undo tip seen-count不落config及长响应滚动存活四项契约。所有五项均ignored且未执行；断言范围不证明精确帧、请求body、其它配置未写、scroll位置或完整stream。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`下一组五个用例共285行并登记SHA-256，新增macOS原生文本粘贴时限、idle prompt input wake、undo tip会话cap、mid-turn queued follow-up提示及活动GROW_HOME模型热加载五项契约。全部ignored且未执行；host clipboard恢复限制、经验idle等待、未触发send-now及未选择热加载模型均保留为验证边界。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第三组五个用例共316行并登记SHA-256，新增embedded blocked-backend启动、活动后Ctrl-C不rewind、model等待标签、config effort菜单及Windows原生文本粘贴五项契约。全部ignored且未执行；Windows不可用clipboard会SKIP成功，screen行分类和标签substring均不扩大成完整history或内部状态证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第四组五个用例共348行并登记SHA-256，新增multiline chip payload、scrollback Esc cancel、inline paste payload、mid-turn slash Esc优先级及text-selection settings registration五项契约。全部ignored且未执行；contains/current-screen/任一label断言不扩大成完整payload、历史唯一、turn liveness或seeded value选择证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第五组五个文件共383行并登记SHA-256，新增welcome logo geometry、运行中prompt Esc、idle-empty Esc swallow、idle double-Esc清理/历史及plan revise空Enter五项契约。全部ignored且未执行；motif、screen substring、可见surface及首session目录证据不扩大成阈值、composer ownership、内部pending state或plan tool完成证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第六组五个文件共420行并登记SHA-256，新增continue history/follow-up、macOS Otty IME image suppression、undo tip跨process reset、macOS image paste可见排序及small-screen tip慢turn生命周期五项契约。全部ignored且未执行；平台SKIP、current-screen、visible ordering和固定时点不扩大成ledger、thread ownership、精确TTL或全尺寸证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第七组五个文件共468行并登记SHA-256，新增bash chrome cwd strip、queued bash promotion、embedded focus stale-row repair、Windows image paste ordering及spaced path OSC8五项契约。全部ignored且未执行；current-screen、marker、feed_screen、platform SKIP和raw substring证据不扩大成完整history、真实nested stack、thread ownership或可点击链接证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第八组五个文件共489行并登记SHA-256，新增path-free image preview、pre-activity Ctrl-C rewind、drag autoscroll clamp、paste-immediate-send及page-flip setting五项契约。全部ignored且未执行；per-body计数允许跨请求重试，synthetic mouse/current-screen/held-turn证据不扩大成真实输入、持久history或全部viewport证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第九组五个文件共507行并登记SHA-256，新增forced-wheel exact rows、queued-prompt Ctrl-C wire uniqueness、drag-wheel selection extension、stream Ctrl-C recovery/log markers及same-turn interjection五项契约。全部ignored且未执行；最后Chat body、joined OSC52、current-screen和substring log证据不扩大成全endpoint、真实clipboard、历史唯一或typed diagnostic证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第十组五个文件共543行并登记SHA-256，新增queued bash FIFO drain、usage/context/session modal、cancel-rewind-resend uniqueness、removed queue Chat exclusion及parallel same-file edit merge五项契约。全部ignored且未执行；current-screen、per-body、Chat-only和UI diffstat证据不扩大成durable transcript、全endpoint、queue ack或实际文件修改证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第十一组五个文件共574行并登记SHA-256，新增double-Esc rewind picker、nested quote selection、gap-row drag、blank-chrome whole-block drag及streaming verb fold五项契约。全部ignored且未执行；多个OSC52 join及忽略focus等待形成新/扩展backlog，不能声称单次copy范围或已确认scrollback ownership。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`后续五个文件共666行并登记SHA-256，新增misclassified wheel flood平均帧位移、Read header路径选择、raw quote源码marker、welcome/empty-session状态及dashboard overlay键盘返回五项契约。全部ignored且未执行；两个selection用例拼接OSC52、wheel使用aggregate ratio、dashboard单row与固定Tab等待均作为证据边界记录，不扩大为单次精确copy、逐flush cap或多会话navigation保证。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e/scroll.rs`与四个scroll回归用例共927行并登记SHA-256，新增共享marker/stream fixture、trackpad条件式under-travel、wheel chars/frame floor、bottom overscroll follow及stream中viewport progress五项契约。全部用例ignored且未执行；settled helper忽略focus footer、trackpad低travel低frame成功SKIP、aggregate frame chars、过量down burst及无法强制ACP饥饿均明确保留为验证边界。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`四个endline/wait生命周期用例共799行并登记SHA-256，新增markerless parked FIFO、三任务auto-wake链、重复park marker suppression及resume spinner chrome四项契约。全部Unix-only ignored且未执行；断言主要来自current screen，响应因果、持久记录、短wait结果及spinner精确位置均不扩大保证。前两个用例虽无显式quit，但局部源码核对确认harness Drop有有界kill/wait；这不算graceful quit验证。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`四个Bash交互用例共598行并登记SHA-256，新增输出抽样fold、带引号文件补全、token-only Tab dropdown及queued Bash编辑执行四项契约。全部Unix-only ignored且未执行；full只抽查行、pre-Tab仅即时snapshot、queue成功路径不排除原命令也执行，均按真实证据收窄并将后两项登记backlog。本轮没有Cargo构建、测试、真实PTY或shell执行。

完整静态读取pager `tests/pty_e2e`四个verb-group用例共821行并登记SHA-256，新增混合verb折叠/member导航、header与member选择、settings实时relayout及thinking member fold四项契约。全部ignored且未执行；折回只抽查部分member、单gesture仍可含多个joined OSC52、配置不读回且thought body未展开，均作为验证边界并登记相应backlog。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`三个edit/queue用例共732行并登记SHA-256，新增edit style-run变化与artifact、sequential merge/text break及queued screen/blob唯一性三项契约。全部ignored且未执行；目标syntax color、全部hunk/文件事实及单blob内重复均未被现有断言排除，分别登记backlog。固定/tmp artifact本轮未生成，Cargo与PTY均未运行。

完整静态读取pager `tests/pty_e2e`四个layout/input/style用例共601行并登记SHA-256，新增auto-compact首非空行、Linux Otty image-probe gate、iTerm raw readline及skill fg传播四项契约。全部ignored且未执行；status身份、真实Wayland/iTerm和具体skill accent均不由现有断言证明，auto-compact弱代理另登记backlog。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`三个process/modal/selection用例共559行并登记SHA-256，新增background记录shell PID退出、Plugin contextual hints及pretty quote prefix selection三项契约。全部ignored且未执行；sleep后代未被PID检查、modal不执行变更、OSC52按多payload join，分别按边界记录并扩充backlog。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`四个文件共1406行，其中auto-wake文件含两测试，新增cancel保留/恢复用户prompt、取消后deferred completion、basename demo fallback、width reflow anchor及thinking live toggle五项契约。全部ignored且未执行；marker唯一性、viewer身份、focus、单ASCII anchor与thinking展开均按真实断言收窄，相关弱证据已扩充backlog。本轮没有Cargo构建、测试或真实PTY。

完整静态读取pager `tests/pty_e2e`第十二组五个文件共602行并登记SHA-256，新增gap-entry anchor、Minimal ANSI native scrollback、collapsed edit double-click、above-prompt strip anchor及prompt suggestion ghost五项契约。全部ignored且未执行；忽略focus等待已扩展backlog，text extraction、UI body marker及hint代理不扩大成ANSI style、实际文件修改或suggestion请求身份证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`第十三组五个文件共644行并登记SHA-256，新增Tab focus mode matrix、stuck-drag Esc recovery、recap header exclusion、Linux PRIMARY middle-click及wheel frame amplification五项契约。全部ignored且未执行；recap joined payload已扩展backlog，footer/style/fake-X/frame bounds不扩大成内部focus、真实clipboard或cadence证明。本轮没有Cargo构建或真实PTY。

完整静态读取pager `tests/pty_e2e`最后四个未登记文件共892行并登记SHA-256，新增scrolled-out drag copy、rename border、rename resume/plain对照、scroll HUD env-on、default-off、command toggle、word-select tip accept、mode gate及per-tip opt-out九项契约。全部ignored且未执行；OSC52 join、rename持久化代理、HUD substring/final-state和word-select无选择结果等证据边界均已写入契约并登记债务。本轮没有Cargo构建、测试或真实PTY。

验证脚本曾误以不存在的`crates/pager`统计Rust/PTY缺口而输出假零；复核后统一以crate inventory的真实根`crates/codegen/pager`计算。修正口径下，本批写入前Pager共有646个Rust文件、已登记376个、缺270个，PTY目录122个文件、缺4个；这四个文件现已逐行读取并登记，后续验证必须同时断言根目录存在，禁止空glob被当成全覆盖。

完整静态读取pager `src/app/acp_handler`三个生产模块共275行并登记SHA-256，新增viewer turn anchor、subagent activity projection、orphan synthetic finish、MCP init progress、MCP initialized及server-status modal/refetch六项契约。时间换算fallback、synthetic零指标、inactive redraw与modal状态边界均按源码记录；没有运行Cargo、测试、通知路由、MCP或subagent操作。

完整静态读取pager `src/app/acp_handler/interactions.rs`与`routing.rs`共460行并登记SHA-256，新增ask-user、plan approval、session match priority、notification/MCP owner、session+scrollback借用、active predicate及interactive AgentView resolver七项契约。所有reverse request结果、replacement清理、root/child优先级、race fallback和可见性返回值均按源码分支记录；未运行Cargo、测试、ACP transport或UI交互。

完整静态读取pager `src/app/acp_handler/permissions.rs`共428行并登记SHA-256，新增permission owner/auto-approve、FIFO state、subagent provenance、display fallback、protected edit description、MCP argument bounds及两项recap规则共八项契约。root/child、通知时序、meta退化、200/2000限幅和recap组合条件均逐分支记录；未运行Cargo、测试、权限UI、通知或recap生命周期。

完整静态读取pager `src/app/acp_handler/settings.rs`共496行并登记SHA-256，新增model publication/catalog recursion/deferred retry、Auto gate、soft-default、picker precedence、verb grouping、tips/tags refresh、roster、announcement及settings DTO十一项契约。workflow freeze、配置优先级、presence语义、invalidations与effect/send边界均逐分支记录；未运行Cargo、测试、settings/models通知或外部服务。

完整静态读取pager `src/app/acp_handler/background.rs`共623行并登记SHA-256，新增stdout interception、background demotion、monitor append、scheduled create/fire/delete、child cwd、git head与completion九项契约。累计输出、replay、description priority、unknown-task、自愈、root/child、stale-on-load和redraw边界均逐分支记录；未运行Cargo、测试、进程、scheduler或git操作。

完整静态读取pager `src/app/acp_handler/mod.rs`共852行并登记SHA-256，新增control projection、ACP dispatch、root highwater/meta、special updates、prompt adoption、tracker drains、viewer/behavior lifecycle、child branch、workflow helpers、ExtNotification、interjection及ExtMethod十二项契约。root/child不对称、ack语义、live/replay、redraw与side-channel边界均逐分支记录；未运行Cargo、测试、ACP或UI生命周期。

完整静态读取pager `src/app/acp_handler/session_notification.rs`共2095行，第一阶段完成13项契约及delta spec映射，但按完整性门槛暂不登记文件SHA。阶段范围为notice/replay/context/Grow routing、subagent permission与spawn/progress/finish、turn terminal、hooks及plugin/hook catalog；剩余分支将在下一阶段补齐后统一登记。未运行Cargo、测试或通知。

完成pager `src/app/acp_handler/session_notification.rs`第二阶段17项契约与delta spec映射，并以SHA-256 `8169033b5b744c4b00d8d820cec370feffcc8761e3c660c4337d06b01819a484`登记reviewed_files。两阶段合计覆盖全2095行和30项事实；本阶段未运行Cargo、测试或通知生命周期。

完整静态读取pager `app/acp_handler/tests`三个小型单元测试文件共235行并登记SHA-256，新增announcement、workflow与child permission routing三项契约。全部仅静态审计、未执行Cargo；effect持久化、leader顺序、revision/replay及真实UI均不由这些测试源码扩大保证。

完整静态读取pager `app/acp_handler/tests`三个后续文件共442行并登记SHA-256，新增git/path、plugin push和goal projection三项测试契约。额外读取canonical GoalUpdated DTO确认token_budget/usage/status message缺省规则，从而限定retired字段负例的证据解释；未运行Cargo、git、plugin或goal流程。


完整静态读取pager `app/acp_handler/tests/background_tasks.rs` 637行并登记SHA-256，新增background replay、demotion、ownership、monitor和completion测试契约。该文件22项测试本轮未执行；没有Cargo构建。父`markdown`递归统计曾误包含独立`markdown-fuzz` nested crate，后续覆盖率统一按最深crate根归属，避免重复计数。


完整静态读取pager `app/acp_handler/tests/command_feedback.rs` 235行并登记SHA-256，新增command progress、memory correlation与compaction replay测试契约。六项测试本轮未执行；没有Cargo构建。另将既有CommittedJsonlLines source定位由`pub struct`修正为当前源码的`pub(crate) struct`，同步feature-map与session-timeline delta。


完整静态读取pager `app/acp_handler/tests/interjection.rs` 247行并登记SHA-256，新增parked lifecycle与broadcast interjection测试契约。九项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/coordination.rs` 430行并登记SHA-256，新增coordination sideband lifecycle、replay及source-tool测试契约。八项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/permissions.rs` 371行并登记SHA-256，新增permission argument、recap、pane与cleanup测试契约。十六项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/mcp.rs` 539行并登记SHA-256，新增MCP progress、owner modal patch与catalog refresh测试契约。十八项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/settings.rs` 392行并登记SHA-256，新增verb grouping、Auto gate与soft permission default测试契约。十一项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/scheduled_tasks.rs` 326行并登记SHA-256，新增scheduled fire/upsert/linkage与owner routing测试契约。九项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/session_routing.rs` 398行并登记SHA-256，新增session demultiplex、race fallback与activity ownership测试契约。十一项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/interactions.rs` 580行并登记SHA-256，新增interaction resolution、background ownership与plan reopen测试契约。十六项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/models.rs` 684行并登记SHA-256，新增model catalog、authoritative change control与child isolation测试契约。十八项测试本轮未执行；没有Cargo构建。该测试文件不覆盖同ModelId路由热更或sampler native状态。


完整静态读取pager `app/acp_handler/tests/plan_mode.rs` 591行并登记SHA-256，新增plan approval、Behavior confirmation与held FIFO测试契约。二十四项测试本轮未执行；没有Cargo构建。


完整静态读取pager `app/acp_handler/tests/mod.rs` 1757行并登记SHA-256，新增ACP fixture topology、subagent replay disk和四项直接测试三条契约。未执行Cargo；临时文件能力仅是fixture源码事实，不计作本轮动态验证。


合并Luna/high并行审计的pager `app/acp_handler/tests/reconnect.rs`：1123行、26项测试、SHA-256与当前工作树一致，草稿三项契约的来源已由行号描述规范化为26个当前源码精确测试函数。未执行Cargo或动态reconnect。


合并Luna/high并行审计的pager `app/acp_handler/tests/subagents.rs`：1473行、29项测试、SHA-256与当前工作树一致；两项草稿来源已规范化为全部29个当前源码精确测试函数。未执行Cargo或动态subagent流程。

合并Luna/high并行审计的pager `app/acp_handler/tests/session_events.rs`：1474行、41项测试、SHA-256与当前工作树一致；39项测试映射到四项新增契约，2项重叠测试精确补入既有coordination与compaction-replay契约。未执行Cargo或动态session event。

合并Luna/high并行审计的pager `app/root/dispatch/tests/dashboard.rs`：5205行、154项测试（152普通、2 tokio）、SHA-256匹配；13项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态dashboard dispatch。

合并Luna/high并行审计的pager `tests/settings_e2e.rs`：6438行、226项测试、SHA-256匹配；12项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态settings流程。

合并Luna/high并行审计的pager `src/scrollback/render.rs`：4980行、79项测试、SHA-256匹配；6项新增契约精确覆盖全部测试且无重复，并保留生产符号证据。未执行Cargo或真实终端渲染。


OpenSpec strict初检发现四项session-events英文契约缺RFC 2119关键词；已在feature-map与test-harness-runtime delta同步补入SHALL，不改变事实范围，并重新执行strict验证。

合并Luna/high并行审计的pager `src/scrollback/state/mod.rs`：3966行、65项测试、SHA-256匹配；5项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态scrollback lifecycle。

合并Luna/high并行审计的pager `src/scrollback/state/layout.rs`：3707行、55项测试、SHA-256匹配；5项新增契约精确覆盖全部测试且无重复。未执行Cargo或真实布局渲染。

合并Luna/high并行审计的pager `src/views/picker.rs`：4148行、35项测试、SHA-256匹配；5项新增契约精确覆盖全部测试且无重复。未执行Cargo或真实picker交互。

Luna/high独立复核pager `src/views/settings_modal/tests.rs`：7482行、169项测试（1 ignored）、SHA-256与既有reviewed_files登记一致。该文件此前已完整审计，本次不重复增加需求；未执行Cargo。

合并Luna/high并行审计的pager `src/views/extensions_modal.rs`：6642行、142项测试、SHA-256匹配；10项新增契约覆盖全部测试，交叉证明的测试保留多需求映射。未执行Cargo、网络或安装流程。

合并Luna/high并行审计的pager `src/views/dashboard/render.rs`：8971行、112项测试（2 ignored）、SHA-256匹配；9项新增契约精确覆盖全部测试且无重复，并保留生产符号证据。未执行Cargo或真实终端渲染。

合并Luna/high并行审计的pager `src/views/shortcuts_help.rs`：3657行、64项测试、SHA-256匹配；3项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态shortcuts UI。

合并Luna/high并行审计的pager `src/app/agent_view/selection.rs`：3374行、47项测试、SHA-256匹配；6项新增契约精确覆盖全部测试且无重复，并列出36个生产函数。未执行Cargo或真实selection/clipboard。

合并Luna/high并行审计的pager `src/views/question_view.rs`：3546行、54项测试、SHA-256匹配；3项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态question response。


完整静态读取并登记pager十个最小生产文件共391行：root dispatch session/mod、diagnostics tmux probe、agent goal、dashboard diagnostics、scrollback blocks/mod、external editor、scrollback/mod、file-search/mod、views/mod与dispatch/mod。全部SHA-256来自当前工作树，新增六项窄契约；未执行Cargo或动态I/O。


完整静态读取并登记pager五个小型生产文件共460行：scrollback wrapper façade、jump dispatch、debug overlay style、status bar与Markdown export。当前SHA-256已写入，三个测试符号逐一映射；未执行Cargo或动态UI。

合并Luna/high并行审计的pager `src/views/dashboard/state.rs`：11695行、241项测试、SHA-256匹配；11项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态dashboard state。

合并Luna/high并行审计的pager `src/app/root/mod.rs`：9600行、218项测试、SHA-256匹配；11项新增契约精确覆盖全部测试且无重复。未执行Cargo或真实root app。

合并Luna/high并行审计的pager `src/app/root/event_loop.rs`：4730行、80项测试、SHA-256匹配；7项新增契约精确覆盖全部测试且无重复；条件测试不会在单一平台同时编译。未执行Cargo或动态event loop。


完整静态读取并登记pager五个小型生产文件共553行：session modal、Btw block、Accented wrapper、state types与Lifecycle hook block。当前SHA-256已写入，Accented三个测试函数均有精确来源；未执行Cargo或动态UI。

合并Luna/high并行审计的pager `src/app/agent_view/render.rs`：4730行、16项测试、SHA-256匹配；7项新增契约精确覆盖全部测试且无重复。未执行Cargo或真实AgentView渲染。

合并Luna/high并行审计的pager `src/views/prompt_widget/tests.rs`：4582行、241项测试、SHA-256匹配；8项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态prompt输入。

合并Luna/high并行审计的pager `src/app/session/mod.rs`：4110行、53项测试、SHA-256匹配；4项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态session lifecycle。

### 2026-09-08 Pager 69.5%阶段一致性复核

当前feature-map共2537项，crate inventory共1298个reviewed_files；全部已登记文件SHA-256匹配当前工作树，feature键无重复，每项requirement标题存在于对应delta，每个source路径存在。Pager按真实crate根和最深crate归属统计为449/646个Rust文件（69.5%），尚缺197个；不得把该阶段结果表述为Pager或全仓审计完成。`openspec validate --all --strict --no-interactive`为15/15通过。工作树68M、`target/`不存在、数据卷剩余56GiB；本轮阶段复核未运行Cargo。

合并Luna/high并行审计的pager `src/views/prompt_widget/mod.rs`：3631行、0项本地测试、SHA-256匹配；8项新增生产契约，外置tests.rs不重复登记。未执行Cargo或动态prompt widget。

合并Luna/high并行审计的pager `src/app/agent_view/modal_routing.rs`：3381行、31项测试、SHA-256匹配；8项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态modal routing。

合并Luna/high并行审计的pager `src/app/root/effects/mod.rs`：4089行、99个Effect分支、0项本地测试、SHA-256匹配；13项新增生产契约覆盖全部分支且无重复，并新增既有local-coordination能力的change delta容器。未执行Cargo或任何effect。

合并Luna/high并行审计的pager `src/scrollback/text_selection.rs`：3106行、102项测试、SHA-256匹配；6项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态selection。

合并Luna/high并行审计的pager `src/views/tasks_pane.rs`：3311行、51项测试、SHA-256匹配；12项新增契约中测试来源精确覆盖全部测试且无重复。未执行Cargo或动态tasks pane。

合并Luna/high并行审计的pager `src/views/permission_view.rs`：3066行、47项测试（1 ignored）、SHA-256匹配；5项新增契约精确覆盖全部测试且无重复，并覆盖39个生产函数。未执行Cargo或动态permission UI。

合并Luna/high复用代理审计的pager `src/scrollback/state/selection.rs`：2897行、71项测试、SHA-256匹配；11项新增契约精确覆盖全部测试且无重复。未执行Cargo或动态selection state。

合并Luna/high复用代理审计的pager `src/app/agent_view/modals.rs`：2902行、27项测试、SHA-256匹配；6项新增契约精确覆盖全部测试且无重复，并覆盖22个生产方法。未执行Cargo或动态modal动作。


### 2026-09-08 Pager 70.7%阶段一致性复核

本轮并行批次全部回收后，feature-map为2606项、source证据8244条、crate inventory为1306个reviewed_files；全部已登记文件SHA-256匹配当前工作树，feature键无重复，每项requirement标题存在于对应delta且所有source路径存在。Pager按真实crate根统计为457/646个Rust文件（70.7%），尚缺189个；该进度仍不得表述为Pager或全仓审计完成。`openspec validate --all --strict --no-interactive`为15/15通过。工作树68M、`target/`不存在、数据卷剩余56GiB；本阶段未运行Cargo，无需执行cargo clean。
合并Luna/high并行审计的pager `src/diagnostics/mod.rs`：3028行、110项测试、SHA-256匹配；3项新增契约精确覆盖新增测试，doctor slash证据补入既有契约。未执行Cargo或动态diagnostics。
完成`pager-render` crate全量静态核对：Cargo.toml、67个Rust文件和3个.tmTheme资源共71个文件，现有173条delta保持不变，全部reviewed_files哈希匹配，crate状态更新为reviewed。未执行Cargo或动态终端验证。
完成shell crate全量静态审计：385个文件（Cargo.toml、383个Rust、1个Python runner，含3个bench源码），490条唯一功能需求（189条既有、301条新增），全部385个文件SHA-256匹配并消除pending_mapping；未执行Cargo或动态测试。
完成pager-pty-harness crate全量静态核对：40个文件（Cargo.toml、38个Rust、bench README），79条既有delta、全部reviewed_files哈希匹配，crate状态更新为reviewed。未执行Cargo或动态PTY验证。


OpenSpec strict复核发现三项agent_view新增契约缺少RFC 2119关键词；已同步在feature-map与client-surfaces delta补入SHALL，事实范围不变。

完成`tools` crate全量静态审计：198个文件、200条需求（198条新增、2条既有跨crate需求补入tools证据），全部文件SHA-256与当前工作树匹配；`tools` inventory标记为reviewed，未运行Cargo、build.rs或外部下载。严格校验将在本批次合并后重新执行。

完成`nono` crate全量静态审计：36个文件、39条`sandbox-boundary`需求，全部文件SHA-256与当前工作树匹配；`nono` inventory标记为reviewed，未运行Cargo、build.rs或平台沙箱。

完成`workspace` crate全量静态审计：56个文件、35条既有能力需求补全workspace来源，全部文件SHA-256与当前工作树匹配；`workspace` inventory标记为reviewed，未运行Cargo。

合并Pager `src/app/root/dispatch/tests/task_result.rs`：2965行、68项内联测试、SHA-256匹配；新增69条契约，测试来源逐项登记，pager reviewed 文件增至461个。未运行Cargo或动态dispatch链路。

完成`nix-ohos` crate全量静态审计：62个文件、62条需求，全部文件SHA-256与当前工作树匹配；`nix` inventory标记为reviewed，未运行Cargo或平台系统调用。

合并Pager `src/scrollback/blocks/tool/edit.rs`：2919行、42项内联测试、SHA-256匹配；新增14条需求并补充1条既有契约来源，pager reviewed 文件增至462个。未运行Cargo或真实终端渲染。

合并Pager `src/app/agent_view/input.rs`：2802行、57项内联测试、SHA-256匹配；新增70条契约，pager reviewed 文件增至463个。未运行Cargo或动态输入链路。

合并Pager `src/app/agent_view/links.rs`：2773行、88项内联测试、SHA-256匹配；新增5条契约，pager reviewed 文件增至464个。未运行Cargo或动态链接交互。

合并Pager `src/app/agent_view/interactions.rs`：2623行、30项内联测试、SHA-256匹配；新增15条契约，pager reviewed 文件增至465个。未运行Cargo或动态交互链路。

合并Pager `src/app/agent_view/interactions.rs`：2623行、30项内联测试、SHA-256匹配；新增15条契约，pager reviewed 文件增至465个。未运行Cargo或动态交互链路。

合并Pager `src/app/agent_view/paste.rs`：2695行、76项内联测试、SHA-256匹配；新增15条契约，pager reviewed 文件增至466个。未运行Cargo或动态剪贴板链路。

合并Pager `src/app/root/dispatch/dashboard.rs`：2484行、46个生产函数、SHA-256匹配；新增5条契约，pager reviewed 文件增至467个。未运行Cargo或动态dashboard链路。

合并Pager `src/views/dashboard/peek.rs`：2467行、28项内联测试、SHA-256匹配；新增12条契约，pager reviewed 文件增至468个。未运行Cargo或动态peek链路。

合并Pager `src/views/list_pane/state/mod.rs`：2740行、115项内联测试、SHA-256匹配；新增13条契约，pager reviewed 文件增至469个。未运行Cargo或动态列表交互。

合并Pager `src/app/root/dispatch/tests/router.rs`：2452行、91项内联测试、SHA-256匹配；新增15条契约，pager reviewed 文件增至470个。未运行Cargo或动态dispatch。

合并Pager `src/scrollback/state/nav.rs`：2426行、39项内联测试、SHA-256匹配；新增11条契约，pager reviewed 文件增至471个。未运行Cargo或动态导航。

合并Pager `src/app/agent_view/mermaid_worker.rs`：2372行、37项内联测试、SHA-256匹配；新增14条契约，pager reviewed 文件增至472个。未运行Cargo或动态Mermaid渲染。

合并Pager `src/views/dashboard/row.rs`：2131行、43项内联测试、SHA-256匹配；新增6条契约，pager reviewed 文件增至473个。未运行Cargo或动态dashboard渲染。

合并Pager `src/views/list_pane/state/methods.rs`：2290行、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至474个。未运行Cargo或动态列表交互。

合并Pager `src/views/agent.rs`：2227行、47项内联测试、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至475个。未运行Cargo或动态Agent视图。

合并Pager `src/app/mod.rs`：2143行、74项内联测试、SHA-256匹配；新增11条契约，pager reviewed 文件增至476个。未运行Cargo或动态App生命周期。

合并Pager `src/views/workflows.rs`：2125行、24项内联测试、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至477个。未运行Cargo或动态Workflow视图。

合并Pager `src/scrollback/block.rs`：1775行、33项内联测试、SHA-256匹配；新增10条契约，pager reviewed 文件增至478个。未运行Cargo或动态scrollback。

合并Pager `src/views/modal_window.rs`：1968行、47项内联测试、SHA-256匹配；新增12条契约，pager reviewed 文件增至479个。未运行Cargo或动态modal交互。

合并Pager `src/scrollback/wrappers/entry_renderer.rs`：2064行、32项内联测试、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至480个。未运行Cargo或动态渲染。

合并Pager `src/views/announcements.rs`：1578行、29项内联测试、SHA-256匹配；新增8条契约，pager reviewed 文件增至481个。未运行Cargo或动态announcement交互。

合并Pager `src/views/file_search/line_viewer.rs`：1866行、9项内联测试、SHA-256匹配；新增3条契约，pager reviewed 文件增至482个。未运行Cargo或动态文件搜索。

合并Pager `src/views/block_viewer.rs`：1748行、无内联测试、SHA-256匹配；新增9条契约，pager reviewed 文件增至483个。未运行Cargo或动态block viewer。

合并Pager `src/views/list_pane/render.rs`：1748行、27项内联测试、SHA-256匹配；新增7条契约，pager reviewed 文件增至484个。未运行Cargo或动态列表渲染。

合并Pager `src/app/root/dispatch/tests/turn.rs`：1987行、60项内联测试、SHA-256匹配；新增14条契约，pager reviewed 文件增至485个。未运行Cargo或动态turn处理。

合并Pager `src/app/agent_view/prompt.rs`：1906行、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至486个。未运行Cargo或动态prompt处理。

合并Pager `src/app/agent_view/viewer.rs`：1011行、无内联测试、SHA-256匹配；新增9条契约，pager reviewed 文件增至487个。未运行Cargo或动态viewer。

合并Pager `src/app/agent_view/session.rs`：2035行、29项内联测试、SHA-256匹配；补齐该文件功能契约，pager reviewed 文件增至488个。未运行Cargo或动态会话。

合并Pager `src/app/root/dispatch/tests/session/lifecycle.rs`：2166行、70项内联测试、SHA-256匹配；新增7条契约，pager reviewed 文件增至489个。未运行Cargo或动态session生命周期。

合并Pager `src/app/root/dispatch/prompt.rs`：1488行、无内联测试、SHA-256匹配；新增14条契约，pager reviewed 文件增至490个。未运行Cargo或动态prompt dispatch。

补充Pager `src/app/root/dispatch/tests/session/lifecycle.rs`第二轮细化：同一2166行文件新增12条更细的生命周期契约，既有7条摘要契约保留，pager reviewed 文件仍为490个。未运行Cargo或动态session dispatch。

合并Pager `src/views/tasks_pane.rs`：3311行、51项内联测试、SHA-256匹配；新增12条契约，pager reviewed 文件增至491个。未运行Cargo或动态tasks pane。

补充Pager `src/scrollback/state/mod.rs`独立全量复核：3966行、65项内联测试、SHA-256匹配；既有契约来源补齐，Pager文件登记保持490个。未运行Cargo或动态scrollback。

补充Pager `src/scrollback/state/mod.rs`第二轮独立审计：新增1条契约并合并8项既有来源，Pager文件登记保持490个。未运行Cargo或动态scrollback。

合并Pager `src/app/root/dispatch/router.rs`：1453行、无内联测试、SHA-256匹配；新增12条契约，pager reviewed 文件增至492个。未运行Cargo或动态router。

合并Pager `src/app/root/dispatch/task_result.rs`：1481行、无内联测试、SHA-256匹配；新增9条契约，pager reviewed 文件增至493个。未运行Cargo或动态task result。

合并Pager `src/views/modal.rs`：1655行、12项内联测试、SHA-256匹配；新增6条契约，pager reviewed 文件增至494个。未运行Cargo或动态modal。

合并Pager `src/app/agent_view/mouse.rs`：1225行、无内联测试、SHA-256匹配；新增6条契约，pager reviewed 文件增至494个。未运行Cargo或动态鼠标交互。

合并Pager `src/app/root/dispatch/tests/session/load.rs`：1689行、54项内联测试、SHA-256匹配；新增10条契约，pager reviewed 文件增至495个。未运行Cargo或动态session加载。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 16 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 10 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 16 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 16 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 15 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 2 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 5 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 8 条唯一契约并合并 13 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 6 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 2 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 11 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 10 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 9 条唯一契约并合并 4 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 10 条唯一契约并合并 3 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 2 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 2 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 16 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 15 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 1 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 1 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 4 个 Pager Rust 文件，新增 14 条唯一契约并合并 5 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 1 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 14 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 12 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 11 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 13 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。

- 本批静态审计覆盖 3 个 Pager Rust 文件，新增 11 条唯一契约并合并 0 条既有契约；逐文件 SHA、行数和内联测试符号均已核对，未运行 Cargo。
