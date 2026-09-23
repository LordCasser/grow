## ADDED Requirements

### Requirement: Release discovery and channel filtering
版本发现 SHALL 查询固定GitHub仓库单页100个release，排除draft，按Rust semver::Version排序取最大合法版本。

#### Scenario: Release discovery and channel filtering boundary
- **WHEN** channel不是精确alpha
- **THEN** 排除prerelease标记为true的release；该层不拒绝未知channel，不翻页，10秒HTTP失败传播。

证据：`crates/codegen/update/src/version.rs` — `select_gh_release_version`。

### Requirement: Release version cache freshness
版本缓存 SHALL 使用version、可选stable_version和RFC3339 checked_at，只有非未来且年龄严格小于30分钟才新鲜。

#### Scenario: Release version cache freshness boundary
- **WHEN** 缓存缺失、JSON或时间非法、到达TTL
- **THEN** 返回false；新鲜度不检查版本与channel对应关系。

证据：`crates/codegen/update/src/version.rs` — `is_version_cache_fresh`。

### Requirement: Release cache publication and query effects
write_version_cache SHALL 创建目录后写固定json.tmp并rename，错误仅警告；get_latest_version查询成功后写缓存。

#### Scenario: Release cache publication and query effects boundary
- **WHEN** 查询完成但没有安装
- **THEN** 仍可写入新鲜缓存；fetch_latest_version不写，写入没有并发锁或fsync保证。

证据：`crates/codegen/update/src/version.rs` — `write_version_cache`。

### Requirement: Release stable pointer and channel display
stable指针 SHALL 尽力在3秒内另行查询；标签以编译版本与缓存stable比较，领先为alpha否则stable。

#### Scenario: Release stable pointer and channel display boundary
- **WHEN** 首次显示时缓存缺失或不可解析
- **THEN** 返回空标签或None并由各自独立OnceLock永久缓存；不按配置channel显示，也不检查缓存TTL。

证据：`crates/codegen/update/src/version.rs` — `channel_label`。

### Requirement: Managed version filename parsing
managed版本探测 SHALL 从Unix应用symlink目标文件名解析完整semver及十种平台后缀，先确认目标存在。

#### Scenario: Managed version filename parsing boundary
- **WHEN** dangling link、普通文件或非Unix
- **THEN** 返回None；文件名有效不证明文件内容或可执行性，grow-pager使用独立prefix。

证据：`crates/codegen/update/src/version.rs` — `installed_on_disk_version`。

### Requirement: Required startup version range
启动政策 SHALL 仅以required上下限包含端点判定，越界提示并exit1；矛盾范围或运行版本不可解析时放行。

#### Scenario: Required startup version range boundary
- **WHEN** 启用distro-pm
- **THEN** 跳过启动范围检查；恢复子命令是否先返回由调用方保证。

证据：`crates/codegen/update/src/version_policy.rs` — `enforce_version_policy_or_exit`。

### Requirement: Pinned install hard floor
显式pin SHALL 仅受有效required_minimum限制，不以软minimum或required_maximum拒绝。

#### Scenario: Pinned install hard floor boundary
- **WHEN** 有硬floor且target不可解析或低于floor
- **THEN** 返回TargetBelowFloor；没有有效floor时本层不验证target格式。

证据：`crates/codegen/update/src/version_policy.rs` — `check_install_target`。

### Requirement: Release policy update plan
更新计划 SHALL 使用配置resolve_target的夹取再skip结果，解析后target高于latest时标Unavailable。

#### Scenario: Release policy update plan boundary
- **WHEN** 政策Skip或Unavailable
- **THEN** 自动入口跳过，显式更新分别缓存并返回None或报错；夹取目标不代表asset存在。

证据：`crates/codegen/update/src/auto_update.rs` — `plan_for`。

### Requirement: Installer availability gate
get_installer SHALL 在普通构建返回gh-release，distro-pm返回None。

#### Scenario: Installer availability gate boundary
- **WHEN** 构建来自其他安装途径
- **THEN** 普通构建仍返回gh-release，并不探测安装来源；公有底层安装函数本身没有此gate。

证据：`crates/codegen/update/src/auto_update.rs` — `get_installer`。

### Requirement: Release comparison and rollback direction
needs_update SHALL 接受stable、alpha、enterprise精确名称，以Rust Version比较；stable和enterprise拒绝预发行target。

#### Scenario: Release comparison and rollback direction boundary
- **WHEN** current为预发行而target为正式版
- **THEN** 直接要求安装，即使目标更低；其他情况allow_downgrade为true时不等即更新，构建metadata也影响比较。

证据：`crates/codegen/update/src/auto_update.rs` — `needs_update`。

