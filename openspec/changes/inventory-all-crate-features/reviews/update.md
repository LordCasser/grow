# update 逐包审阅记录

状态：pending。已完整阅读 Cargo.toml、src/lib.rs、src/version.rs（537 行）、src/version_policy.rs（221 行）；auto_update.rs 已读 1–500 行，其余及 integration tests 尚未读。暂不纳入 reviewed 或完整能力映射。未运行本包测试。

## 版本发现、缓存与显示

- UpdateConfig 仅 channel String，默认 stable，无类型枚举约束。GitHub Releases 固定 LordCasser/grow，单页 per_page=100，没有翻页；每次建立 reqwest client，10秒 timeout、grow-updater UA、GitHub Accept，HTTP错误/JSON错误传播。排除 draft；只有精确 alpha 接纳 prerelease=true，其他 channel 都按稳定过滤。只去掉一个小写 v 前缀，忽略非法 semver，再取 semver 最大值；稳定筛选依赖 release.prerelease 标志，而非 tag 自身预发行字段。
- fetch_latest_version 不写缓存；get_latest_version 在主查询成功后另查 stable 指针（外层3秒 best effort），然后写缓存，即使只是查询也写入。version.json 必有 version/checked_at，可缺 stable_version，未知字段接受。新鲜度仅检查 RFC3339 时间：未来时间拒绝，年龄严格小于30分钟；不校验缓存版本或 channel，TTL 边界过期。
- write_version_cache 创建目录，固定 version.json.tmp 写入后 rename；所有错误仅 warn，无返回错误，不加锁、不fsync、不随机临时名。并发写是否冲突需后续审计，当前不能宣称并发事务安全。
- installed_on_disk_version 仅 Unix：读取 managed application symlink、metadata跟随验证存在，从 link target 文件名解析版本，不执行二进制；不验证是普通可执行文件、内容或目标确在 downloads 下。非Unix None。识别10种完整平台后缀，可去.exe，bin前缀区分grow与grow-pager，semver验证保留原版本字符串及预发行。
- cached_stable_version 同步读缓存，不检查TTL；derive_channel仅比较 compiled VERSION 与 stable，前者大则alpha，否则stable，解析失败None。channel_name和channel_label各自独立OnceLock，首次读缓存后永久固定；两者初始化时间不同时不保证相同快照。标签并非配置channel或版本名是否含alpha。
- version单元测试覆盖未来时钟、TTL边界、命名平台/预发行矩阵、channel比较、release选择、JSON字段和默认配置；尚未执行，不能据此声称网络或原子安装已验证。

## 启动与安装版本政策

- 启动仅 required_minimum/required_maximum 硬范围，包含端点；矛盾范围或运行版本无法parse时fail open。distro-pm编译分支直接返回；其他分支resolve政策、越界stderr提示并exit1。恢复子命令先返回是调用方约定，尚未查看调用方，不能仅凭注释当作本函数保证。
- check_install_target只约束有效硬下限，低于下限或不可解析target均失败；无硬下限时非法target此层允许；显式pin高于硬上限此层允许。矛盾required范围忽略硬约束。对应三项单元测试未执行。
- 补查shell/src/util/config/resolve/version.rs 145–265行：clamp先软maximum再有效required_maximum，最后required_minimum优先；再以软minimum作skip而非抬升。无法parse目标且有硬floor时从0.0.0 clamp，否则原样传递。配置层解析尚属shell未完整审阅范围。

## auto_update 已读入口（1–500行）

- UpdateStatus camelCase JSON，不省略None（输出null）；文本error优先，其次available/latest，状态打印返回Ok。check先get_installer，再读取当前运行版本和auto_update配置；无installer不发查询且无错误。查询get_latest_version会写缓存；策略Skip/Unavailable均返回update_available=false且error=None，Install再needs_update，None区分semver错误与unsupported channel。
- plan_for用政策resolve_target：None=>Skip，target和latest都可解析且target>latest=>Unavailable，否则Install；并未证明夹取目标的release asset存在。
- auto_update_target无缓存写入，解析/查询失败折为None，使用运行版本决定更新。ensure_latest_on_disk先heal，再取policy plan；探测目标磁盘版本，失败回退运行版本；需要时安装，再比较运行版本与重新探测磁盘版本（或本次installed）决定relaunch。后续probe/heal/install/needs_update实现未读，暂不确认其更深保证。
- PATH候选先bare grow，Windows按PATHEXT顺序追加，默认.COM/.EXE/.BAT/.CMD，空suffix过滤；PATH目录外层优先，metadata失败/目录跳过，Unix仅检查任一执行mode bit，不是真正access(X_OK)，也不限定regular file。相对/空PATH入口沿用cwd，不额外做Windows cwd或App Paths查找。
- classify_target以canonical路径等于managed判Managed（Windows ASCII不分大小写）；Unix否则symlink标Symlink；其他RegularFile。resolve_install_target始终PATH优先，其后exe_override或current_exe；override未检验存在，与InstallTarget注释“guaranteed to exist”并不一致。此处只记事实，不修运行时代码。

