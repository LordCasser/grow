# Grow 功能审查 · 2026-09-07

后续深入审查见 [Trajectory 分类与诊断链路复审](/Users/lordcasser/workspace/projects/grow/docs/architecture/trajectory-review-2026-09-07.md)：另确认 7 项问题，并补充 5 组分类与架构建议。

审查基线：`87d1df4c3b0a36718427eb0db190fbf8a3c84098`。检查功能正确性，并提出有代码依据的架构优化，不涉及安全审计。下文保留修复前的发现和复现证据，行号对应审查时源码；用户随后授权二次分析并修复，当前修复状态与回归结果见文末。工作区内其他任务的改动予以保留。

先沿 README、Timeline、Shell、Workflow 和既有审计记录建立调用关系，再检查实际工具入口与边界状态。固定复现和基线测试交给 `gpt-5.6-luna` 子代理，主审重新核对当前源码、生产调用链和证据范围。Atlas 只用于局部结构查询，不将局部查询当作全仓覆盖证明。

以下功能发现都有运行证据；其中生产入口与下游影响的静态核对会单独说明。架构建议另列，不能等同于已复现的功能缺陷。没有把未接入生产的辅助类、已修复旧问题或被反证的猜想计入发现。

**F1 · [P1] `/loop` 中文参数会在本地解析阶段 panic**

位置：[loop_cmd.rs:39](/Users/lordcasser/workspace/projects/grow/crates/codegen/pager/src/slash/commands/loop_cmd.rs:39)，同类问题还在 [interval.rs:15](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs:15)。

`parse_loop_args` 把第一个词交给 `is_interval_token`，后者按最后一个字节拆后缀。输入 `/loop 每5分钟 检查构建` 时，切分点落在“钟”的 UTF-8 编码中，尚未交给模型就会 panic。`scheduler_create(interval="5分钟")` 也经过同样的切分，未能返回正常的 `InvalidInterval`。项目 release 配置为 `panic="abort"`，因此这条错误输入路径可以结束进程。

从原文件提取未改动的 parser 函数运行，得到：

```text
"每5分钟 检查构建" -> panic: end byte index 9 is not a char boundary
"5ｍ 检查构建"   -> panic: end byte index 3 is not a char boundary
"5m check build" -> Ok((Some("5m"), "check build"))
```

验证范围是实际解析函数与入口调用链，没有启动完整 TUI。最小修复：在字节切分前确认 ASCII 后缀，或用字符边界拆分；两个入口一起覆盖中文、全角后缀和正常间隔输入。[复现输出](/tmp/grow-review-20260907/loop-parser/run.log)、[scheduler 输出](/tmp/grow-review-20260907/interval/run.log)。

**F2 · [P1] `search_replace` 会改坏匹配区域之外的非 UTF-8 字节**

位置：[search_replace/mod.rs:508](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs:508)。

整个文件先经过 `String::from_utf8_lossy`，替换完成后再整体写回。对于 Latin-1 文件，即使只替换 ASCII 的 `old`，其他位置的 `0xe9` 也会被写成 `ef bf bd`。真实 `SearchReplaceTool::run` 返回 `EditsApplied`，没有提示编码损失。

```text
before: prefix [e9] old\n
after:  prefix [ef bf bd] new\n
```

这是文本局部修改造成的额外数据改变，不要求支持所有编码；对于无法无损处理的输入，拒绝修改也比损失性解码后写回正确。最小修复：在写入前使用严格解码，或实现明确的编码保留策略；回归断言匹配区域外的字节完全不变。Hashline 编辑也有同类解码语句，但本项运行证据来自标准工具，不把同根因重复计数。[真实 API 复现](/tmp/grow-review-20260907/search-replace/harness.log)。

**F3 · [P1] 重复安装插件失败后，原来的可用安装也被删除**

位置：[installer.rs:74](/Users/lordcasser/workspace/projects/grow/crates/codegen/plugin-marketplace/src/installer.rs:74)。