### Requirement: Update status output and check semantics
UpdateStatus SHALL 以camelCase单行JSON含null字段输出，文本错误优先；check使用运行版本及禁止普通回退的比较参数。

#### Scenario: Update status output and check semantics boundary
- **WHEN** 后台允许回退但check查询较低正式版
- **THEN** check可报告无更新，不能声称两入口决策完全一致；check查询写缓存。

证据：`crates/codegen/update/src/auto_update.rs` — `check_update_status`。

### Requirement: Install target resolution and classification
安装目标 SHALL PATH优先，其次override或current_exe；canonical等于managed为Managed，Unix其他symlink为Symlink，其余RegularFile。

#### Scenario: Install target resolution and classification boundary
- **WHEN** PATH候选含相对目录或非可执行文件
- **THEN** 相对目录沿用cwd；Unix检查任一execute mode bit并跳目录，非真实access检查；override不检验存在。

证据：`crates/codegen/update/src/auto_update.rs` — `resolve_install_target`。

### Requirement: Disk probe cache and replaceability
目标probe SHALL 优先信任canonical版本文件名，再以10秒等待执行grow --version；结果含None按路径缓存。

#### Scenario: Disk probe cache and replaceability boundary
- **WHEN** 并发probe或外部替换普通文件
- **THEN** 无single-flight或TTL保证；land成功失效部分键，verify信任当前exe或probe，不是内容认证，timeout不保证kill。

证据：`crates/codegen/update/src/auto_update.rs` — `probe_target_version`。

### Requirement: Leader disk convergence and relaunch outcome
ensure_latest_on_disk SHALL heal后获取计划，以磁盘版本优先、未知回退运行版本决定安装，再比较磁盘与运行版本决定relaunch。

#### Scenario: Leader disk convergence and relaunch outcome boundary
- **WHEN** 另一更新已在磁盘可观察地落地
- **THEN** 可跳下载仍返回relaunch_needed；缓存陈旧或探测失败限制此判断，不是跨进程互斥。

证据：`crates/codegen/update/src/auto_update.rs` — `ensure_latest_on_disk`。

### Requirement: Background update opt in and download handle
后台检查 SHALL 先heal，再检查TTL与auto_update必须Some(true)，运行版本决定提示而磁盘版本决定启动下载。

#### Scenario: Background update opt in and download handle boundary
- **WHEN** 下载子进程启动失败
- **THEN** 警告但仍返回版本提示；禁用自动更新则不做后台查询或提示，未知磁盘按需下载。

证据：`crates/codegen/update/src/auto_update.rs` — `check_update_background`。

### Requirement: Automatic update execution modes
run_update_if_available SHALL 在blocking子进程成功时返回true，其余跳过或打印失败后false。

#### Scenario: Automatic update execution modes boundary
- **WHEN** 以NonBlocking启动
- **THEN** 当前exe只带update参数，三流null且detach，返回Child可wait；不传预取target/channel，drop不主动杀子进程。

证据：`crates/codegen/update/src/auto_update.rs` — `run_update_subcommand`。

### Requirement: Post update restart resolution
restart_grow SHALL PATH优先重启，无PATH时仅当前exe位于grow_home且managed存在才改用managed。

#### Scenario: Post update restart resolution boundary
- **WHEN** 重新执行
- **THEN** 保留全部原参数和环境但删除GROW_AUTO_UPDATE；Unixexec替换，非Unixspawn后exit0。

证据：`crates/codegen/update/src/auto_update.rs` — `restart_grow`。

### Requirement: Release platform and temporary naming
安装 SHALL 按编译目标选择十种asset平台并优先OHOS；临时文件完整名称追加PID与进程序号。

#### Scenario: Release platform and temporary naming boundary
- **WHEN** 不支持的编译目标或重复下载
- **THEN** 前者报错；后者生成不同进程序号名，但不是随机独占或PID复用安全承诺。

证据：`crates/codegen/update/src/auto_update.rs` — `unique_temp_sibling`。

### Requirement: Parallel range download boundary
并行下载 SHALL HEAD取大小，按每16MiB一块上限8块，至少两块才分段；每段要求206及精确请求字节数。

#### Scenario: Parallel range download boundary boundary
- **WHEN** parallel任一步错误
- **THEN** 回退完整GET；不校验Content-Range/ETag，块在内存完整缓冲后写入，每请求20分钟而非全流程预算。

证据：`crates/codegen/update/src/auto_update.rs` — `try_parallel_download`。

### Requirement: Download size and publication differences
带进度下载 SHALL 流中限制512MiB且拒绝空体；静默下载没有同等上限且允许空体。

#### Scenario: Download size and publication differences boundary
- **WHEN** 下载成功发布
- **THEN** Unixchmod0755在rename前执行；常规错误尽力删temp，取消和崩溃不保证清理或fsync持久。