## 待续

从 auto_update.rs 501 行继续；之后 tests/test_io.rs、tests/test_network.rs、tests/common/mod.rs。完整阅读后再制定测试隔离方式，避免真实自更新或改动用户安装。

## auto_update 生产逻辑完成（续读501–2880行）

现已读完auto_update全部生产逻辑；测试模块刚开始，已读PATHEXT两项测试，下一段从2881行继续。以下是实际实现边界，覆盖或修正前面的暂定注释解释。

### 探测、后台与重启

- PROBE_CACHE以canonical路径（失败用原路径）缓存Option<String>，包括失败None，无TTL/mtime/inode检测；mutex只包查表与写表，异步probe在锁外，不是single-flight，并发可能重复exec。外部替换普通文件不会自动失效；land成功后删literal及当前canonical键，旧symlink目标键可能保留。缓存mutex poison unwrap会panic。
- probe先凭canonical文件名接受版本，不校验内容；否则执行 --version，要求成功退出、UTF8、精确grow空格前缀和首token semver。10秒timeout限制等待，stdout无大小上限；没有本地kill_on_drop设置，detach是否清理需结合tty-utils。verify_target_replaceable信任当前exe canonical相同路径，否则信任上述probe；不是签名/文件内容鉴权。
- get_installer正常永远Some gh-release，distro-pm None；不是安装来源探测。installer_allows_downgrade对任意字符串返回true，和auto_update_target旧注释“GitHub never downgraded”相反。needs_update先parse；stable/enterprise拒绝预发行target，但current为预发行且target正式版时直接true，即使目标更低且allow_downgrade=false。alpha普通比较；允许回退则任何不等，否则仅target>current。check_update_status固定false而后台true，故正常版本回退提示与执行并不完全一致。
- background及run_update_if_available先heal，再TTL，再auto_update必须Some(true)；关闭自动更新也不显示后台版本提示。查询失败/政策skip或unavailable均无更新；判断不需更新时写缓存。background以运行版本判提示，磁盘判是否启动下载；未知磁盘=>下载，启动失败仍可返回update提示。实际child仅current_exe update，不传本次channel/target参数，由子进程重新解析。
- run_update_if_available返回true仅blocking子进程成功，子进程失败打印后Ok(false)；interactive只控制提示分支，不向用户询问。Blocking stdin/stdout null、stderr inherit，不detach；NonBlocking三流null、detach并返回Child，drop不主动kill。后台调用没有总超时或跨进程锁。
- restart目标PATH优先；PATH无结果时current canonical位于grow_home且managed存在则选managed，否则current。managed只exists检测。参数完整保留OsString，env复制但删除GROW_AUTO_UPDATE；Unix flush后exec，非Unixspawn后exit0。

### 下载与发布