底层返回 `AlreadyInstalled` 后，marketplace 安装器先删除安装目录、移除 registry 记录并保存，再尝试新安装。先成功安装本地插件，再破坏源 manifest 并重复调用真实安装 API，得到：

```text
Err(InstallFailed { detail: "no plugins found in the source (missing required plugin.json)" })
old_path_exists=false registry_has_key=false
```

CLI 的一个入口有提前判重，但 Shell 的本地 marketplace 安装分支没有同样的短路，因此不能用 CLI 测试的成功来排除这条路径。远端分支存在同构操作；通过本地 `file://` Git fixture，改变 provenance 后令源不可用，也复现了旧目录与记录同时消失。同 provenance 的远端重复安装有短路，不受这个具体场景影响。

最小修复：普通重复安装直接返回已有结果；需要重装时，在新安装完全可用之前保留旧安装，并复用现有事务更新路径。复制失败残留半成品目录也在同一安装事务中处理，不另计一个问题。[复现、调用入口与命令](/tmp/grow-review-20260907/plugin/result.md)。

**F4 · [P2] 调度任务 ID 只保留毫秒时间，同毫秒任务无法区分**

位置：[scheduler/types.rs:251](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs:251)。

UUID v7 去掉横线后只取前 12 个十六进制字符，恰好仅留下 48 位毫秒时间戳。连续调用真实 `ScheduledTask::new` 1,000 次，只有 4 个不同 ID，单个 ID 重复 436 次。这个构造测试没有向 actor 创建超过 50 个任务；它验证的是身份生成。

生产 actor 的 Create 只检查任务总数就 `push`，没有拒绝同 ID；Update/Delete 又只操作第一个匹配项。因此同毫秒创建的不同任务会共享可操作句柄，后一个任务的更新可能落到前一个任务上。

最小修复：保留 UUID 的随机部分或完整 UUID，并用同毫秒创建两个不同 prompt 的用例验证更新、删除各自命中正确任务。[复现输出](/tmp/grow-review-20260907/scheduler-id/harness.log)。

**F5 · [P2] Hashline 编辑缺少 `FileWritten` 通知，未进入 Agent 写入跟踪**

位置：[hashline/edit/mod.rs:398](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs:398)，消费者：[notification_bridge.rs:398](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/tools/notification_bridge.rs:398)。

标准 Write/SearchReplace 在成功写盘后发送携带 `previous_content` 的 `FileWritten`。Hashline 新建和修改路径直接写盘并返回结果，没有发送该通知。真实工具 API 的通知通道对比结果为：

```text
standard_write: EditsApplied, FileWritten=1
standard_edit:  EditsApplied, FileWritten=1
hashline_edit:  EditsApplied, FileWritten=0
```

生产 bridge 通过这个事件调用 `record_agent_write` 和 `add_before_snapshot_for_prompt`；普通文件系统变化通知无法补回写前内容。把实际捕获的通知按 bridge 相同调用方式送入真实 `FileStateTracker`，得到 `standard_before_snapshots=2 hashline_before_snapshots=0`。对于仅通过 Hashline 修改且没有其他来源快照的文件，这会遗漏对应的 Agent 写入归属和文件撤销记录。已验证通知与快照记录，没有执行完整 TUI 撤销操作。

最小修复：让 Hashline 两条成功写入路径发送同一 `FileWritten`，使用实际写前、写后内容；验证新建、修改及无其他编辑参与时的撤销。[复现记录](/tmp/grow-review-20260907/hashline/result.md)。

**F6 · [P2] Hashline 新文件写入在校验之前执行，报错仍留下文件**

位置：[hashline/edit/mod.rs:329](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs:329)。

新文件的单个 Write 操作先 `fs.write_file`，随后才调用 `apply_edits`。真实 API 以会触发锚点前缀校验的内容创建文件，返回 `InvalidInput`，但 `file_exists=true` 且内容已经完整写入。已有文件的相同输入则在写入前拒绝。两条路径的“失败”含义不一致。

