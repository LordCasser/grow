# sandbox 逐包审阅

本阶段读取manifest、types.rs、logging.rs全部，lib.rs推进560行。其他模块、剩余lib、集成测试和smoke示例待核对；不计reviewed。

- 默认enforce marker feature；nono固定=0.53.0、globset为非optional Unix依赖，Linux ignore同样不随feature移除；内核代码cfg enforce+unix，非Unix无内核实现。smoke example required-features enforce。版本钉住与Seatbelt规则顺序相关，不能在仅编译通过下假设升级等价。
- Global SANDBOX和CONFIGURED_PROFILE各OnceLock，后续set失败忽略，安装不更新已有全局；AUTO_ALLOW_BASH relaxed bool且必须is_active才生效。is_inside_bwrap仅环境变量能Unicode读取即true（包括空值），trust_bwrap_marker_for_devbox固定false，不能把marker视为隔离证明。
- requested_confinement_profile来自配置请求且过滤Off，不代表apply成功。is_active/profile_name检查applied；metrics和log_violation实际仅检查全局已安装，未检查applied，尽管注释声称inactive无日志/metrics。
- SandboxManager::new只保存profile/初始网络配置，不使用workspace；enforcing apply遇Off直接Ok。按需ensure hook slots和namespace lockdown错误传播，resolve/capability转换错误也传播；support不支持或Sandbox::apply错误则记录ApplyFailed并Ok，applied仍false。因此Result Ok不证明内核隔离成功；调用方fail-closed决策需后续shell核对。
- apply成功记录applied=true，日志resolved deny为effective路径；此对象没有once调用防护。无enforce或非Unixstub仅日志并Ok。install先flush（忽略错误），再尝试全局set。child network开关只在applied && configured && Linux，表示已知启动点应安装过滤，并非所有后代网络被该函数自动限制。
- SandboxEvent默认serde字段snake_case、enum原variant名称，无deny_unknown_fields；可选字段省略。profile_applied构造器直接enforced=true并填路径，不能单凭构造事件证明内核状态；apply_failed enforced=false。Fs/Net/Bypass metrics relaxed计数，不提供一致联合快照，计数可wrap。
- Logger先增metrics再尝试Mutex记录，poison导致事件丢弃但计数保留；Vec无上限。flush先drain，再创建目录/open append/write JSONL，任何写失败已drain记录不回队；序列化失败跳过，无fsync/原子批次/轮转/跨进程锁保证，日志路径config::grow_home/sandbox-events.jsonl。
- Linux bwrap构造先检查marker，取current_exe与Unicode args，命令为PATH中bwrap、cap-drop ALL、bind / /，已存在optional写禁路径ro-bind，hook plan binds，读禁用000占位覆盖，随后dev/proc挂载及marker1再exec原参数。只返回Command不执行；None混合“不需要/已内部/构造失败”，调用方必须区分需求。非Linux入口None。
- 占位文件/目录位于grow_home加PID，复用同类型并chmod000，异类型删除重建；按metadata/exists会跟随symlink，无随机独占/no-follow，PID仅避免不同活进程通常竞争，不证明可信占位身份；本层未见清理。
- requires_read_deny在enforce Unix只看Custom原始deny非空，不看展开结果，避免展开失败误成无需限制；stub false。devbox_based只认Devbox或Custom直接extends devbox。requires_hook_write_deny也按直接extends豁免，完整profiles解析后再核对递归继承限制。
- Linux deny plan展开glob是启动快照，warning说明后建文件不覆盖；分割exact/glob并受deny模块预算，展开失败None。hook需求但无plan拒绝；无enforce Linux仍可构造devbox/data写禁挂载，读禁为空。lib其余部分未读，不推导完整bwrap验证行为。

## lib收尾、paths与profiles起始

补读lib561–末尾，lib完整；paths完整；profiles推进250行。