- 编译目标映射10个平台，OHOS先于Linux，gnu/musl明确，不支持目标报错；不依据运行时系统探测。
- 临时名完整base追加PID与AtomicU64序号；非随机、不保证PID复用后绝无碰撞，下载File::create可覆盖预存同名。publish先Unixchmod0755再rename；Windowsrename失败后走windows_replace_exe。未fsync。
- 每个HTTP请求20分钟，HEAD/各range/单连接回退分别计时，无整个download总预算。parallel尝试HEAD成功、Content-Length、至少16MiB，但chunk=sizeMiB/16取1..8，因此实际上至少32MiB才并行。预分配稀疏文件，各range先完整缓冲Vec再blocking seek/write；206和精确长度校验，不检查Content-Range或ETag/If-Range，无法单凭此函数证明各块同一对象。try_join_all错误会丢弃其他future，但已启动blocking write不因此取消。
- download_with_progress两路径均受512MiB上限，单连接拒绝已知0长度及最终空体、流中超限；不显式比较实际长度与Content-Length（传输层可能报错）。parallel任何错误都回退重发GET，包括已知超限；单连接再次独立校验。静默download_silent传None上限且单连接无空体/流大小限制，不能把有进度路径的限制概括到所有下载。
- 常规错误会尝试删除temp；取消/进程退出不保证清理。chmod0755也用于下载archive发布临时路径，不只可执行binary。
- smoke只执行--version看exit成功，不验证输出或与请求版本相等；10秒每次，ETXTBSY最多8次、25ms乘attempt退避，其余失败false。probe不具此retry。completions三shell顺序执行且无timeout，stdout完整缓冲；bash/zsh写grow_home，fish写独立HOME/.config/fish，创建和写失败忽略；HOME缺失则相对路径。测试隔离必须同时考虑HOME和GROW_HOME。

### 安装落地、恢复与清理

- Unix external symlink原子换link保留旧目标；regular复制到唯一sibling、chmod0755、rename，无fsync和权限/owner保留；失败临时文件留给后续sweep。Managed始终操作grow_home/bin，并不一定替换传入alias路径。land公有接口自身不做verify或smoke。
- Managed先捕获grow/agent两个状态，再顺序替换；第二个失败逆序尽力恢复已完成项，恢复失败warn。不是双路径原子事务，取消/崩溃无RAII回滚、无锁，不能把“All-or-nothing”注释当绝对保证。Unix capture已有普通文件read_link失败，变更前退出；Windows独立rollback.bak。成功后清备份；失败恢复备份失败时保留已完成项备份。
- relative target只处理同目录或同祖父的兄弟目录，其余原样返回传入target（不自动绝对化）。symlink temp+rename之前sweep过期tmp-link；临时复制/链接以mtime严格>1小时清理，未知/未来mtime不删；无PID活性确认。
- Windows先直接copy（非原子），仅error5/32再rename aside；aside固定.old不可用则唯一名，rename因5/32最多3次；copy失败尝试rename恢复但忽略恢复错误。旧.exe备份无年龄门槛尽力清理，不能保证并发回滚源存活。
- cleanup_old_downloads只prefix匹配，含.tmp优先处理过期（包括非版本样式），其后跳symlink、要求digit开头及已识别平台semver。保留全部current相等文件，以及其他候选排序最高的一个文件（不要求低于current，也不按版本去重）；其余fresh<=1小时保留。普通versioned未来/未知mtime不满足fresh，仍尝试删除，与tmp未知时间保留相反；不检查运行中进程或active symlink目标。
- heal正常gh-release每次对managed路径尽力修agent，不受当前实际PATH target是否external约束。Unix grow必须是可跟随symlink，agent raw target相等则跳，否则替换，可覆盖普通agent文件；Windows比较长度后64KiB块内容，差异时replace，读失败不修。

### Release校验及完整安装入口

- SHA256SUMS最多128KiB，流中限额，非空UTF8；每个非空行必须64位小写hex、两个空格分隔、非空无斜杠name。只拒绝目标asset重复，其他asset重复不拒；不验证name额外空白。SHA256归档完整流hash与manifest精确比较，manifest来自同release HTTPS，不是独立签名。
- 解压blocking task接受tar迭代器交出的恰好一个Regular entry、path等于平台grow/grow.exe、大小1..512MiB；create_new临时文件，copy后实际长度一致，flush后publish。拒绝额外可见entry、路径/链接/空归档；是否gzip尾部/扩展元数据完整验证由tar/flate2决定，不能夸大为逐字节容器验证。blocking任务取消不保证终止或清temp。
- install_gh_release_from若target=None总是查真实GitHub stable，忽略自定义base的版本发现；有target直接字符串拼路径/URL，此公有入口不验证semver或政策。先创建downloads/bin、下载manifest/archive、hash、解压publish、smoke，之后才resolve/verify实际安装目标。相同版本的既有binary_path可能在最终验证前已被覆盖，smoke失败删除该路径；“目标未动”只适用于最终landing步骤，不能扩大成全流程保证。
- Managed landing后尽力更新grow-latest；/usr/local/bin grow/agent旧target仅字符串contains .grow/downloads/且非ends grow-latest就尝试改，不校验属于当前用户grow_home；external跳这些维护。随后清grow和grow-pager旧downloads，生成completion；成功run_install_script再尽力删models_cache。测试需要避开Managed全流程真实系统链接副作用。