这与下面的前缀误识别是独立问题：即使前缀校验完全正确，校验拒绝后仍不应该先完成写盘。最小修复：新建文件也先完成纯校验，再执行写入和通知。[复现记录](/tmp/grow-review-20260907/hashline/result.md)。

**F7 · [P2] Hashline 局部编辑会把整份 CRLF 文件改成 LF**

位置：[hashline/edit/apply.rs:283](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs:283)，输入拆行：[anchor.rs:21](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs:21)。

`split_lines` 使用 `str::lines` 去掉 CRLF 的 CR，编辑后以 `join("\n")` 重组所有行。真实工具只替换三行文件的第二行，输出为：

```text
before: one\r\ntwo\r\nthree\r\n
after:  one\nchanged\nthree\n
```

因此未修改的第一行和第三行也产生 diff，可能影响要求 CRLF 的文件与后续工具。标准 SearchReplace 已有 CRLF 保留处理，Hashline 路径没有相同保证。最小修复：保留源文件换行策略，验证改动区域之外的行终止符不变。[复现记录](/tmp/grow-review-20260907/hashline/result.md)。

**F8 · [P2] Hashline 把合法 C++ 成员调用误判为复制的锚点**

位置：[hashline/edit/apply.rs:59](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs:59)。

只要 `->` 前的内容短、包含 `:` 且没有空格，就被认定为锚点前缀。因此 `ns::get()->run();` 这样的合法语句，在真实 Write、Replace、InsertAfter 三种操作中都返回 `InvalidInput`，已有文件保持不变。该判断没有要求行号或验证当前 scheme 的 hash 语法。

最小修复：只识别实际锚点格式，复用 scheme 的解析规则；让正常 C++ 作用域与成员访问通过。回归同时保留对真实复制锚点的提示。[复现记录](/tmp/grow-review-20260907/hashline/result.md)。

**F9 · [P2] `search_replace` 无法匹配本来完全一致的 CRLF `old_string`**

位置：[search_replace/mod.rs:511](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs:511)。

文件一侧被归一化成 LF，`input.old_string` 一侧保持 CRLF。真实工具对内容 `alpha\r\nbeta\r\n`，用相同换行形式的多行 `old_string` 执行替换，返回 `NoMatchesFound`，尽管原始文件确实含有完全相同的字符串。

读工具通常给模型 LF 文本，所以常规测试通过；显式带 CRLF 的调用仍是有效输入。最小修复：统一匹配双方的换行归一化规则，同时继续保留写回时的源文件换行形式。[真实 API 输出](/tmp/grow-review-20260907/search-replace/harness.log)。

**F10 · [P2] Memory watcher 会把其他 workspace 的新内容加入当前项目检索**

位置：[memory/backend.rs:255](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/backend.rs:255)，watcher 创建：[spawn.rs:1258](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/spawn.rs:1258)。

初始化索引通过 `list_memory_files()` 选择全局 MEMORY.md、当前 workspace MEMORY.md 和当前 session 日志；watcher 却递归监听整个 `global_dir`，消费路径时没有复用同一范围判断。B 项目的新 MEMORY.md 被 A 的 `classify_source` 归为 `global`，写入 A 的索引，而 A 的正常文件枚举仍排除 B。

这会让当前项目的约定检索混入另一项目的内容，并按 global 来源参与排序。验证使用两个隔离 workspace、真实文件事件和索引写入，并用实际编译的 `MemoryBackendImpl`、FTS/hybrid search 再次确认：B 的路径以 `global` 来源出现在 A 的结果中，而 `list_memory_files()` 排除 B。运行命令见 [Memory 记录](/tmp/grow-review-20260907/memory/result.md)。

最小修复：初始化枚举与 watcher 增量同步共用一套所属范围判断，覆盖全局文件、当前项目、兄弟项目及删除事件。