证据：`crates/codegen/update/src/auto_update.rs` — `download_silent`。

### Requirement: Release checksum manifest and archive integrity
Release安装 SHALL 获取最多128KiB非空UTF8 SHA256SUMS，要求小写64hex双空格格式及唯一目标asset，逐字节hash归档验证。

#### Scenario: Release checksum manifest and archive integrity boundary
- **WHEN** 目标缺失、重复或hash不符
- **THEN** 安装失败；同源manifest不是独立签名，其他asset重复不由目标重复检查拒绝。

证据：`crates/codegen/update/src/auto_update.rs` — `release_asset_sha256`。

### Requirement: Release archive extraction format
解压 SHALL 只接受tar迭代器交出的一个平台grow文件Regular entry，大小1到512MiB，create_new临时文件并校验实际长度后发布。

#### Scenario: Release archive extraction format boundary
- **WHEN** 额外entry、路径错误、链接或空归档
- **THEN** 失败并尽力清temp；阻塞解压任务不因等待future取消而保证终止。

证据：`crates/codegen/update/src/auto_update.rs` — `extract_release_archive`。

### Requirement: Release smoke check and completions
smoke SHALL 以--version成功退出为通过，ETXTBSY最多八次退避；安装后顺序尽力生成bash/zsh/fish completion。

#### Scenario: Release smoke check and completions boundary
- **WHEN** 脚本输出错误版本但exit0
- **THEN** smoke仍通过，不比较请求版本；completion无timeout，fish写独立HOME，其余写grow_home。

证据：`crates/codegen/update/src/auto_update.rs` — `smoke_test_binary`。

### Requirement: Binary landing strategies
land SHALL 按Managed、Unix外部symlink、Unix普通文件和Windows普通文件选择落地方式。

#### Scenario: Binary landing strategies boundary
- **WHEN** Unix普通文件替换
- **THEN** 复制唯一sibling、chmod0755、rename后失效probe；不保留原权限或保证fsync，底层公有land不校验版本。

证据：`crates/codegen/update/src/auto_update.rs` — `land_binary_on_target`。

### Requirement: Managed entrypoint rollback boundary
Managed更新 SHALL 先捕获grow与agent状态，再顺序替换，后项失败则逆序尽力恢复前项。

#### Scenario: Managed entrypoint rollback boundary boundary
- **WHEN** 恢复失败、取消或并发更新
- **THEN** 不能保证双路径全有全无；恢复失败warn，Unix现存非symlink在capture时报错，不能据注释推导事务。

证据：`crates/codegen/update/src/auto_update.rs` — `swap_managed_bin_links`。

### Requirement: Unix symlink publication and relative layout
Unix symlink SHALL 先创建唯一temp-link再rename，保留旧目标；同目录或兄弟目录生成短相对路径。

#### Scenario: Unix symlink publication and relative layout boundary
- **WHEN** 其他布局或失败临时残留
- **THEN** 返回原target路径而非强制绝对化；按mtime超过一小时尽力清理旧temp-link。

证据：`crates/codegen/update/src/auto_update.rs` — `atomic_symlink_swap`。

### Requirement: Windows executable replacement recovery
Windows SHALL 先copy，遇5或32错误rename aside再copy，失败尝试恢复；locked旧aside改用唯一名。

#### Scenario: Windows executable replacement recovery boundary
- **WHEN** 恢复rename失败或并发清理aside
- **THEN** 返回原copy错误，恢复错误被忽略；旧aside无年龄保护，不能保证原子落地与回滚源必存。

证据：`crates/codegen/update/src/auto_update.rs` — `windows_replace_exe`。

### Requirement: Managed entrypoint healing
heal SHALL 对gh-release managed目录以grow修agent，Unix要求grow是有效symlink，Windows比较长度和内容。

#### Scenario: Managed entrypoint healing boundary
- **WHEN** 当前PATH是external target
- **THEN** heal仍可维护managed目录；Unixgrow普通文件或dangling跳过，失败只记录。

证据：`crates/codegen/update/src/auto_update.rs` — `heal_managed_install`。

### Requirement: Old download retention and temporary cleanup
清理 SHALL 保留所有current相等平台文件及其他候选最高版本的一个文件，并保留年龄不超过一小时的其他版本文件。

#### Scenario: Old download retention and temporary cleanup boundary
- **WHEN** 未知或未来mtime
- **THEN** tmp保留，普通versioned不满足fresh仍尝试删；不检查运行进程或活动link，最高other不一定比current低。

证据：`crates/codegen/update/src/auto_update.rs` — `cleanup_old_downloads`。

### Requirement: Release installation pipeline ordering
Release安装 SHALL 下载manifest和archive、校验解压发布并smoke后，才resolve及verify最终target并land。