### 显式更新命令

- apply_channel_switch先尝试持久化忽略错误，仍修改内存并打印成功，无channel值验证；run_update在installer gate之前执行它。
- pin仅check硬floor后直接安装，无latest查询/disk-dedup，成功尽力持久化auto_update=false，不写version cache；因此强制同版本pin也重装。
- 非pin政策Skip写latest缓存返回None，Unavailable报错不缓存。探磁盘失败回退运行版本；非force需要更新继续、不需要但显式switch且目标不同也继续，否则写cache并返回Some(target)。当needs_update因预发行target拒绝时，这个Some不证明该target真已安装，函数文档保证存在边界。
- force不越过Skip/Unavailable，但跳过正常parse/channel错误；若needs_update为Some(false)重装effective_current，否则安装policy target（None按true）。成功写实际选择target的缓存；非force且没有GROW_AUTO_UPDATE才打印restart提示，不在本函数自动重启。

待完成auto_update余下单元测试及全部integration，之后才标reviewed和运行受隔离测试。以上债务仅记录，不改运行时代码。

## 全包测试源码阅读完成

已补读auto_update2881–5338行、tests/test_network447行、test_io894行、common67行。Cargo与所有Rust文件共完整阅读，尚待动态测试终态和功能映射。

- probe memo测试是顺序调用两次再invalidate，不验证并发dedup或外部替换失效。filename优先测试明确用不可执行任意内容文件，说明版本名即被接受。
- needs_update矩阵覆盖大小写/空channel、非法semver、预发行、enterprise、rollback；构建metadata +abc/+xyz也影响semver::Version的Ord与相等，规格必须写实际Rust类型排序，而非笼统“遵守SemVer precedence”。
- symlink原子性测试只断言前后exists，无并发读观察；多项残留检查仍用with_extension(tmp/tmp-link)旧命名，无法证明新PID序号临时文件全部清理。相对链接迁移测试使用cp -a临时树，不是实际Docker挂载。
- cleanup测试覆盖prefix碰撞、混合alpha/stable、多平台current、mtime新旧、无效输入；未覆盖future/不可读mtime或同一旧版本多平台tie。Windows测试有share_mode模拟锁、aside恢复、旧备份清理；macOS执行不覆盖这些cfg分支。
- network模拟HTTP测试验证release过滤、二进制字节、空体（silent允许）、4xx/5xx、512MiB已知length提前拒绝。32MiB range测试具206 responder，但无请求计数/Range断言，完整GET回退也能通过最终bytes断言；不能单凭该测试成功证明并行路径实际执行。5MiB完整bytes断言也不证明内存使用上界。
- test_io的cache_is_fresh重写了生产逻辑，未调用公开is_version_cache_fresh，并遗漏future guard和version必填形状；其3项fresh/stale/missing测试只验证测试内模型，不能算生产freshness集成验证。时间戳测试容忍前后5秒；原子写测试顺序覆盖而非并发。
- full install fixture是shell脚本，--version及completions都只echo版本；PATH/HOME由RAII恢复，GROW_HOME由每测试binary的OnceLock临时目录keep，reset删已知文件/目录。真实系统/usr/local/bin仍可能被Managed流程触及；本次运行前确认grow/agent均非symlink，该条件不会进入写入分支。
- full pipeline覆盖plain/Managed/external symlink、checksum错误不落地、non-grow拒绝；dangling只测land层。second_pass_skips_download实际只安装一次再probe，没有执行第二次更新决策，因此不能证明端到端下载去重。running executable测试是解释器脚本sleep30，仅证明其存活与rename结果，未覆盖原生Mach-O代码页。
- 未发现本包测试实际执行run_update/ensure_latest_on_disk的完整政策与后台子进程链；不把helper矩阵扩大为这些入口已集成验证。
- 补查tty-utils::detach_command仅设置session/console行为，不设置kill_on_drop；probe/smoke timeout不保证子进程被杀死。