**F11 · [P2] 迟到的 embedding 可以绑定到已经更新的 chunk 文本**

位置：[memory/index.rs:550](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/index.rs:550)，异步采集与回写：[backend.rs:290](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/backend.rs:290)。

embedding 请求保留 `(chunk_id, text)`，回写只带 `chunk_id` 和向量。chunk ID 是路径加顺序号；另一轮 reindex 在同一 ID 更新正文后，旧请求仍能成功写入旧正文的向量。现有校验只检查模型/端点身份，没有检查 chunk 内容版本。

确定性固定上述交错后，数据库中正文已经为新内容，向量仍为旧 `[1,0,0,0]`；KNN 对旧向量返回距离 `0.0`，`chunks_without_embeddings()` 已为空，因此普通 missing backfill 不会再纠正它。生产启动阶段的后台 reindex/embed 与搜索的增量同步可以并发，见 [spawn.rs:2786](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/spawn.rs:2786)。

最小修复：请求捕获内容 hash，写回时在事务内比较同一 hash；已变化的结果丢弃并保留补齐资格。已使用实际编译的 `MemoryIndex` 与 sqlite-vec 复现；没有调用真实 embedding 服务，而是固定异步请求与回写之间允许发生的更新。[向量值、KNN 与缺失查询输出](/tmp/grow-review-20260907/memory/result.md)。

**F12 · [P2] Memory 未变化 chunk 的行号在前文插入后保持旧值**

位置：[memory/index.rs:277](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/index.rs:277)。

reindex 仅凭正文 hash 相等就跳过整条记录，没有更新 `start_line/end_line`。在前一节增加两行，后一节文本和 chunk 顺序不变时，后一节实际范围由 `3..5` 变成 `5..7`，索引仍保存 `3..5`。按返回范围读取文件，得到前一节新加的 `extra-one\nextra-two`，而不是命中的关键词。

这是 reindex 后的定位错误，与向量缓存无关。已使用实际编译的 `MemoryIndex` 确认。最小修复：正文相同可以保留 FTS/embedding，但位置元数据仍应更新；回归覆盖前文增删行、正文相同的后续 chunk。[复现输出](/tmp/grow-review-20260907/memory/result.md)。

**F13 · [P2] `memory_search` 展示从 0 开始的行号，和 `memory_get` 的从 1 开始约定不一致**

位置：[memory/search_tool.rs:99](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/memory/search_tool.rs:99)，读取转换：[memory/get_tool.rs:98](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/memory/get_tool.rs:98)。

chunk 使用从 0 开始、末端不包含的范围，search 直接输出为 `lines start-end`；get 明确接收从 1 开始的 `from`，调用 backend 前再减一。新文件首次索引就有这种不一致，无需发生 F12 的前文修改。

以较小的合法 chunk 配置固定分段，实际 backend search 返回 `4..5`，工具格式化为 `lines 4-5`，命中 token 实际在文件第 5 行。把显示起点 `from=4` 用于单行读取，按 get 的转换执行实际 backend API，得到上一行 `## Second`，没有读到 token。若按显示的闭区间读取两行，则会多读上一行；第一块还可能显示 `lines 0-…`。验证使用真实编译的 backend，工具格式化与参数转换按当前源码复现，没有运行完整工具会话。

最小修复：仅在 search 的展示边界把 `[start, end)` 转成从 1 开始的闭区间 `[start+1, end]`，backend 内部继续使用同一套偏移约定；回归验证首次索引的非首块 `search → get`。[真实运行输出](/tmp/grow-review-20260907/memory/result.md)。

**独立的架构优化建议**