#### Scenario: Release installation pipeline ordering boundary
- **WHEN** 同版本downloads目标已存在或验证最终target失败
- **THEN** 发布可能已覆盖downloads文件，不保证全流程零副作用；target=None总查真实GitHub stable，显式target此底层不验证政策。

证据：`crates/codegen/update/src/auto_update.rs` — `install_gh_release_from`。

### Requirement: Managed release aliases and post install cleanup
Managed落地后 SHALL 尽力更新grow-latest、匹配字符串的legacy系统link并删旧pager；全部安装清旧downloads并生成completion。

#### Scenario: Managed release aliases and post install cleanup boundary
- **WHEN** 成功经过run_install_script
- **THEN** 尽力删除models_cache；external落地跳managed系统alias维护，legacy判断并非当前home所有权校验。

证据：`crates/codegen/update/src/auto_update.rs` — `run_install_script`。

### Requirement: Explicit channel switch and pin effects
run_update SHALL 先应用channel switch，再installer gate；pin直接安装，成功尽力持久化auto_update=false。

#### Scenario: Explicit channel switch and pin effects boundary
- **WHEN** channel持久化失败或pin成功
- **THEN** switch仍改内存并提示；pin不写版本cache且不做磁盘去重，失败策略见硬floor要求。

证据：`crates/codegen/update/src/auto_update.rs` — `apply_channel_switch`。

### Requirement: Explicit force and installed outcome semantics
非pin更新 SHALL 以磁盘版本优先判定，force仍遵守计划Skip或Unavailable，但跳过正常channel错误分支。

#### Scenario: Explicit force and installed outcome semantics boundary
- **WHEN** needs_update为false且未显式切换
- **THEN** 写缓存并返回Some(target)，预发行target被拒时此返回不证明已安装；force可能重装effective_current。

证据：`crates/codegen/update/src/auto_update.rs` — `run_update`。

### Requirement: Updater verification evidence boundary
update验证 SHALL 区分helper测试、模拟release安装和真实完整更新入口，不将替代模型或前后断言扩大为运行时保证。

#### Scenario: Updater verification evidence boundary boundary
- **WHEN** cache或second-pass测试通过
- **THEN** cache helper重写逻辑、second-pass只probe；range可回退完整GET，Windows分支需对应平台执行。

证据：`crates/codegen/update/tests/test_io.rs` — `cache_is_fresh`。


### Requirement: Shell cached release channel identity

channel_name_from_cache SHALL 首次读取grow_home/version.json的stable_version并与编译VERSION按semver比较，当前更高alpha，否则stable；读错/JSON错/字段缺失或版本解析错None。OnceLock缓存整个Option，包括失败None，后续文件更新不重读；不使用cli.channel或版本字符串预发布名直接判渠道。

#### Scenario: Cache appears after first failed read
- **WHEN** 首次缺文件返回None，随后写入有效version.json
- **THEN** 同进程后续调用仍None。

证据：`crates/codegen/shell/src/util/config/resolve/version.rs` — `pub fn channel_name_from_cache`。

### Requirement: Shell version bounds tightening and invalid ranges

VersionPolicy SHALL 仅从ConfigLayers.user cli四个版本字符串及各自GROW环境收集候选，trim空值忽略、坏semver warning忽略；minimum/required_minimum取最大，maximum/required_maximum取最小，环境只能收紧，加载层失败仅用环境。硬范围倒置保留原字段并warning，但effective_required上下界均失效；软范围倒置warning并由目标解析处理，不修改原值。

#### Scenario: Loosening environment floor
- **WHEN** 本地最低版本高于环境最低版本
- **THEN** 保持本地更高下限。

证据：`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn version_candidates`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn fold_bound`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn resolve_version_bound`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `pub fn has_contradictory_required_range`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn env_tightens_but_cannot_loosen`。

### Requirement: Shell version target clamp ordering

resolve_target SHALL 先将有效semver目标按软maximum、有效硬maximum压低，再以有效硬minimum抬高，最后仅在目标可解析且低于软minimum时返回None。软minimum不抬高目标，也不比较当前安装版本；坏目标在有有效硬minimum时从0.0.0走同clamp，否则原样通过。installable_floor仅有有效硬minimum时返回同clamp结果；硬下限可胜过较低软上限。这里不下载或执行启动阻断。

#### Scenario: Invalid target without hard floor
- **WHEN** 目标dev且只设置软minimum
- **THEN** 返回Some(dev)，无法解析不触发软下限skip。

证据：`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn clamp_version`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `pub fn resolve_target`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn skips_update_target`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `pub fn installable_floor`；`crates/codegen/shell/src/util/config/resolve/version.rs` — `fn resolve_target_clamps_then_skips`。
