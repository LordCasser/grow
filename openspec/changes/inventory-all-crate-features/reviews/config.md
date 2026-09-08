# config 逐包核查

包路径：`crates/codegen/config`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读；cargo test --locked -p config 通过71项测试（0失败、0忽略），本地日志 /tmp/grow-config-tests.log。

## 模块与开关

- `crates/codegen/config/Cargo.toml`
- `crates/codegen/config/src/campaigns.rs`
- `crates/codegen/config/src/config_override.rs`
- `crates/codegen/config/src/fs_atomic.rs`
- `crates/codegen/config/src/global_hook_sources.rs`
- `crates/codegen/config/src/global_hook_sources_tests.rs`
- `crates/codegen/config/src/lib.rs`
- `crates/codegen/config/src/loader.rs`
- `crates/codegen/config/src/managed_text/format.rs`
- `crates/codegen/config/src/managed_text/mod.rs`
- `crates/codegen/config/src/managed_text/source.rs`
- `crates/codegen/config/src/managed_text/tests.rs`
- `crates/codegen/config/src/managed_text/transaction.rs`
- `crates/codegen/config/src/managed_text/validator.rs`
- `crates/codegen/config/src/paths.rs`
- `crates/codegen/config/src/shell.rs`
- `crates/codegen/config/src/version_overrides.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Environment boolean vocabulary](../specs/configuration-rules/spec.md#requirement-environment-boolean-vocabulary)：环境布尔解析 SHALL trim并忽略ASCII大小写，识别1/true/yes/on/enabled与0/false/no/off/disabled。
- [Grow home and executable paths](../specs/configuration-rules/spec.md#requirement-grow-home-and-executable-paths)：Grow home SHALL 由字符串GROW_HOME或默认home下.grow构造并OnceLock缓存；默认home先canonicalize，无home回退相对路径，mkdir失败不阻止返回。
- [Session cwd encoding and marker decoding](../specs/configuration-rules/spec.md#requirement-session-cwd-encoding-and-marker-decoding)：CWD编码 SHALL 在URL编码长度不超过255时使用编码文本，否则采用最多40字符slug加BLAKE3前16hex；空slug用workspace。
- [Basic atomic file replacement](../specs/configuration-rules/spec.md#requirement-basic-atomic-file-replacement)：基础原子写入 SHALL 使用同目录basename.pid.counter.tmp，create_new后write_all再rename，支持可选Unix mode。
- [User configuration loading and expansion](../specs/configuration-rules/spec.md#requirement-user-configuration-loading-and-expansion)：配置加载 SHALL 对缺失文件返回空table，对其他I/O或TOML语法错误记录并返回；字符串值递归展开环境变量而不展开键，随后应用版本覆盖。
- [Hook configuration layer loading](../specs/configuration-rules/spec.md#requirement-hook-configuration-layer-loading)：用户Hook配置层 SHALL 单独读取user配置、应用版本覆盖并提取hooks table，保留provenance/source_name/path。
- [Recursive configuration patch semantics](../specs/configuration-rules/spec.md#requirement-recursive-configuration-patch-semantics)：配置合并 SHALL 只在两侧均为table时递归，否则整值替换；按patch迭代顺序后者覆盖，canonical strip只移除顶层version_overrides/campaigns/auth_provider/provider。
- [Version constrained configuration precedence](../specs/configuration-rules/spec.md#requirement-version-constrained-configuration-precedence)：版本覆盖 SHALL 先解析全部trim后的semver边界，再按minimum稳定升序应用匹配patch；缺minimum为0.0.0，上下界含等号，同minimum靠后声明获胜。
- [Campaign extraction and priority](../specs/configuration-rules/spec.md#requirement-campaign-extraction-and-priority)：Campaign SHALL trim且要求非空id，忽略空patch，数组结构错误时忽略整组；merge按source顺序首次id胜出，filter去除dismissed并保序，apply逆序使列表靠前者赢leaf。
- [Campaign enablement and dismissed state](../specs/configuration-rules/spec.md#requirement-campaign-enablement-and-dismissed-state)：Campaign启用 SHALL 同时受GROW_CAMPAIGNS和base.features.campaigns控制，任一显式false均禁用。
- [Platform shell selection](../specs/configuration-rules/spec.md#requirement-platform-shell-selection)：Unix shell种类 SHALL 仅由SHELL是否包含zsh选择Zsh或Bash；每种路径缓存按同basename且可执行的GROW_SHELL、SHELL、which、固定目录、硬编码路径依次选择。
- [Shell invocation and capability helpers](../specs/configuration-rules/spec.md#requirement-shell-invocation-and-capability-helpers)：Shell辅助 SHALL 分别返回Unix utilities、ampersand、chain separator与调用参数；命令存在性使用父进程PATH的which。
- [Global hook source inventory](../specs/configuration-rules/spec.md#requirement-global-hook-source-inventory)：全局Hook来源 SHALL 在无home时返回空；有home时始终列出固定hooks目录及hooks-paths registry，registry本身不参与目录发现。
- [Hook source path and file constraints](../specs/configuration-rules/spec.md#requirement-hook-source-path-and-file-constraints)：Hook slots SHALL 创建并要求真实hooks目录和registry文件，新registry在Unix使用0600与NOFOLLOW，现有权限不收紧。
- [Hook ancestor pin planning](../specs/configuration-rules/spec.md#requirement-hook-ancestor-pin-planning)：Hook祖先计划 SHALL 收集真实现存目录链，在缺失、symlink或非目录停止，去重按深度排序，跳过已经mount的目录但继续祖先。
- [Managed configuration planning and inspection](../specs/configuration-rules/spec.md#requirement-managed-configuration-planning-and-inspection)：ManagedConfig SHALL 在plan验证request和source，生成不可变updated bytes、目标路径与inspection；requested item状态为Absent、Exact或NeedsUpdate。
- [Managed configuration source snapshots](../specs/configuration-rules/spec.md#requirement-managed-configuration-source-snapshots)：托管配置source SHALL 要求普通文件、metadata大小不超过4MiB、无NUL且UTF8；snapshot比较bytes、BLAKE3、mode和identity。
- [Managed parent anchoring](../specs/configuration-rules/spec.md#requirement-managed-parent-anchoring)：托管配置父路径 SHALL 捕获现存真实目录链，apply创建缺失目录后重验既有前缀并建立anchor。
- [Managed request and marker grammar](../specs/configuration-rules/spec.md#requirement-managed-request-and-marker-grammar)：托管文本请求 SHALL 使用非空无CR/LF的comment prefix；namespace、owned prefix和item name仅允许ASCII字母数字及点、下划线、横线、空格，items非空且名称唯一，body禁CR与marker候选行。
- [Managed newline and item preservation](../specs/configuration-rules/spec.md#requirement-managed-newline-and-item-preservation)：托管文本渲染 SHALL 拒绝裸CR与混合LF/CRLF，默认LF并保留原文件换行形式、外围文本与未请求item。
- [Managed transaction reservation and publication](../specs/configuration-rules/spec.md#requirement-managed-transaction-reservation-and-publication)：变化的托管配置apply SHALL 获取持久同级.grow.lock的阻塞文件锁，重验source后create_new保留backup/temp，写入bytes和Unix mode并sync，再运行可选validator及重验后rename发布。
- [Managed transaction verification and recovery](../specs/configuration-rules/spec.md#requirement-managed-transaction-verification-and-recovery)：托管配置发布后 SHALL 重验父目录、同步并核对目标bytes和mode，失败则恢复原文件或删除新目标并再同步验证。
- [Managed syntax validator lifecycle](../specs/configuration-rules/spec.md#requirement-managed-syntax-validator-lifecycle)：托管配置validator SHALL 将temp路径追加给program/args，null stdio并detach，按10ms轮询退出；非零退出失败，group建立或attach失败可降级。

## 边界

- 返回None，不擅自选true或false。
- 指向home下bin的grow或grow.exe。
- user_grow_home的var_os检查不等于grow_home字符串解析；已缓存路径不重新计算。
- 读取目录中的.cwd，拒绝末端目录链接及marker链接、非文件、超过1MiB；Unix使用NOFOLLOW和限长读取。
- trim后返回，不校验hash对应或绝对性；sessions目录构造不负责写marker，父路径竞态未完全排除。
- 返回错误并尝试删除tmp；当前create_new碰撞失败也会删除同名tmp，不保证清理对象归属。
- 不额外创建父目录、fsync或检查目标是否并发变化；mode仍受umask影响。
- 按字符计算1-based行列，保留错误。
- 只读取user home配置，不在此层合并项目配置；环境展开使用no_errors语义。
- 不展开环境变量，不扫描项目或应用campaigns。
- 警告并跳过，不构造有效hooks层。
- 后一个整数组替换，不逐元素合并。
- 非table祖先触及全部后代，空路径不触及；take_patch_array先移除section，失败也已消费。
- 不合并任何patch，但version_overrides section已经移除。
- 前者不匹配；后者剥离section并成功返回，不应用覆盖。
- 提取user campaigns后按dismissed应用，不隐式调用merge去重。
- 使用patch祖先替换语义，祖先非table替换也计入。
- 仍禁用。
- 视为无dismissed id；本层加载不负责持久化dismiss操作。
- 可能运行detached且null stdio的--version，当前没有deadline；环境路径可以是相对路径。
- 识别显式pwsh/powershell/bash别名/cmd，自动依次where pwsh、固定PowerShell、Git Bash、PowerShell fallback；where也无deadline，显式部分值不验证存在。
- 分别-c并给MSYS转换禁用变量、-NoProfile -NonInteractive -Command、/C；Python UTF8环境由调用方实际应用。
- Unix/pwsh/Git Bash为&&，PowerShell和Cmd为分号；此返回值不证明Cmd分号具备预期连接语义。
- trim后忽略空行和相对路径，按词法路径首次去重；缺失configured路径按目录处理。
- 保留固定来源并标记incomplete；拒绝symlink模式遇见链接返回硬错误，不交付部分列表。
- metadata错误使检查停止，六个/tmp、/var、/etc及/private对应路径被豁免；检查非handle-relative，不保证消除竞态。
- 只列一层非隐藏小写.json且名称长度大于5，排序；直接校验拒绝链接、非普通文件及Unix多硬链接，不解析JSON，现有configured普通文件不作为目录扫描。
- 据此识别mount，mountinfo路径不解码八进制转义。
- 其他平台mount检测返回false；本层只产生计划，不执行挂载。
- unmanaged_text排除整个outer，render保留未请求项目；managed_block来自updated内容。
- 重新验证parent、路径解析与source；backup/temp hints只是候选，实际路径由apply返回。
- 初始缺失可创建，最终链接最多40次且检测循环，跟随后dangling拒绝；初始父链接经canonicalize物理化。
- Unix使用dev/ino及07777 mode，其他平台len/mtime；metadata与read分离，实际read不再次限长，不能据此保证无TOCTOU。
- 拒绝继续发布。
- Unix同步打开的目录，非Unix为空操作；路径metadata与打开句柄没有原子身份绑定。
- 拒绝；outer至多一对，item名称必须属于owned prefix且不等于namespace。
- 拒绝，即使不属于本次请求；候选要求行首comment prefix后有空格/tab再接chevrons，普通嵌入文本保持惰性。
- 已有则替换，无则插outer关闭前，无outer则追加；body尾部LF去除后转换为文件换行。
- 新文本保持无末尾换行；Exact按去掉section尾CR/LF后的规范文本比较，空文件新块也无末尾换行。
- 碰撞最多128次尝试且不删除碰撞文件；NoChange仍确保及重验parent/source，但不锁、不运行validator、不重写。
- 锁无超时且open不做NOFOLLOW/type校验；保留mode不保证owner/ACL，新目标Unix mode为0644，不是持久事务日志。
- 尽力清理本次backup/temp并返回错误，父目录及锁文件可保留。
- 回滚成功删除backup返回原错误；回滚失败返回Recovery包含两类错误并保留backup；成功apply保留实际backup，不保证排除外部并发覆盖。
- 尝试group terminate、50ms后kill，再child kill及最多1秒reap，清理错误附带返回。
- Unix不显式清理仍活后代；不捕获stderr，不保证spawn本身有界，不把group失败降级当完整进程树清理。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 已确认事实，待全包合并为 delta

- `lib.rs::env_bool`：trim + ASCII lowercase；1/true/yes/on/enabled 为 true，0/false/no/off/disabled 为 false；空、未设置、非 Unicode 或未知值为 None。
- `paths.rs`：default_grow_home 将找到的 home dunce canonicalize 后追加 .grow，找不到 home 用当前相对目录；grow_home 使用字符串 GROW_HOME 或默认，OnceLock 缓存，mkdir 错误忽略。user_grow_home 先用 var_os/home_dir 判断可解析再调用 grow_home，非 Unicode GROW_HOME 与缓存存在边界，不能推导永不回退相对目录。application 路径为 bin/grow 或 grow.exe。
- CWD 路径编码：URL 编码 <=255 字节原样；超限使用 ASCII leaf slug（最多40字符）+ BLAKE3 前16 hex，总长<=57，空slug用workspace。decode 拒绝末端目录symlink，先识别 URL 解码的 Unix绝对路径/本平台Windows冒号形式，否则读 .cwd；marker拒绝symlink/nonfile/>1MiB，Unix open O_NOFOLLOW且再次限制读长，内容trim但不验证绝对路径或hash对应。父目录替换仍非handle-relative；sessions_cwd_dir只构造路径不写marker。
- `fs_atomic::write_atomically`：同目录 basename.pid.counter.tmp，create_new，可选Unix mode，write_all后rename，错误时尝试remove tmp；无父目录创建/fsync/父目录fsync/CAS，mode被umask约束。create_new碰撞失败后仍remove该tmp，潜在清理非本次所有文件，待独立审计。
- `loader`：缺配置文件返回空table，其余I/O/语法错误记录后返回，语法定位按字符计1-based行列。load_toml_file递归展开值字符串（含数组、不展开键），使用shellexpand env no_errors；load_config_file随后应用版本覆盖；load_from_disk只调用user_grow_home。project overlay不由此函数实现。
- hooks layer单独读取user config，不展开环境变量；应用version overrides，读取hooks table，错误或非table警告跳过；不扫描project，不应用campaigns。HookConfigLayer保留provenance/source_name/path/hooks；new把source_name同时转PathBuf。
- `deep_merge_toml`：两侧均table则逐键递归，其他情况整值替换（含数组）。
- `config_override`：先remove根key，再反序列化metadata+patch数组，失败已消费原section。patch_touches_path认为任何非table祖先替换都会触及后代；空path false。apply_patches按迭代顺序later wins，仅去除指定顶层键。canonical strip keys为version_overrides/campaigns/auth_provider/provider，保留普通model选择。
- `version_overrides`：所有trim后的semver上下限先解析，缺minimum=0.0.0；解析失败前不合并任何patch，但section已经移除；稳定按minimum升序，同minimum声明靠后者覆盖；上下限含等号，min>max不单独报错只无法匹配。installed_semver失败时直接剥离section并返回成功。
- `campaigns`：取数组反序列化失败警告并忽略全部；id trim且必需非空，空patch跳过；merge函数按source顺序首次id胜出；filter去dismissed保序；apply逆序合并所以列表靠前者赢leaf；ids_touching_paths检查上述祖先替换。ConfigLayers::load仅抽user campaigns，effective_disk_only按dismissed应用，未调用merge去重。
- campaign gate：env GROW_CAMPAIGNS显式false或base.features.campaigns=false任何一个都禁用，env true不能覆盖本地false；dismissed存home/campaigns_state.json，读取/解析失败默认空set。仅加载，不在这里持久化dismissed。
- Unix shell：detect kind仅SHELL字符串含zsh才选Zsh，否则Bash，不读GROW_SHELL也不缓存kind。每kind路径OnceLock：GROW_SHELL同basename且executable > SHELL同条件 > which > /bin,/usr/bin,/usr/local/bin,/opt/homebrew/bin > 硬编码/bin/name。环境值可为相对路径，并非总绝对。executable先metadata任意x位；否则detached/null stdio执行--version，无deadline，不能保证探测有界。
- Windows（代码cfg为not(unix)）：GROW_SHELL识别pwsh/powershell/bash|gitbash|git-bash/cmd|cmd.exe；明确pwsh/powershell/cmd不检查存在，bash找不到继续探测。自动where pwsh.exe >固定powershell路径 >Git Bash候选/where包含git路径 >PowerShell fallback；函数注释cmd fallback过时。探测where也无deadline。
- Shell traits：Unix或GitBash报告Unix utilities；命令存在性另用which读父进程PATH。chain separator Unix/pwsh/GitBash为&&，PowerShell和Cmd为;（Cmd真实语法适配另审计）。ampersand分POSIX background/PS Core/WindowsPS/Cmd separator。Windows invocation GitBash -c+MSYS_NO_PATHCONV=1/MSYS2_ARG_CONV_EXCL=*；pwsh/powershell -NoProfile -NonInteractive -Command；cmd /C。均返回PythonUTF8=1与PYTHONIOENCODING=utf-8:surrogateescape，caller需实际应用。

## 剩余模块补充事实

- global_hook_sources：没有 home 返回空。固定 hooks 目录和 hooks-paths 文件即使不存在也列出，registry 不参与目录发现。读取 registry 的非 NotFound 错误保留固定源并标记 incomplete；trim 行，仅保留绝对路径，按词法路径首次去重，未存在 configured 路径视作目录。拒绝 symlink 模式遇见链接硬失败；组件 metadata 错误终止检查并返回未见链接，六个 /tmp、/var、/etc 及 /private 对应路径被无条件豁免。
- hook slots 创建前后检查 home；hooks 必须真实目录，registry 必须真实文件，新文件 Unix 0600 + O_NOFOLLOW，现有 mode 不收紧。检查没有基于父目录句柄串行完成，registry hardlink 不拒绝。
- hook 文件发现只列一层，名称必须非隐藏、小写 .json 且长度>5，排序但不预筛类型；直接文件校验拒绝 symlink/nonfile，Unix nlink 必须为1，不解析 JSON。validated_files 仅处理目录源，结果排序去重，配置的现有普通文件不参与。
- ancestor 辅助返回真实现存目录链，遇缺失或 symlink/non-dir 停止；pin 计划去重、按深度排序，跳过已经 mount 的目录但继续祖先。Linux 根/dev变化/mountinfo 判定 mount，mountinfo 路径未做八进制解码；其他平台返回 false。这里只计算计划，不执行挂载。
- managed_text plan 验证 request，解析词法绝对路径及最终 symlink、捕获 parent/source，形成不可变 updated bytes 与 typed inspection。只报告请求 item 的 Absent/Exact/NeedsUpdate；unmanaged_text 删除整个 outer 区块，不只是请求项目。backup/temp hints 不保证实际保留路径。verify_unchanged 重验 parent、目标解析结果和 source。
- source：最终链接最多40次并检测循环，初次 missing 可以创建，跟随后 dangling 拒绝；先 canonicalize 可存在的父路径，故初始父 symlink 会物理化，并非一律拒绝。source 要求普通文件、metadata 大小<=4MiB、无NUL和UTF8，实际 fs::read 不再次限长。snapshot 比较 bytes/hash/mode/identity；Unix dev+ino，其他平台 len+mtime。Unix mode 保存07777。metadata/read/open 分离，不能据此声明无TOCTOU。
- parent plan 捕获现存祖先，apply 创建缺失父目录并重验前缀；anchor 保存路径身份与打开目录，Unix sync_all，非Unix sync为空操作。文件句柄身份没有与先前 metadata 绑定核对。
- format：comment prefix 非空且无CR/LF；namespace、owned prefix、item name 只允许ASCII字母数字、点、下划线、横线、空格，不trim。items 非空且名称唯一，body 禁CR或marker_candidate 行。没有在validate_request直接检查 item 属于prefix，但最终parse拒绝外来item。
- markers 必须精确行首 prefix + 单空格 + >>>/<<< + 空格name + 空格同向符号。候选要求prefix后至少空格或tab且trim后以chevrons开头；候选含namespace或owned prefix的畸形行拒绝，嵌入普通文本不识别。outer至多一对且有序；内部允许空白与prefix命名item，拒绝裸内容、嵌套、重复、错配或未闭合；outer外任何owned prefix item拒绝，与本次请求项无关。
- newline 全文件检测：裸CR、LF/CRLF混合拒绝；无换行默认LF。item body尾部LF全去掉再转换换行；逐item重解析、替换已有或插入outer关闭前，保留未请求item与外围字节。无outer时追加；保留原文件是否末尾换行，空文件创建不带末尾换行。Exact比较去掉区块末尾CR/LF后与规范section一致。
- transaction：无变化先parent确保/revalidate后NoChange，不锁、不校验器、不重写。变化使用持久同级.grow.lock、阻塞File.lock，无deadline，open未NOFOLLOW/type检查。锁后重验；backup/temp create_new碰撞重试最多128，原文件bytes与mode写backup并sync，temp同样同步；可选validator后重验并rename。
- publish之前错误尽力清理本次backup/temp；发布后重验父、sync、校验bytes/mode。失败回滚旧文件或删除新文件，再sync/verify；回滚成功删backup返回原错误，失败返回Recovery保留backup。成功返回实际backup路径，backup保留。只保存mode不承诺owner/ACL，非持久事务日志；外部并发写入和路径竞态不完全排除。
- validator program+args追加temp，null stdio并detach；group创建/attach失败降级，10ms try_wait；成功退出即返回，非零失败。timeout从spawn/attach之后计时，超时/wait错误先group terminate、等50ms、kill，再child kill并最多1秒reap；teardown错误附带。Unix正常退出不显式清理仍活后代，Windows job Drop行为另见tty-utils；不承诺spawn本身有界。
- tests：缺失/空/普通/无末尾换行/CRLF、旧item保留、标记拒绝、UTF8/NUL/大小、最终symlink/循环/深度/父物理化、mode+backup、stale源/父替换、新父symlink、碰撞重试、7处precommit和4处postpublish注入、真实publish/sync注入、校验失败与超时、锁竞争后stale拒绝。注入及本平台测试不证明所有OS或敌对路径竞态安全。

全部16个Rust文件及manifest已经阅读，23项要求映射完成。测试在macOS独立工作树执行，未执行Windows和其他平台验证。