**A1 · 收敛文件编辑的提交步骤，保留不同编辑算法。** F2、F5、F6、F7、F9 表明，标准编辑和 Hashline 各自实现读取、校验、写回、通知后，已经产生不同的失败语义和内容保留行为。先分别修复问题，再从现有工具中抽取小范围的共同提交步骤：读取写前内容、完成校验与转换、写盘、发送一次包含写前/写后内容的 `FileWritten`。字符串匹配与锚点定位继续各自负责，不必引入通用编辑引擎或新 actor。验收应围绕共同契约：失败不改变文件、局部修改保留其他内容、成功写入能撤销；新建与修改都必须覆盖。

**A2 · Memory 的初始化、增量同步、异步向量回写应共用明确的不变量。** F10 来自范围判断分叉，F11 来自内容版本缺失，F12 来自把“正文未变”等同于“记录未变”。[启动后台任务](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/spawn.rs:2786) 与 [搜索触发同步](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/backend.rs:255) 应复用范围判断与写入规则。分步收敛：先统一文件所属范围，再让位置元数据独立刷新，最后为 embedding 写回增加内容 hash 条件。现有 SQLite 索引继续作为可重建派生数据，不需要增加另一份持久化状态。

重建协调另开一项：当前 [release_claim](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/index.rs:631) 无条件清除 claim；已实测 A 获得 claim、B 在过期后接管、A 释放、C 又成功获得 claim。释放应比较本次获取的 owner token；启动后台任务也应遵守相同的协调约定。它解决重复并发工作，不能代替 F11 的内容版本校验，二者分别验收。

**A3 · 调度器只保留一份间隔解析和完整任务身份。** TUI `/loop` 与 scheduler 工具各自按字符串切分间隔，出现了相同的 Unicode panic；任务 ID 又在展示友好化时丢掉唯一性。可在已有 scheduler 类型边界提供共享解析结果，TUI 和工具入口只做参数映射。持久化及操作使用完整 UUID；需要短展示时仅在 UI 格式化，不能让短展示值成为更新/删除键。先修 F1/F4，再整理依赖方向，不为这两个辅助功能新建泛化框架。

**A4 · 对话投影可减少未变化内容的重复处理，但先保住投影失效语义。** [reconcile_projection](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/state.rs:298) 每次请求都将全部 `ConversationItem` 序列化并 hash，包括未变化的图片 body；[request_projection](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/state.rs:339) 又为每个 native span 从头查找 SurfaceId。前者成本随完整 body 大小增长，后者查找最坏为 `O(spans × items)`。

局部 harness 复现同一 `serde_json::to_vec → blake3::hash` 算法，对相同 JSON 图片字段重复 10 次：

| 字段大小 | 每次平均耗时 |
| --- | ---: |
| 1 MiB | 33.7 ms |
| 8 MiB | 270.4 ms |
| 32 MiB | 1,100.8 ms |

这是 **debug 构建、普通 JSON fixture 的局部算法测量**，没有通过完整 `ChatStateHandle` 请求，也没有测 release；不能直接当成产品延迟或吞吐结论。它确认了重复全量处理这一成本来源。[测量代码与命令](/tmp/grow-review-20260907/projection/measurement.md)。

优化方向是在现有 actor 内缓存未变化投影内容的指纹，对 append 只处理新增项，并一次建立 SurfaceId 到位置的临时映射。不能仅凭 Timeline 事件不可变就永久复用指纹：Goal、图片预算、裁剪等请求投影变化仍须使对应缓存失效。先加入真实请求的 release 基准，覆盖追加、压缩、图片预算变化与 native continuation 重置，再决定是否值得调整；不建议先增加持久化缓存或新的状态源。

**A5 · 测试投入应补跨组件契约，而非继续堆局部成功用例。** 七个 crate 的 3,731 项现有测试全部通过，本轮仍复现了输入解析、编辑提交、检索定位和重装失败等问题。每项修复各补一个能通过真实入口失败的回归：`search → get`、`Hashline edit → snapshot/rewind`、`reinstall failure → old installation usable`、`create two tasks → update/delete correct task`。两套编辑工具复用相同契约用例；parser 增加 Unicode 输入性质测试。测试直接约束用户可见结果，不复制当前实现的中间步骤，也无需另建测试框架。