- lib测试主要检查bwrap Command参数，并没有执行该命令证明mount生效；覆盖缺失optional写路径跳过、缺失读路径仍bind占位、目录占位类型、祖先bind先于leaf、叶身份更换拒绝、devbox+deny组合。Linux限定测试不能以macOS运行替代；env guard只保存Unicode值，非Unicode原值被当None。configured_profile OnceLock测试依赖本进程首次写入。
- paths默认临时写目录/tmp、/var/tmp（此处不检查存在）；macOS额外存在的/private/tmp、/private/var/tmp、整个/private/var/folders。TMPDIR若存在且目录则加入，不要求绝对、可信根或canonical去重。可写profile路径为workspace+grow_home+temp；read-only仍允许grow_home+temp，不能描述为全文件系统绝对不可写。设备文件表null/zero/random/urandom/tty/ptmx，目录pts/fd，实际授权后续capability处理。
- ProfileName默认Workspace，精确解析workspace/devbox/read-only|readonly/strict/off|none，其他任何字符串（含空串/大小写差异）作为Custom，FromStr本身不失败。故requested_confinement_profile对未定义名字也可能返回请求，真正定义检查延后。
- ProfileConfig extends/network Option及三路径列表默认空，SandboxConfig profiles默认空，均无deny_unknown_fields。global与project加载read错误静默None，TOML坏文件warning后None。global先载入，project只补未存在的名字，冲突不会覆盖；若global读失败则不能据此保证保留其权威定义。
- sandbox_profile_conflicts重新读取两文件，仅报告双方同名Custom且ProfileConfig结构不等，排序返回；内建/alias名排除，不做路径展开后语义等价比较。read错误默认空，所以无冲突报告不证明两个文件都成功加载。
- device_file_openable通过File::open，仅NotFound/ENXIO/ENODEV返回false，其他错误仍尝试grant，避免无tty导致整套Landlock失败；不是权限可用的完整预检。to_capability_set Off返回空CapabilitySet而非执行sandbox；通常路径resolve后转capability，default_read先grant / Read，read_only跳不存在及非UTF8。其余授权逻辑待补读。

## profiles完整阅读

补读profiles251–末尾，生产逻辑及测试全部覆盖。

- capability转换read_only不存在或非UTF8跳过；read_write不存在先create_dir_all，创建失败warning后跳过，非UTF8也跳过。已有文件仍走allow_path，错误传播；未按workspace拼接自定义read_only/read_write，原PathBuf由当前进程相对路径语义和nono处理，不能等同deny路径的workspace解析。路径授权没有与deny先行去重或最小化。
- DEVICE_FILES逐个openable筛选并allow_file_mut，grant错误warning继续；DEVICE_DIRS存在且目录则allow_path错误传播。此处不把restrict_network写入CapabilitySet，网络开关留给child launch；不能把Strict解析成功理解为进程级网络封禁。
- write_deny先将typed sources及validated_hook_json_files_for_sources补充成路径/目录对，JSON alias验证失败传播；再调用写禁平台实现。read+write deny分exact/glob，exact先effective，glob平台转译，Linux实际读禁依赖bwrap，后续deny模块核对。
- Workspace default_read=true、workspace+状态+temp可写、网络开；ReadOnly同样全局可读，状态+temp可写、网络限制配置true；Strict default_read=false，但显式读取/usr /lib /lib64 /bin /sbin /etc /dev /proc /sys /tmp /run /var /System /Library /private及存在的home/Library，另workspace/state，后两也可写。并不是只读workspace或仅最少证书路径。
- Devbox枚举根目录所有目录，排除/data /proc /sys /dev，另无条件workspace可写，全局read，write_deny空。枚举失败或坏dirent静默忽略；路径is_dir跟随链接。若workspace在/data，read_write仍含workspace，/data保护最终依赖平台附加实现；本模块并不通用拒绝这种配置。
- Custom必须存在配置，默认继承Workspace；只允许内建base，拒绝Off/none及其他Custom，故没有多层递归继承。改name，network Option覆盖，read_only/read_write/deny都是追加，不替换已有base grant；read_only不能削弱base已给的write。继承Devbox明确清空hook write_deny。
- 测试覆盖parse/display、内建及继承network、strict系统路径、capability构造、devbox不把/data继承成read-deny、缺Custom、global first-wins威胁表、Off错误、设备open筛选。多个profile测试先读真实host hook sources，无法解析时打印skipping并返回；必须从nocapture区分实际运行。strict_capability_set_builds_without_openable_dev_tty只构造capability，不强制移除控制终端且不apply；不能当内核强制隔离证明。