测试已启动：cargo test --locked -p update -- --test-threads=1 --nocapture，会话27616，日志/tmp/grow-update-inventory-tests.log；启动时尚无终态，不记通过。

## 映射完成（前文pending为阶段历史）

全部源码已读，测试终态另记。

## 功能与规范映射

- [Release discovery and channel filtering](../specs/release-update/spec.md#requirement-release-discovery-and-channel-filtering)：版本发现 SHALL 查询固定GitHub仓库单页100个release，排除draft，按Rust semver::Version排序取最大合法版本。
- [Release version cache freshness](../specs/release-update/spec.md#requirement-release-version-cache-freshness)：版本缓存 SHALL 使用version、可选stable_version和RFC3339 checked_at，只有非未来且年龄严格小于30分钟才新鲜。
- [Release cache publication and query effects](../specs/release-update/spec.md#requirement-release-cache-publication-and-query-effects)：write_version_cache SHALL 创建目录后写固定json.tmp并rename，错误仅警告；get_latest_version查询成功后写缓存。
- [Release stable pointer and channel display](../specs/release-update/spec.md#requirement-release-stable-pointer-and-channel-display)：stable指针 SHALL 尽力在3秒内另行查询；标签以编译版本与缓存stable比较，领先为alpha否则stable。
- [Managed version filename parsing](../specs/release-update/spec.md#requirement-managed-version-filename-parsing)：managed版本探测 SHALL 从Unix应用symlink目标文件名解析完整semver及十种平台后缀，先确认目标存在。
- [Required startup version range](../specs/release-update/spec.md#requirement-required-startup-version-range)：启动政策 SHALL 仅以required上下限包含端点判定，越界提示并exit1；矛盾范围或运行版本不可解析时放行。
- [Pinned install hard floor](../specs/release-update/spec.md#requirement-pinned-install-hard-floor)：显式pin SHALL 仅受有效required_minimum限制，不以软minimum或required_maximum拒绝。
- [Release policy update plan](../specs/release-update/spec.md#requirement-release-policy-update-plan)：更新计划 SHALL 使用配置resolve_target的夹取再skip结果，解析后target高于latest时标Unavailable。
- [Installer availability gate](../specs/release-update/spec.md#requirement-installer-availability-gate)：get_installer SHALL 在普通构建返回gh-release，distro-pm返回None。
- [Release comparison and rollback direction](../specs/release-update/spec.md#requirement-release-comparison-and-rollback-direction)：needs_update SHALL 接受stable、alpha、enterprise精确名称，以Rust Version比较；stable和enterprise拒绝预发行target。
- [Update status output and check semantics](../specs/release-update/spec.md#requirement-update-status-output-and-check-semantics)：UpdateStatus SHALL 以camelCase单行JSON含null字段输出，文本错误优先；check使用运行版本及禁止普通回退的比较参数。
- [Install target resolution and classification](../specs/release-update/spec.md#requirement-install-target-resolution-and-classification)：安装目标 SHALL PATH优先，其次override或current_exe；canonical等于managed为Managed，Unix其他symlink为Symlink，其余RegularFile。
- [Disk probe cache and replaceability](../specs/release-update/spec.md#requirement-disk-probe-cache-and-replaceability)：目标probe SHALL 优先信任canonical版本文件名，再以10秒等待执行grow --version；结果含None按路径缓存。
- [Leader disk convergence and relaunch outcome](../specs/release-update/spec.md#requirement-leader-disk-convergence-and-relaunch-outcome)：ensure_latest_on_disk SHALL heal后获取计划，以磁盘版本优先、未知回退运行版本决定安装，再比较磁盘与运行版本决定relaunch。
- [Background update opt in and download handle](../specs/release-update/spec.md#requirement-background-update-opt-in-and-download-handle)：后台检查 SHALL 先heal，再检查TTL与auto_update必须Some(true)，运行版本决定提示而磁盘版本决定启动下载。
- [Automatic update execution modes](../specs/release-update/spec.md#requirement-automatic-update-execution-modes)：run_update_if_available SHALL 在blocking子进程成功时返回true，其余跳过或打印失败后false。
- [Post update restart resolution](../specs/release-update/spec.md#requirement-post-update-restart-resolution)：restart_grow SHALL PATH优先重启，无PATH时仅当前exe位于grow_home且managed存在才改用managed。
- [Release platform and temporary naming](../specs/release-update/spec.md#requirement-release-platform-and-temporary-naming)：安装 SHALL 按编译目标选择十种asset平台并优先OHOS；临时文件完整名称追加PID与进程序号。
- [Parallel range download boundary](../specs/release-update/spec.md#requirement-parallel-range-download-boundary)：并行下载 SHALL HEAD取大小，按每16MiB一块上限8块，至少两块才分段；每段要求206及精确请求字节数。
- [Download size and publication differences](../specs/release-update/spec.md#requirement-download-size-and-publication-differences)：带进度下载 SHALL 流中限制512MiB且拒绝空体；静默下载没有同等上限且允许空体。
- [Release checksum manifest and archive integrity](../specs/release-update/spec.md#requirement-release-checksum-manifest-and-archive-integrity)：Release安装 SHALL 获取最多128KiB非空UTF8 SHA256SUMS，要求小写64hex双空格格式及唯一目标asset，逐字节hash归档验证。
- [Release archive extraction format](../specs/release-update/spec.md#requirement-release-archive-extraction-format)：解压 SHALL 只接受tar迭代器交出的一个平台grow文件Regular entry，大小1到512MiB，create_new临时文件并校验实际长度后发布。
- [Release smoke check and completions](../specs/release-update/spec.md#requirement-release-smoke-check-and-completions)：smoke SHALL 以--version成功退出为通过，ETXTBSY最多八次退避；安装后顺序尽力生成bash/zsh/fish completion。
- [Binary landing strategies](../specs/release-update/spec.md#requirement-binary-landing-strategies)：land SHALL 按Managed、Unix外部symlink、Unix普通文件和Windows普通文件选择落地方式。
- [Managed entrypoint rollback boundary](../specs/release-update/spec.md#requirement-managed-entrypoint-rollback-boundary)：Managed更新 SHALL 先捕获grow与agent状态，再顺序替换，后项失败则逆序尽力恢复前项。
- [Unix symlink publication and relative layout](../specs/release-update/spec.md#requirement-unix-symlink-publication-and-relative-layout)：Unix symlink SHALL 先创建唯一temp-link再rename，保留旧目标；同目录或兄弟目录生成短相对路径。
- [Windows executable replacement recovery](../specs/release-update/spec.md#requirement-windows-executable-replacement-recovery)：Windows SHALL 先copy，遇5或32错误rename aside再copy，失败尝试恢复；locked旧aside改用唯一名。
- [Managed entrypoint healing](../specs/release-update/spec.md#requirement-managed-entrypoint-healing)：heal SHALL 对gh-release managed目录以grow修agent，Unix要求grow是有效symlink，Windows比较长度和内容。
- [Old download retention and temporary cleanup](../specs/release-update/spec.md#requirement-old-download-retention-and-temporary-cleanup)：清理 SHALL 保留所有current相等平台文件及其他候选最高版本的一个文件，并保留年龄不超过一小时的其他版本文件。
- [Release installation pipeline ordering](../specs/release-update/spec.md#requirement-release-installation-pipeline-ordering)：Release安装 SHALL 下载manifest和archive、校验解压发布并smoke后，才resolve及verify最终target并land。
- [Managed release aliases and post install cleanup](../specs/release-update/spec.md#requirement-managed-release-aliases-and-post-install-cleanup)：Managed落地后 SHALL 尽力更新grow-latest、匹配字符串的legacy系统link并删旧pager；全部安装清旧downloads并生成completion。
- [Explicit channel switch and pin effects](../specs/release-update/spec.md#requirement-explicit-channel-switch-and-pin-effects)：run_update SHALL 先应用channel switch，再installer gate；pin直接安装，成功尽力持久化auto_update=false。
- [Explicit force and installed outcome semantics](../specs/release-update/spec.md#requirement-explicit-force-and-installed-outcome-semantics)：非pin更新 SHALL 以磁盘版本优先判定，force仍遵守计划Skip或Unavailable，但跳过正常channel错误分支。
- [Updater verification evidence boundary](../specs/release-update/spec.md#requirement-updater-verification-evidence-boundary)：update验证 SHALL 区分helper测试、模拟release安装和真实完整更新入口，不将替代模型或前后断言扩大为运行时保证。

## 边界

- 排除prerelease标记为true的release；该层不拒绝未知channel，不翻页，10秒HTTP失败传播。
- 返回false；新鲜度不检查版本与channel对应关系。
- 仍可写入新鲜缓存；fetch_latest_version不写，写入没有并发锁或fsync保证。
- 返回空标签或None并由各自独立OnceLock永久缓存；不按配置channel显示，也不检查缓存TTL。
- 返回None；文件名有效不证明文件内容或可执行性，grow-pager使用独立prefix。
- 跳过启动范围检查；恢复子命令是否先返回由调用方保证。
- 返回TargetBelowFloor；没有有效floor时本层不验证target格式。
- 自动入口跳过，显式更新分别缓存并返回None或报错；夹取目标不代表asset存在。
- 普通构建仍返回gh-release，并不探测安装来源；公有底层安装函数本身没有此gate。
- 直接要求安装，即使目标更低；其他情况allow_downgrade为true时不等即更新，构建metadata也影响比较。
- check可报告无更新，不能声称两入口决策完全一致；check查询写缓存。
- 相对目录沿用cwd；Unix检查任一execute mode bit并跳目录，非真实access检查；override不检验存在。
- 无single-flight或TTL保证；land成功失效部分键，verify信任当前exe或probe，不是内容认证，timeout不保证kill。
- 可跳下载仍返回relaunch_needed；缓存陈旧或探测失败限制此判断，不是跨进程互斥。
- 警告但仍返回版本提示；禁用自动更新则不做后台查询或提示，未知磁盘按需下载。
- 当前exe只带update参数，三流null且detach，返回Child可wait；不传预取target/channel，drop不主动杀子进程。
- 保留全部原参数和环境但删除GROW_AUTO_UPDATE；Unixexec替换，非Unixspawn后exit0。
- 前者报错；后者生成不同进程序号名，但不是随机独占或PID复用安全承诺。
- 回退完整GET；不校验Content-Range/ETag，块在内存完整缓冲后写入，每请求20分钟而非全流程预算。
- Unixchmod0755在rename前执行；常规错误尽力删temp，取消和崩溃不保证清理或fsync持久。
- 安装失败；同源manifest不是独立签名，其他asset重复不由目标重复检查拒绝。
- 失败并尽力清temp；阻塞解压任务不因等待future取消而保证终止。
- smoke仍通过，不比较请求版本；completion无timeout，fish写独立HOME，其余写grow_home。
- 复制唯一sibling、chmod0755、rename后失效probe；不保留原权限或保证fsync，底层公有land不校验版本。
- 不能保证双路径全有全无；恢复失败warn，Unix现存非symlink在capture时报错，不能据注释推导事务。
- 返回原target路径而非强制绝对化；按mtime超过一小时尽力清理旧temp-link。
- 返回原copy错误，恢复错误被忽略；旧aside无年龄保护，不能保证原子落地与回滚源必存。
- heal仍可维护managed目录；Unixgrow普通文件或dangling跳过，失败只记录。
- tmp保留，普通versioned不满足fresh仍尝试删；不检查运行进程或活动link，最高other不一定比current低。
- 发布可能已覆盖downloads文件，不保证全流程零副作用；target=None总查真实GitHub stable，显式target此底层不验证政策。
- 尽力删除models_cache；external落地跳managed系统alias维护，legacy判断并非当前home所有权校验。
- switch仍改内存并提示；pin不写版本cache且不做磁盘去重，失败策略见硬floor要求。
- 写缓存并返回Some(target)，预发行target被拒时此返回不证明已安装；force可能重装effective_current。
- cache helper重写逻辑、second-pass只probe；range可回退完整GET，Windows分支需对应平台执行。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## update 测试终态与映射收口

会话27616退出0：macOS默认feature，cargo test --locked -p update -- --test-threads=1 --nocapture，126单元+25 IO集成+17网络集成=168通过，0失败/ignored，0 doctest。日志/tmp/grow-update-inventory-tests.log。Windows专属测试未编译运行，未执行distro-pm feature；前述替代freshness模型、未实际第二次更新和range可回退的覆盖限制仍有效。

34项release-update要求已映射，8份manifest/Rust证据哈希核对通过，累计36/61包、454项要求、30个delta能力，剩25包。OpenSpec strict15/15与git diff --check通过；全仓尚未完成，不归档inventory change。