建议先处理可能中断会话或损坏已有内容的 F1–F3，再修文件编辑与 Memory 的正确性问题。A1–A3 随对应修复后独立整理；A4 先做生产形态的性能基准，优先级低于已复现的正确性问题。

**验证与范围**

完整执行以下七个 crate 的现有单元测试，合计 **3,731 passed / 0 failed / 7 ignored**：

| crate | passed | ignored |
| --- | ---: | ---: |
| chat-state | 460 | 0 |
| sampler | 218 | 0 |
| sampling-types | 264 | 0 |
| workflow | 65 | 0 |
| memory | 296 | 0 |
| plugin-marketplace | 100 | 0 |
| tools | 2,328 | 7 |

```sh
CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p workflow -p chat-state -p sampling-types -p sampler \
  --lib --quiet -- --test-threads=4
CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p tools -p memory -p plugin-marketplace \
  --lib --quiet -- --test-threads=4
```

另通过 2 项 marketplace 定向测试及 1 项 Shell 插件入口测试。它们不代表完整 Shell 回归。基线日志：[核心四 crate](/tmp/grow-review-20260907/baseline.log)、[工具与 Memory](/tmp/grow-review-20260907/tools-baseline.log)。独立复现的源码、命令和输出保存在 [/tmp/grow-review-20260907](/tmp/grow-review-20260907)，它们未加入产品源码或测试集。

实际运行环境为 macOS、Homebrew Rust/Cargo 1.97.1；仓库声明 1.93.1，但当前 PATH 没有由该文件切换工具链。测试离线运行；HTTP 模型服务、完整 TUI、Linux/Windows/OHOS 行为和 vendored 第三方实现不在本轮完整验证范围内。基线通过仅说明现有断言通过，不能覆盖本轮另外构造的边界用例。

**排除项与独立债务**

- `FileOperationLockManager` 获批后取消确实能泄漏锁，但当前没有生产调用者，因此不计为本轮用户可触发的功能缺陷。只有后续重新接入该组件时才需要一并解决其取消语义。[辅助类复现](/tmp/grow-review-20260907/locks/harness.log)。
- “FTS-only reindex 不清理旧向量”的猜想被反证：内部扩展可用字段与公开 `vec_available()` 的语义不同；实际修改和删除均清理了 `chunks_vec`。
- Memory 的重建 claim 在过期后可被接管，但旧 owner 的无条件 `release_claim` 又能清掉新 owner 的 claim。已用固定交错验证；它属于并发重建协调的独立修复边界，不与 F11 的 chunk 内容版本校验合成一个大改动，也不作为另一项用户结果重复计数。
- 9 月 5 日报告中已记录修复的 watcher dirty flag、embedding identity、Timeline 追加等问题不重复列入发现。本轮 F10/F11/F12 对应不同的范围、内容版本和位置元数据条件。

**二次分析与修复记录**

用户授权后，逐项重查入口、周边约束和复现条件，再实现以下修复。固定验证仍由低成本子代理承担，主审复核代码并串行执行共享 Cargo 构建。

| 范围 | 当前实现 |
| --- | --- |
| F1、F4 | `/loop` 复用 scheduler 的 ASCII/数值解析；保留完整 UUID v7。真实 actor 回归覆盖两个任务分别更新、删除。 |
| F2、F9 | 两套编辑入口拒绝非 UTF-8 内容；SearchReplace 统一匹配双方的 CRLF/LF，映射回原文修改，保留未编辑区域的字节。 |
| F3 | 重复安装返回 AlreadyInstalled，保留旧目录与 registry；新安装先在同文件系统临时目录完成复制/发现，再发布。覆盖本地、远端 provenance 变化和复制失败。 |
| F5–F8 | Hashline 先校验再写盘，新建与修改均发送包含写前/写后内容的 FileWritten；保留行终止符，区分真实锚点与合法 C++ 语句。补上未终止末行扩展和 EOF 插入边界。 |
| F10 | 初始化枚举与 watcher 增量同步复用同一文件范围判断，排除兄弟 workspace。 |
| F11 | embedding 请求携带 chunk 内容 hash，写回事务同时检查模型身份与当前 hash；迟到结果不会覆盖新正文的向量。 |
| F12、F13 | 正文相同时仍刷新位置，保留有效向量；search 对外输出从 1 开始的闭区间。真实 search/get 两个工具共享 mock 后端验证行号契约，另测实际 MemoryBackend 检索与读取。 |
| A2 中已复现的 claim 问题 | claim 增加 owner token，释放时比较本次所有权；旧 owner 不能清除已被接管的 claim。 |