## deny精确路径完整、glob起始

完整读取deny/mod.rs458行，glob.rs推进240行。

- effective_deny_paths仅workspace join、排序和去重，注释canonical set不等于filesystem canonicalize或词法..归一；Linux字符串转换用display可能lossy。目录判定按当前is_dir，未存在路径当文件；macOS日后变目录不会自动对子树生成subpath。
- macOS deny aliases覆盖原路径、canonical成功时路径、二者/tmp /var /etc与/private对应形式；canonical失败回退原文。escape拒绝非UTF8和所有control，转义反斜杠与双引号，失败传播不静默漏规则。
- read+write deny添加file-read*、file-write*与八具体write动作(data/create/unlink/mode/owner/flags/times/setugid)，意图抵消nono后发宽write allow。源码明确这是实测规则行为，不是普遍可推导的优先级；应保留真实macOS e2e要求。目录用subpath、文件literal，移除同路径exact file caps覆盖冲突；不自动保护read-deny所有祖先节点。
- Hook write-only deny保持read：typed目录或当前目录同时literal+subpath，文件literal；补祖先的create/unlink literal，防止节点替换。祖先选择基于原路径前缀，取最深包含的writable root，只覆盖existing ancestor chain和存在root，不在根内则无祖先规则；alias在发射阶段补齐。nested writable roots下不是全部上级都被本helper锁定。Linux这两类capability deny函数本身no-op，依赖bwrap。
- mod测试验证祖先选择、join/sort/dedup、escape和private alias，未在这些单元测试里apply内核。
- glob仅含* ? [判为模式；非UTF8进入exact，纯{a,b}没有这三字符时也作literal。相对glob根为workspace且整串为tail；绝对glob取首个含元字符组件之前为root，其后为tail。验证拒绝brace/backslash、非独立组件**、字符类首成员]及nested[/POSIX类，再用globset literal_separator验证；这里不是完整gitignore语法。
- macOS手写转换**/可选目录前缀、**任意、*非斜杠、?一个非斜杠，类!/^转否定；普通regex元字符转义。root加canonical/private aliases，各regex首尾锚定；根非UTF8形式跳过。真正platform发射、Linux遍历预算及测试尚待读，不从注释“always matches”推导全部输入的跨平台等价。

## deny glob完整阅读

补读glob241–843，整个模块及测试已覆盖。

- macOS每个模式验证后生成所有root alias锚定regex，空regex列表或无法表达控制字符返回Err，发射read+具体write deny；不像exact路径，不移除可能冲突的exact file caps。注释“平台规则在read/write allows之后”与parent记录的write-allows后发不完全一致，不能依赖这句概述证明优先级，保留实际e2e要求。
- Linux apply_deny_globs_to_capability_set仅日志/no-op，连validate也不在该分支执行；validate实际在bwrap展开路径。因此直接capability构造Ok不证明Linux glob合法或生效。
- Linux展开默认depth64、unique matches4096、visited entries200000。所有glob先validate并编译成绝对匹配，relative前缀escape workspace（workspace字符串lossy）；roots仅BTreeSet精确去重，不合并祖先/后代，所以重叠root可能重复访问计数。返回匹配路径BTreeSet排序去重。
- 每root.exists false跳过，不细分不存在与metadata权限/其他失败。walker关闭standard/hidden过滤且follow_linksfalse；成功dent计visited，超预算拒绝；depth达到cap且dent是目录就拒绝，即便该目录实际空、或pattern不可能匹配更深项，属保守拒绝。match不限制普通文件，目录和symlink本身可能入集；非UTF8匹配拒绝。
- walk PermissionDenied warning跳过，其余IO或非IO错误拒绝；这只依赖当时权限，不能保证后续权限改变仍不可读。没有wall-clock deadline，条目上限不限制单次IO/DNS挂载卡顿。启动后新匹配路径不覆盖；目录bind与macOS仅锚定匹配节点对其后代的具体效果待e2e核对。
- macOS parity单元用15种segment生成单/双组件pattern，与23种ASCII路径做交叉比较，使用Rust regex库而非Seatbelt运行时；不能称所有pattern、Unicode、换行及真实内核都已等价验证。nono接受规则测试也只是构造验证。Linux测试覆盖匹配/空集合/三预算/非法模式/不下钻symlink/错误分类/非UTF8；权限错误分类为构造Error，不是真权限变化集成。当前macOS未执行Linux分支。

## child_net完整与network_policy起始

完整读取child_net.rs全部代码及测试；network_policy推进260行。

- Linux child网络classic BPF先检查audit arch，再检查x86_64 x32位，异常均EPERM；支持常量仅x86_64/aarch64，其他架构EXPECTED_ARCH=0，不是可用的通用架构实现。阻断connect/bind/sendto/sendmsg/sendmmsg/listen/accept/accept4及io_uring三入口，共11项；其余ALLOW。不检查socket地址族，Unix domain也受这些syscall限制；不阻断read/write到已继承连接，不是完整网络FD隔离。
- install_child_network_filter必须调用方在fork后exec前调用，先NO_NEW_PRIVS再PR_SET_SECCOMP；无TSYNC，只应用当前调用线程及后代继承。函数自身不查全局profile/enforce feature；调用点负责选择。构造Vec发生在此入口，实际fork安全边界需与调用方式核对。
- namespace lockdown阻断全部unshare/setns，clone3统一ENOSYS让libc回退，legacy clone仅检查args0低32位的八NEW namespace标记，普通clone允许。安装用seccomp TSYNC，rc>0作为失败TID返回错误、-1 errno；前置NO_NEW_PRIVS已生效后失败不会回滚。没有once guard。此filter不阻断一般mount等其他操作，不能脱离cap-drop及挂载配置声称完整namespace防逃逸。
- 两函数非Linux均Ok no-op且保留unsafe调用约束。全部单元测试Linux限定，用自制BPF解释器喂synthetic arch/syscall/flags，检查八socket+三io_uring、compat/x32、clone fallback、namespace bits和最终allow；本文件没有实际安装seccomp的动态内核测试。
- network_policy明确纯建模、未被当前profile选择或运行时执行；ChildNetworkPolicy tag mode/content policy snake_case有Unrestricted/Blocked/Websites，bool转换只产生前两种。
- WebsiteOrigin先严格raw http://或https://（小写prefix），拒绝ASCII空白/control/DEL、反斜杠、userinfo、wildcard、路径/query/fragment，仅可末尾单/。URL解析后只接受Domain，拒绝IP literal（包括URL规范化后的IP）；去一个尾点，ASCII小写/IDNA结果，DNS label 1..63且首尾alnum、中间alnum或-，总<=253；有效非零端口，display总包含端口。单label如localhost可通过，未作DNS解析/私网校验，不是HTTP hook SSRF规则。
- WebsitePolicy BTreeSet去重排序、deny优先allow再default，精确scheme/host/port匹配，不含子域/path/redirect扩展。serde经Origin parse约束；策略本身未deny_unknown_fields。NetworkPolicySnapshot版本1，private字段new封装，后续序列化/校验实现尚待读。

## network_policy完整与hook_write_deny起始

network_policy补读至末尾，全部类型/方法/8项测试完整；hook_write_deny推进280行。

- canonical_json是当前struct顺序+serde_json紧凑序列化及BTreeSet排序，不是一般JSON canonicalization标准。sha256对该UTF8文本哈希，validate_sha256仅忽略ASCII大小写比较，不检查签名、来源或常量时间。from_canonical_json先解析RawSnapshot version+Value，再拒绝非1版本，之后解析policy；不要求输入本身为canonical文本、未知字段不拒绝，结果可重新规范化。version必须先能解析u32，负值/超范围不是UnsupportedVersion而是InvalidJson。
- tests验证default port/case/尾点/IDNA、原始语法拒绝、deny优先和精确origin、排序去重hash、固定JSON与固定SHA256、错误版本先于未知policy mode、bool映射。没有DNS/连接/代理实施，本模型不能作为已上线website egress capability；只映射纯策略类型与快照能力。
- hook write保护基础predicate只排除Devbox/Off，不区分Custom extends；外层requires helper和profile resolve负责内建继承豁免。prepare会ensure slots再resolve，非Linux返回Ensured，不表示内核已保护；profile_hook_write_deny本身不ensure。
- resolve_hook_write_deny_snapshot传reject_symlinks=true解析global sources，configured_error或缺失配置路径失败，检查顶层regular hardlinks和Unix直接JSON别名；来源解析具体校验已在config审阅记录，后续本模块应用步骤仍待核对。
- capture/revalidate_path_identity对末级symlink_metadata，拒绝symlink；非目录nlink不为1拒绝（代码不只regular），记录dev/ino/is_dir/nlink/path，不含内容hash/mtime/owner/mode。revalidate这些字段必须相同，包括目录nlink；同inode原地改内容无法由identity检测。祖先是否retargetable由其他边界负责，不能凭末级no-follow声称整路径无race。
- enforcement_leaf_paths保留顶层来源再补validated直接JSON，按路径HashSet去重保留顺序；DirJsonSnapshot列直接JSON并逐个验证capture后按路径排序，保护的是文件集合/身份，不是内容摘要。build_bwrap_plan主体尚待读。

## Hook写保护完整与普通integration

补完hook_write_deny全部生产逻辑、独立hook_write_deny_tests全部6测试，以及integration_test.rs105行。剩deny_paths_e2e1057和smoke169行待读。

- build_bwrap_plan要求所有source存在，去重捕获leaf，typed且实际目录才捕获直接JSON快照并补leaf；祖先来自unique_ancestors_rootward，剔除也是leaf的路径。revalidate先核对全部leaf，再重列每目录JSON比较数量/path/dev/ino/nlink，再检查祖先非symlink且为目录。祖先未保存原dev/ino，不能声称祖先完整身份锁定；plan到实际bwrap执行仍有时间窗口。
- append仅在revalidate成功后追加祖先RW自绑定，再leaf RO绑定；它不执行Command、不持有用于bind的open FD。目录快照不比较文件内容，原地写入同inode不能通过此检查发现。叶目录nlink变化可能比JSON快照更早导致IdentityChanged。
- Linux path_is_effectively_readonly用statvfs ST_RDONLY，失败或NUL路径返回VerifyIo；验证每个leaf的挂载只读标志，不实际尝试写入，也不证明同inode别名/祖先规则/历史快照完全一致。
- verify_hook_write_deny_enforced先ensure_namespace_lockdown，再重新resolve当前sources和leaf检查只读。ensure使用OnceLock<Result>，首次失败也永久缓存，不重试；验证函数因安装seccomp带不可逆副作用，并非纯只读检查。非Linux入口直接Ok，不等于Seatbelt检测通过。maybe_install在基础predicate+marker条件安装，marker不独立证明bwrap。
- Hook测试覆盖目录inode替换、symlink替换、hardlink拒绝、Linux晚增hardlinked JSON、registry/目录JSON别名；晚增测试允许多种错误，未分别证明普通晚增与同inode内容改写。普通integration只检测support（不assert支持）、构造capabilities、Off apply、日志内存计数/drain、未安装global默认网络false，没有内核隔离测试，也未验证磁盘日志失败恢复。

## 全包阅读完成：e2e与smoke

完整读取deny_paths_e2e1057行与smoke169行，至此所有Rust/manifest已覆盖。

- e2e用当前test binary subprocess_entry ignored入口，父测试显式启动；HOME/GROW_HOME与workspace隔离，实际apply后检查is_applied（通用deny所有平台，hook场景只macOS要求）。Linux走真实profile bwrap路由；无需把主测试进程不可逆限制。
- support不足或Linux bwrap实际bind true失败默认打印skipping返回；REQUIRE环境变量可读取即要求失败（空字符串也算）。本轮设置REQUIRE=1，避免软跳过伪装已执行。非Linux marker spoof测试仍直接return，无日志；ignored subprocess入口由父调用不应单独--ignored运行。
- read断言只保证MARKER未暴露，任意读取失败或空占位均通过；child cat/sh只查stdout marker不查退出码。generic rename忽略rename结果只查目标marker，区别于hook rename要求具体拒绝错误。write必须EACCES/EPERM/EROFS，hook unlink还接受EBUSY、rename接受EXDEV/EBUSY。
- 覆盖exact三个路径含目录内文件、glob四目标及两对照，macOS后建late.pem写拒绝并非匹配control写成功。Hook覆盖现有/首次创建固定slots、configured目录在Grow和workspace内、父rename拒绝但兄弟可写、hook仍可读、hardlink和symlink拒绝；两个JSON拒绝测试仅非零退出，没有精确错误归因。
- Linux unshare测试确认binary存在，但接受退出码1就算拒绝，并未在无过滤条件先证明该环境允许该操作；root mount probe没有保证目标存在且spawn失败也通过。不能据此唯一归因seccomp/cap-drop。未包含child网络实际socket矩阵；需要Linux lane单独验证。
- subprocess output无超时，场景列表用逗号env分割，fixture名称受控；清理best-effort。smoke真实执行apply后打印read/write，但不assert预期、不累计失败退出码；结束总打印完成。固定测试文件名可能覆盖既有文件，且注释read-only阻止/tmp写与实际profile不符；本轮不执行此非断言型示例，不改生产行为。

## 映射完成（前文阶段待办为历史记录）

## 功能与规范映射

- [Sandbox enforcement feature availability](../specs/sandbox-boundary/spec.md#requirement-sandbox-enforcement-feature-availability)：Sandbox SHALL 默认启用enforce，内核实现限enforce+Unix，其他构建apply为stub；nono固定0.53.0。
- [Sandbox requested and installed state](../specs/sandbox-boundary/spec.md#requirement-sandbox-requested-and-installed-state)：Sandbox SHALL 分开保存配置请求与安装状态，OnceLock只接受首次值；auto_allow_bash还要求实际active。
- [Sandbox apply result boundary](../specs/sandbox-boundary/spec.md#requirement-sandbox-apply-result-boundary)：SandboxManager SHALL 传播配置/能力构造与Hook保护准备错误，但对后端unsupported或apply错误记录失败后返回Ok。
- [Sandbox profile configuration precedence](../specs/sandbox-boundary/spec.md#requirement-sandbox-profile-configuration-precedence)：Sandbox配置 SHALL 全局先加载，项目只能补新名称；冲突查询报告不同的同名Custom配置。
- [Sandbox built in profile resolution](../specs/sandbox-boundary/spec.md#requirement-sandbox-built-in-profile-resolution)：内建profile SHALL 区分Workspace全局可读与workspace/state/temp可写、ReadOnly全局可读及state/temp可写、Strict系统读取表加workspace写、Devbox根目录枚举写。
- [Sandbox custom profile inheritance](../specs/sandbox-boundary/spec.md#requirement-sandbox-custom-profile-inheritance)：Custom SHALL 默认继承Workspace，只允许继承非Off内建profile，追加read_only/read_write/deny并按Option覆盖network。
- [Sandbox filesystem grants and devices](../specs/sandbox-boundary/spec.md#requirement-sandbox-filesystem-grants-and-devices)：能力转换 SHALL 跳过不存在read路径，尝试创建缺失write目录，单独授权设备文件与目录。
- [Sandbox temporary writable roots](../specs/sandbox-boundary/spec.md#requirement-sandbox-temporary-writable-roots)：临时可写路径 SHALL 包括/tmp和/var/tmp、macOS存在的private临时根以及存在且为目录的TMPDIR。
- [Sandbox event and logger persistence](../specs/sandbox-boundary/spec.md#requirement-sandbox-event-and-logger-persistence)：SandboxLogger SHALL 累计事件与relaxed指标，flush将内存队列取出并追加JSONL。
- [Sandbox exact deny path resolution](../specs/sandbox-boundary/spec.md#requirement-sandbox-exact-deny-path-resolution)：精确deny SHALL 相对workspace拼接、排序去重；目录类型按当前is_dir决定。
- [Sandbox Seatbelt deny rule emission](../specs/sandbox-boundary/spec.md#requirement-sandbox-seatbelt-deny-rule-emission)：macOS SHALL 对原路径、canonical及private aliases发射read/write和八个具体write动作deny，拒绝无法表达路径。
- [Sandbox direct hook ancestor write protection](../specs/sandbox-boundary/spec.md#requirement-sandbox-direct-hook-ancestor-write-protection)：macOS Hook写禁 SHALL 保持读取，保护leaf并对最深包含write root内现存祖先发create/unlink deny。
- [Sandbox deny glob dialect](../specs/sandbox-boundary/spec.md#requirement-sandbox-deny-glob-dialect)：deny含*、?、[ SHALL 进入受限glob语法，拒绝brace/backslash、非独立组件**和不支持字符类。
- [Sandbox Linux deny glob expansion](../specs/sandbox-boundary/spec.md#requirement-sandbox-linux-deny-glob-expansion)：Linux glob SHALL 在启动时关闭ignore/hidden过滤且不下钻symlink，默认depth64、matches4096、visited200000。
- [Sandbox bubblewrap command construction](../specs/sandbox-boundary/spec.md#requirement-sandbox-bubblewrap-command-construction)：Linux bwrap SHALL 构造cap-drop ALL、根bind、optional只读write路径、Hook计划、read占位覆盖及dev/proc挂载，再reexec原程序。
- [Sandbox read deny placeholders](../specs/sandbox-boundary/spec.md#requirement-sandbox-read-deny-placeholders)：Linux读禁占位 SHALL 在Grow home使用PID后缀的文件或目录chmod000并ro-bind。
- [Sandbox required protection classification](../specs/sandbox-boundary/spec.md#requirement-sandbox-required-protection-classification)：保护需求 SHALL 按原始profile/config判定，Custom非空deny要求read deny，非Devbox/Off通常要求Hook写禁。
- [Sandbox hook source identity snapshot](../specs/sandbox-boundary/spec.md#requirement-sandbox-hook-source-identity-snapshot)：Hook写保护 SHALL 校验global来源、配置路径存在性、直接JSON别名和leaf dev/ino/type/nlink。
- [Sandbox hook mount plan revalidation](../specs/sandbox-boundary/spec.md#requirement-sandbox-hook-mount-plan-revalidation)：Hook bwrap计划 SHALL 重验leaf身份和目录JSON集合，再添加祖先RW及leaf RO绑定。
- [Sandbox hook readonly verification](../specs/sandbox-boundary/spec.md#requirement-sandbox-hook-readonly-verification)：Linux Hook生效检查 SHALL 先安装TSYNC namespace filter，再重新解析leaf并检查statvfs ST_RDONLY。
- [Sandbox child network syscall filter](../specs/sandbox-boundary/spec.md#requirement-sandbox-child-network-syscall-filter)：Linux child网络filter SHALL 拒绝八socket和三io_uring入口，先拒绝未知audit arch和x32，其他syscall允许。
- [Sandbox namespace syscall filter](../specs/sandbox-boundary/spec.md#requirement-sandbox-namespace-syscall-filter)：Linux namespace filter SHALL TSYNC阻断unshare/setns及含NEW标记clone，clone3返回ENOSYS，普通clone允许。
- [Sandbox exact website policy model](../specs/sandbox-boundary/spec.md#requirement-sandbox-exact-website-policy-model)：网站策略模型 SHALL 规范化HTTP(S) DNS origin与非零有效端口，精确deny优先allow再default。
- [Sandbox versioned website policy snapshot](../specs/sandbox-boundary/spec.md#requirement-sandbox-versioned-website-policy-snapshot)：网络策略快照 SHALL 用版本1的紧凑serde JSON及排序集合生成SHA256，并先检查版本再解析policy。
- [Sandbox enforcement verification scope](../specs/sandbox-boundary/spec.md#requirement-sandbox-enforcement-verification-scope)：sandbox验证 SHALL 区分真实子进程e2e、构造测试与纯打印smoke；REQUIRE开关使后端不可用失败。

## 边界

- 返回Ok且不设置applied；依赖存在不证明当前平台可实施。
- profile_name为空，metrics和log_violation仍可使用已安装logger；请求profile不代表已隔离。
- 调用方仍需检查is_applied；Off和stub亦可Ok，不推导内核已生效。
- 该文件视为无配置，读取错误静默、解析错误警告；无冲突报告不证明加载成功。
- ReadOnly并非完全禁止写入；Strict显式系统读取范围和设备表见审阅记录，网络配置由子进程启动点实施。
- 追加read不会撤销已有write；继承Custom或Off失败，Devbox继承不启用Hook写禁。
- 前者警告跳过，设备NotFound/ENXIO/ENODEV跳过；部分其他授权错误传播，不保证所有配置路径都已授权。
- 加入列表，不在此处验证可信根或canonical去重；macOS包含整个/private/var/folders。
- 已drain事件不回队；poison时指标可增但事件丢弃，没有fsync/轮转或持久成功保证。
- macOS原先literal不会自动覆盖子树；effective集合不是filesystem canonical化。
- 依赖固定nono与真实e2e核对，不能仅以规则构造成功证明优先级；Linux该入口不发deny，依赖bwrap。
- 没有额外祖先规则；匹配root使用路径前缀，详细alias及边界见审阅记录。
- 分流为literal而非glob；macOS转换锚定regex，有限parity测试不是全输入内核等价证明。
- 拒绝计划；PermissionDenied跳过，后建匹配不覆盖，条目上限不是wall-clock deadline。
- 返回None，调用方不能把None当隔离证明；optional不存在write路径跳过，read占位失败拒绝。
- 复用并chmod；没有随机独占或完整no-follow保证，root读取可能空而非权限错误。
- 前者不因结果空而抹去原始read需求；后者豁免Hook写禁，不能只看基础predicate。
- 拒绝；同inode原地内容变化不由身份快照检测，snapshot不是内容hash。
- 祖先只重查非symlink目录，不保存原inode；Command执行前仍有时间窗口，不是FD绑定保证。
- Linux失败Result被OnceLock缓存；非Linux返回Ok stub，不能当Seatbelt检查。
- 此filter不阻断这些read/write，非Linux入口Ok no-op；需调用方在pre-exec安装。
- 返回错误但此前NO_NEW_PRIVS不回滚；filter自身不是完整mount/capability隔离。
- 不触发DNS或运行时实施；拒绝IP/userinfo/path/wildcard，子域不自动继承规则。
- 解析不要求原文本canonical；hash比较忽略ASCII大小写，不是签名或来源认证，也未持久到session。
- 支持记录7个实际内核场景，不证明Linuxseccomp/bwrap执行；smoke无预期失败退出码。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