A1 的通用编辑提交步骤、A4 的投影性能优化及更大范围分类重构继续作为独立架构任务。当前修复收敛了必要的解析、范围与写入不变量，没有引入新 actor、持久化层或通用编辑引擎。架构建议不等于已确认的功能缺陷，也未声称这些后续重构已完成。

修复后的核心回归：chat-state **461 passed**、tools **2,333 passed / 7 ignored**、memory **302 passed**、plugin-marketplace **103 passed**，合计 **3,199 passed / 0 failed / 7 ignored**。其中 tools/chat-state 的通过结果在 [核心日志](/tmp/grow-review-20260907/fix-core-tests.log)；该次运行尚有 Memory/插件 fixture 失败，修正后的完整 Memory/插件通过结果见 [重跑日志](/tmp/grow-review-20260907/fix-memory-plugin-tests.log)。未把中间失败日志误记为整批成功。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline --no-fail-fast \
  -p tools -p memory -p chat-state -p plugin-marketplace --lib --quiet -- --test-threads=2
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline --no-fail-fast \
  -p memory -p plugin-marketplace --lib --quiet -- --test-threads=2
```

磁盘控制：同一时刻只有一个 Cargo 构建，复用仓库 target，不改变 profile/RUSTFLAGS；删除审查专用 `/tmp/grow-review-20260907/memory/target` 约 428 MiB，保留复现源码和日志。

补充集成验收全部通过（按测试命令分别计数）：

| 定向范围 | passed | 日志 |
| --- | ---: | --- |
| Shell Trajectory | 40 | [Trajectory](/tmp/grow-review-20260907/fix-shell-trajectory-tests.log) |
| Shell trace_classifier | 27 | [离线分类器](/tmp/grow-review-20260907/fix-shell-trace_classifier-tests.log) |
| Shell laziness | 83 | [在线检测](/tmp/grow-review-20260907/fix-shell-laziness-tests.log) |
| Shell memory_flush | 26 | [Memory 集成](/tmp/grow-review-20260907/fix-shell-memory_flush-tests.log) |
| Pager loop_cmd | 18 | [/loop](/tmp/grow-review-20260907/fix-pager-loop_cmd-tests.log) |
| Agent plugins::git_install | 33 | [底层安装](/tmp/grow-review-20260907/fix-agent-plugins-git_install-tests.log) |

上述范围分别使用 `cargo test --locked --offline -p <crate> --lib --quiet <filter> -- --test-threads=2`，同样设置 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。Shell/Pager 为定向回归，不声称执行了它们的完整测试集。真实 Chrome 的 8 组交互回归通过，详见 [Trajectory 验收记录](/Users/lordcasser/workspace/projects/grow/docs/architecture/trajectory-review-2026-09-07.md)。

最终 `rustfmt --check`（本次涉及的 24 个 Rust 文件）和 `git diff --check` 通过。验收结束时磁盘可用约 **16 GiB**；未触碰其他任务的既有修改，没有提交 Git commit。F1–F20 及单独确认的 claim 所有权问题均已完成修复。独立架构建议和外部 trace 采集端的测量补齐，仍按各自边界记录，不计作已完成重构。
