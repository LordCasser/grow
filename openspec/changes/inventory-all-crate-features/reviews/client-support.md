# client-support 逐包审阅（进行中）

包保持pending。已完整读Cargo.toml、lib.rs、stderr.rs 50行、ui_config.rs 433行、session/mod.rs 12行、session/info.rs 8行、examples/clipboard_probe.rs 37行；placeholder_images.rs已读1–250，余251–1438及clipboard.rs全部3352行待完成。全部Rust共5339行（包含example）。

## 已核对事实

- 公共模块clipboard、placeholder_images、session、stderr、ui_config；没有Cargo features。macOS objc2，非macOS arboard+wayland-data-control，Linux额外wl-clipboard-rs；image开启png/jpeg/gif/webp。生产clipboard平台行为尚待读，manifest注释不单独作为运行时证据。
- session::Info serde字段id（ACP SessionId）和cwd字符串；session_dir调用tools::util::grow_home::sessions_cwd_dir(cwd)后join id字符串。本包不额外校验id路径形态，不推导目录containment。
- stderr通过进程OnceLock<parking_lot::Mutex>串行访问，优先tty_utils::dup_tui_stderr；fallback Unix libc::dup然后from_raw_fd，没有检查dup返回-1，需独立核对错误边界。非Unix分支使用Windows handle clone，失败expect panic。不能由锁推断所有直接stderr输出也受保护。
- UiConfig serde(default)，无deny_unknown_fields；max_thoughts_width默认120、fork_secondary_model空串、compact_mode=false。optional字段默认None并省略序列化，不等于本层已经解析其注释里的产品默认或枚举范围。
- 字段完整覆盖theme/simple/permission/default_selected_permission、timestamps/timeline/page_flip、auto_dark/light、scroll_speed/mode/invert/lines、vim、mermaid、hunk_tracker、mouse_reporting_toggle、cancel_subagents、remember_tool_approvals、selection、thinking/group_tool_verbs/suggestions、cursor_blink/screen_mode、contextual_hints/combine_queued_prompts/follow_up_behavior/display_refresh。
- 本层显式resolver：timeline None=>false，page_flip None=>true，selection仅精确hold/word_select=>true，无trim/大小写容忍。scroll_speed/lines只用u8，注释1–100/1–10不在serde层实现范围检查；字符串枚举类多为Option<String>，运行时解释需所属pager/shell核对。
- FollowUpBehavior默认Queue，canonical解析trim后精确queue/steer，未知或空字符串=>None；字段deserializer未知字符串回退Queue，非string失败；默认Queue序列化省略，Steer序列化steer。该类型只表达设置，真实入队/steer逻辑位于shell。
- ContextualHints七项undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap均Option<bool>，全部None时整体省略，Some(false)也视为显式配置。DisplayRefreshSettings复用config-types定义，默认整体省略。
- ui_config八项测试已阅读：pageflip、selection、字符串类型、display_refresh解析/default、followup canonical/default/serde。未运行本包测试。
- clipboard_probe示例一次get_attachments，打印毫秒、image MIME与字节数、file_urls行数，anyhow错误非零退出；为避免读取用户剪贴板，本审阅未执行此示例。

## placeholder 起始段

- display-number meta键grow.dev/imageDisplayNumber，写usize、读u64转usize；读取不排除0。
- regex严格要求[Image #数字: 路径]的冒号空格，路径不含右括号/CR/LF；take16在filter_map之前，溢出数字/trim空路径也占匹配名额；返回源字节span，编号0没有额外拒绝。
- strip_paths只处理前16 regex匹配，输出[Image #原数字]；不做extract的usize解析或trim非空校验，超大数字也可strip，超过cap原样保留。无match返回原String。
- 常量单图50000000字节、每prompt16项、aggregate200MiB；扩展名png/jpg/jpeg/gif/webp/bmp/tiff/tif。敏感路径列表photoslibrary/musiclibrary/imovielibrary、Trash、Keychains、Containers、ssh/aws/gnupg；实际加载实现待读，尚不声称这些注释已经完整证明文件验证。

## placeholder 加载与恢复实现

继续读取251–980行：生产实现至661行全部读完，测试读至980；981–1438测试仍待补。

- default_allowed_prefixes对cwd和HOME下Downloads/Desktop/Pictures/Documents/Screenshots分别canonicalize；失败项丢弃，排序去重。不额外拒绝cwd恰好为HOME或根目录；不存在目录不预留prefix。
- load_placeholder_image_with_cap先canonicalize，再调用canonical loader；因此注释“out-of-scope无论存在与否同一错误”不覆盖wrapper：不存在先CanonicalizeFailed，已存在且范围外返回OutsideAllowedPrefixes。错误enum未携带原IO文本，但恢复warn输出用户原path。
- canonical loader信任入参已canonical，不重新验证。依次prefix starts_with、将反斜杠转斜杠后的大小写敏感deny substring、扩展名小写allowlist、metadata普通文件/大小、fs::read、读取后大小、共享header验证。metadata和read分别按路径访问，无文件句柄绑定；不能宣称消除路径替换race。读后限长不能证明读过程中内存有界。
- decode_image_mime委托tools::util::image_validate::validate_image_bytes_with(data,false)，只按注释/调用声明header模式；准确decoder限制须核对该共享实现。这里未比较扩展名与sniff MIME相等，也不重编码，原字节返回。
- recovery先从进入函数时已有raw_images URI生成固定HashSet，不把本轮新增canonical路径加入集合；同一query重复orphan路径会重复恢复。已有URI去重只按路径，不按display_number/data。
- 各placeholder先canonicalize失败continue，已有attached path skip，加载失败warn continue。成功读完后才检查saturating aggregate_bytes+len，超过aggregate_max即break（不尝试后面更小图）；等于允许。aggregate只计本轮恢复原字节，不计原附件或base64膨胀。
- append的ACP ImageContent使用STANDARD base64、sniff MIME、未percent编码file://canonical URI，以及真实display_number meta；输入query由借用传入不改写，strip是独立函数。
- canonical_from_file_uri只接受精确file://前缀。percent decode成功就采用decoded path，即使该路径canonicalize失败也只返回decoded raw，不再次尝试literal路径；decode错误才回退literal。不是完整URL authority/platform解析器。
- 已读测试覆盖strip保留锚点/Unicode/周围文本、extract格式/编号0/数字大值/首个右括号终止/16cap、普通PNG加载、缺失/范围外/扩展名/非图/magic伪造/目录、非root不可读权限及小cap超限开头。剩余测试未读完，不计完整验证。

## placeholder 全文件测试完成及clipboard公共入口

placeholder_images.rs 981–1438测试全部读完；clipboard.rs已读1–480，后续481–3352待读。

- 剩余placeholder测试覆盖静态symlink逃逸/dotdot/所有deny项及正对照、HOME子目录集合/缺失cwd/去重、非file URI与percent路径、恢复编号meta、已附canonical/symlink/percent URI去重、placeholder内部percent路径不解码、空/缺失/范围外、aggregate恰好及少1边界。没有测试同query重复orphan、路径并发替换或wrapper范围外缺失的信息差异。
- clipboard公共get_text/get_image/get_file_urls/get_attachments和元数据probe均委托platform；probe_supported仅cfg macOS判定不检查AppKit实际可用。ImageData为encoded bytes+MIME，不是RGBA。
- image_pasteable_from_types精确识别png/tiff/jpeg UTI，但任一public.file-url或NSFilenamesPboardType强制false；native类型优先PNG、TIFF、JPEG，与输入类型排序无关。
- mime_to_extension精确映射png/jpeg/tiff/gif/webp/bmp，否则bin；mime_from_bytes只识别PNG/JPEG/TIFF/GIF8/RIFF+WEBP/BM前缀，否则octet-stream，没有decoder验证。
- NativeWriteOutcome保留CLI尝试/成功工具、arboard_ok/data_control/any_ok；set_text只以any_ok决定Ok，否则统一no clipboard backend available；真实platform实现待读。
- wait_with_deadline每15ms try_wait；进程已退出先返回（哪怕deadline为0），过期kill+wait错误忽略后返回WaitTimeout。调用方负责关闭stdin。它只终止直接child，不含进程组/后代保证。
- spool_for_stdin把全部bytes写NamedTempFile，再reopen返回reader；named tempfile离开作用域清理路径，避免通过pipe喂大payload时阻塞写入。仍有同步临时文件IO成本。
- OSC52使用STANDARD base64与BEL终止，可显式tmux DCS封装；set_text_osc52通过共享stderr锁write_all+flush，只证明输出成功，不验证终端剪贴板接受。
- is_remote_session只判断SSH_CONNECTION/SSH_TTY/SSH_CLIENT变量存在（空值也true）；不直接判断tmux/screen。is_containerized_without_display开头只要DISPLAY或WAYLAND_DISPLAY存在即false（空值也算），然后检查Docker/Podman哨兵；函数余部待读。

## macOS clipboard 阅读阶段

clipboard.rs继续读取481–1130，后续1131–3352待读。

- 无display的容器判定还接受container环境变量存在；否则false。
- attachments_protocol以IMAGE marker split_once，FURL段contains marker但仅strip_prefix；非前缀marker不会被剥掉。furl trim后空或精确none=>None；image段需IMAGE:前缀且精确PNGf/TIFF/JPEG。parser本身可同时返回两者，由get_attachments决定file优先。
- AppKit通过OnceLock<bool>缓存dlopen结果，失败也不重试；prewarm只加载框架。macOS Wayland probe恒Available(false)。PASTEBOARD_LOCK覆盖原生metadata和image读取autoreleasepool；不会阻止外部进程改变剪贴板。snapshot先changeCount再types，没有读后二次count比较，因此注释“同一状态”不能扩成跨进程原子snapshot保证。
- native_image_read的kill switch按变量存在判断（值0也禁用）；类型匹配后dataForType取encoded bytes，nil/len0/null指针=>None。按NSData len分配并复制，无本层字节cap或decoder验证，MIME来自所选UTI。
- checked_command_stdout检查启动及exit status，失败消息含stderr，成功返回stdout；read侧cmd.output无deadline。
- fallback图片固定使用temp_dir/grow-clipboard-probe.png/.tiff/.jpg，调用前remove三个文件。路径插入AppleScript双引号字符串时无escape；未使用NamedTempFile、随机名或进程间锁。潜在并发/路径字符边界单列债务，不能声称安全临时文件协议。
- read_clipboard_image_from_class按class选固定路径、fs::read全部内容、非空即接受并赋予class MIME；读取失败或空移除对应文件，成功也移除对应文件。没有共享header验证或大小限制；其他分支remove全部是best-effort。
- unified AppleScript先list逐项furl，再single回退；未取到file时PNGf→TIFF→JPEG coercion写临时文件。get_attachments原生image优先，否则subprocess；file_urls存在则删除temps并不读image。raw空早返，不在该分支再次清理。
- get_text使用pbpaste -Prefer txt，空字节None，其他包括空白/换行原样lossy UTF8，不trim；detached command output无deadline。
- macOS set_text_with_outcome把pbcopy列为tried，spool文件stdin，detached spawn后2秒deadline，只凭exit成功标cli_ok/any_ok，不读回确认；错误debug并返回失败outcome。
- get_image先native，再独立image-only AppleScript（不检查furl优先），none删除temps；未知class返回None但read_class分支不清理全部。get_file_urls生产实现尚待读。

## macOS尾部与Linux探测

clipboard.rs推进1131–1795；1796–3352待读。

- macOS get_file_urls实际调用get_attachments后丢弃image，可能执行native图片内容读取，并非只请求文件列表。set_image_file按扩展名jpg/jpeg=>JPEG、tif/tiff=>TIFF，其余PNGf，只转义双引号，osascript output无deadline；不验证内容与扩展一致。
- 非macOS元数据probe固定(None,false)/None，prewarm noop。spawn_with_deadline在独立线程执行closure，超时仅放弃接收，不终止worker；spawn失败/worker退出未回复=>Disconnected。
- Linux data-control kill switch按变量存在首次缓存，Wayland非空环境时同时绕过arboard；arboard lease一次初始化2秒，失败/超时/禁用永久None，成功保留进程级Mutex实例。写入持锁set_text无deadline；读取每次新worker+新Clipboard实例2秒，超时线程可遗留，没有并发数量上限。
- arboard text空串或ContentNotAvailable=>None；PRIMARY还把ClipboardNotSupported=>None。arboard图片转RGBA后encode PNG，width/height以as u32转换，转换实现待后续核对。
- Linux工具argv：wl-paste --no-newline -t text，wl-copy；xclip clipboard读写以及primary读取；xsel clipboard读写和primary读取，不支持typed PNG。linux_tool_spec首次探测结果（包括None）永久缓存，优先非空WAYLAND_DISPLAY+wl-copy，其次非空DISPLAY+xclip/xsel。
- data-control显式probe 2秒worker，调用is_primary_selection_supported：Ok(true)与Ok(false)都代表协议存在=>Available(true)，MissingProtocol=>Available(false)，NoSeats/连接/通信错误=>Unavailable，worker断开=>Error。不要将primary能力布尔值误读为data-control存在与否。
- bool缓存确定答案永久缓存；Unavailable/Error累计3次后永久false，此前本次false但允许以后重试。持Mutex进行探测串行化；显式probe不读取或更新该bool缓存。环境门禁直接false，kill switch独立OnceLock。
- 工具available执行--version并1秒wait，任何exit status都算可用，只要及时退出。PRIMARY工具发现只缓存成功、失败会重试；普通tool spec和write spec缓存失败列表。
- 写入后端列表最多wl-copy加一个X11（xclip优先xsel）；collect前实际无条件探测三个工具，即使对应环境缺失。PRIMARY允许arboard fallback仅非空DISPLAY且无非空WAYLAND_DISPLAY；具体读分派尚待读。

## Linux读写生产实现完成

clipboard.rs已读1796–2445；生产代码已全读，剩2446–3352测试待读。

- CLI写入spool stdin、2秒wait、非零失败。capture在worker read_to_end stdout，忽略读取错误；child deadline后才join reader，若后代继承stdout仍持有可能无界join。无stdout字节上限；超时提前?丢reader handle，不保证后代结束。
- run_capture_out非零返回空Vec，checked变体非零错误。Wayland回读用前者，精确byte比较text；空text与失败返回空Vec可判匹配，不能泛化为回读成功证明。
- readback最多3次，失败间100ms，每次CLI_PROBE_WAIT1秒；Wayland且非(data_control&&arboard_ok)才要求回读，X11和可信arboard成功分支不要求。
- get_text/get_image优先arboard Some，None只有选中Wayland CLI才继续；arboard Err也fallback选中工具。CLI错误传播，text lossy UTF8，image魔术MIME可octet-stream，无完整解码。旧X11 Some可能遮住Wayland新selection。
- PRIMARY需非空DISPLAY，xclip优先xsel，CLI成功空是权威None；非UTF8/错误尝试下一工具，全部失败仅纯X11可arboard fallback，XWayland返回None避免读错selection。
- set_text先probe data_control、arboard写，再所有可用CLI；any_ok最终是cli_ok||arboard_ok，即使无data_control也可因arboard成功为true。每CLI成功按是否需回读标记，失败仅debug，未实现系统原子广播。
- Linux set_image_file fs::read全部输入字节，发送所有支持PNG argv的后端，不sniff或转码也不回读，任何一个成功即Ok；无可用工具error，其余非macOS非Linux平台不支持。
- encode_rgba_to_png普通乘法width*height*4，短缓冲报错，长缓冲未截取直接传encoder（最终是否接受由image库决定），并非保证at-least长度都成功；arboard dimensions转u32前无范围检查。
- 非macOS get_attachments先get_file_urls()?，有file跳过image；file-list错误会阻断image fallback。get_file_urls只arboard无CLI fallback，ContentNotAvailable/空list=>None，其他错误传播；paths display join newline不做canonical/absolute校验。
- 已读worker tests快速值/超时放弃/panic断开；Linux PRIMARY前段tests覆盖argv、无DISPLAY不探测、xclip优先、positive cache、运行失败fallback、empty权威、缺工具、全部失败、pureX11 fallback。跨平台测试尚未新运行。

## 全包阅读完成与验证启动

clipboard.rs剩余2446–3352已全读；全包8个Rust文件5339行及manifest均读完。结构化能力映射尚未生成，保持pending。

剩余Linux测试覆盖Wayland empty fallback、typed text argv、多写后端组合、逐字节回读/空值、重试次数/门禁全矩阵、dependency error分类、3次不确定/错误缓存、确定后不再探测及4并发调用共享一次probe。非macOSworker与RGBA测试受cfg限制，不会在本机macOS运行。macOS命令结果用合成Output，公共测试用临时文件/自有sleep进程和纯OSC52/MIME/协议解析；真实剪贴板round_trip、text-only image与两个native probe smoke均ignore。没有开启ignored，不读写用户剪贴板。

启动默认 `cargo test --locked -p client-support`，日志 `/tmp/grow-client-support-inventory-tests.log`；等待结果，不预报通过。

## client-support 测试终态

会话38227退出0，默认macOS `cargo test --locked -p client-support`：97 passed、0 failed、4 ignored，doc-tests 0。ignored均为真实剪贴板测试，Linux/非macOS cfg测试未在此运行；不声称跨平台动态验证。git diff --check通过，能力映射仍待完成。

## 能力映射完成

# client-support 逐包核查

包路径：`crates/codegen/client-support`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，默认macOS测试97通过、4忽略。

## 模块与开关

- `crates/codegen/client-support/Cargo.toml`
- `crates/codegen/client-support/examples/clipboard_probe.rs`
- `crates/codegen/client-support/src/clipboard.rs`
- `crates/codegen/client-support/src/lib.rs`
- `crates/codegen/client-support/src/placeholder_images.rs`
- `crates/codegen/client-support/src/session/info.rs`
- `crates/codegen/client-support/src/session/mod.rs`
- `crates/codegen/client-support/src/stderr.rs`
- `crates/codegen/client-support/src/ui_config.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Shared UI configuration schema](../specs/client-surfaces/spec.md#requirement-shared-ui-configuration-schema)：UiConfig SHALL 提供主题、fork模型、compact/simple、权限与默认选项、timestamps/timeline/page_flip、自动深浅主题、scroll参数、vim、mermaid、hunk_tracker、鼠标、子agent取消、approval记忆、selection、thinking/group/suggestions、cursor/screen、hints/queue/display_refresh设置。
- [Shared UI setting resolvers](../specs/client-surfaces/spec.md#requirement-shared-ui-setting-resolvers)：UI resolver SHALL 将timeline未配置解析false、page_flip未配置解析true，selection精确hold/word_select解析为持续高亮。
- [Follow up behavior wire defaults](../specs/client-surfaces/spec.md#requirement-follow-up-behavior-wire-defaults)：FollowUpBehavior SHALL 使用queue/steer canonical，trim后匹配，默认Queue且默认值序列化省略。
- [Contextual hint and refresh overrides](../specs/client-surfaces/spec.md#requirement-contextual-hint-and-refresh-overrides)：ContextualHints SHALL 保存undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap七项Option<bool>，display_refresh复用config-types设置。
- [Shared session identity and stderr serialization](../specs/client-surfaces/spec.md#requirement-shared-session-identity-and-stderr-serialization)：client-support SHALL 提供id/cwd会话身份与sessions_cwd_dir(cwd)下join id的目录投影，并通过全局Mutex串行化with_locked_stderr。
- [Image placeholder parsing and path stripping](../specs/client-surfaces/spec.md#requirement-image-placeholder-parsing-and-path-stripping)：图片占位符 SHALL 匹配精确[Image #数字: 路径]，排除路径内右括号/CR/LF，最多检查前16个regex匹配；extract返回编号、trim路径与源字节span。
- [Image display number metadata](../specs/client-surfaces/spec.md#requirement-image-display-number-metadata)：附件编号 SHALL 写入grow.dev/imageDisplayNumber元数据，读取以u64转usize，不按附件数组位置推断编号。
- [Placeholder image prefix construction](../specs/client-surfaces/spec.md#requirement-placeholder-image-prefix-construction)：默认图片prefix SHALL canonicalize cwd和HOME下Downloads/Desktop/Pictures/Documents/Screenshots，丢弃失败项并排序去重。
- [Placeholder file loading validation order](../specs/client-surfaces/spec.md#requirement-placeholder-file-loading-validation-order)：图片加载 SHALL 先canonicalize，再检查prefix和敏感子串、允许扩展、普通文件、读前读后字节cap及共享header验证；默认单图50000000字节。
- [Placeholder image type and sensitive path rules](../specs/client-surfaces/spec.md#requirement-placeholder-image-type-and-sensitive-path-rules)：占位符 SHALL 允许png/jpg/jpeg/gif/webp/bmp/tiff/tif扩展，反斜杠归一后大小写敏感拒绝photoslibrary/musiclibrary/imovielibrary、Trash、Keychains、Containers、ssh/aws/gnupg子树。
- [Orphan image recovery and aggregate budget](../specs/client-surfaces/spec.md#requirement-orphan-image-recovery-and-aggregate-budget)：孤立图片恢复 SHALL 按query顺序加载并追加STANDARD base64 ACP ImageContent、未percent编码file URI及display meta；单次恢复默认aggregate200MiB。
- [Relaxed file URI canonicalization](../specs/client-surfaces/spec.md#requirement-relaxed-file-uri-canonicalization)：file URI辅助 SHALL 接受精确file://前缀，先percent decode再canonicalize，失败回退decoded路径。
- [Clipboard image data and MIME helpers](../specs/client-surfaces/spec.md#requirement-clipboard-image-data-and-mime-helpers)：剪贴板ImageData SHALL 保存encoded bytes和MIME；mime_from_bytes仅检查PNG/JPEG/TIFF/GIF/WEBP/BMP魔术前缀，mime_to_extension精确映射相应扩展。
- [Clipboard process wait and stdin spooling](../specs/client-surfaces/spec.md#requirement-clipboard-process-wait-and-stdin-spooling)：clipboard helper SHALL 以临时文件spool完整stdin，wait每15ms检查child，超deadline杀死并回收直接child后返回WaitTimeout。
- [OSC52 emission and remote environment detection](../specs/client-surfaces/spec.md#requirement-osc52-emission-and-remote-environment-detection)：OSC52 SHALL 将UTF8文本STANDARD base64编码为clipboard c序列，可显式加tmux DCS封装，经共享stderr锁写入并flush。
- [MacOS pasteboard metadata and native image read](../specs/client-surfaces/spec.md#requirement-macos-pasteboard-metadata-and-native-image-read)：macOS SHALL lazily dlopen AppKit并缓存结果，通过Mutex串行化原生pasteboard访问；file-url类型抑制raster，原生优先PNG/TIFF/JPEG。
- [MacOS clipboard subprocess routing](../specs/client-surfaces/spec.md#requirement-macos-clipboard-subprocess-routing)：macOS SHALL 用pbpaste -Prefer txt读取文本、pbcopy spooled stdin写文本；附件优先native，否则AppleScript先furl再PNGf/TIFF/JPEG，image-only入口不要求furl优先。
- [MacOS clipboard transfer files and image writes](../specs/client-surfaces/spec.md#requirement-macos-clipboard-transfer-files-and-image-writes)：macOS fallback SHALL 通过temp_dir固定grow-clipboard-probe三种文件传图片，按class读取非空原字节并best-effort删除。
- [Clipboard attachment stdout protocol](../specs/client-surfaces/spec.md#requirement-clipboard-attachment-stdout-protocol)：AppleScript输出解析 SHALL 以FURL/IMAGE marker分段，trim furl空或none为None，image需IMAGE:及PNGf/TIFF/JPEG。
- [Arboard worker deadlines and persistent write lease](../specs/client-surfaces/spec.md#requirement-arboard-worker-deadlines-and-persistent-write-lease)：非macOS SHALL 用2秒worker创建读取实例，写入lease初始化一次并永久保留成功实例或失败None；写入持Mutex。
- [Wayland data control classification and caching](../specs/client-surfaces/spec.md#requirement-wayland-data-control-classification-and-caching)：Linux data-control SHALL 将依赖Ok任意bool视为协议存在、MissingProtocol为不存在，NoSeats/连接错误不确定；显式probe保留typed结果。
- [Linux clipboard tool discovery](../specs/client-surfaces/spec.md#requirement-linux-clipboard-tool-discovery)：Linux默认工具 SHALL 按非空Wayland+wl-copy、DISPLAY+xclip/xsel选择，普通选择及write列表首次缓存；availability为1秒内--version退出，任意exit code均可。
- [Linux clipboard text and image read precedence](../specs/client-surfaces/spec.md#requirement-linux-clipboard-text-and-image-read-precedence)：文本/图片读取 SHALL 优先arboard Some；None只有选中Wayland CLI才fallback，Err可fallback工具；CLI非零报错。
- [Linux X11 primary selection boundary](../specs/client-surfaces/spec.md#requirement-linux-x11-primary-selection-boundary)：PRIMARY SHALL 需要非空DISPLAY，优先xclip后xsel，成功空结果立即None，非法UTF8或失败尝试下一工具。
- [Linux native clipboard write outcomes](../specs/client-surfaces/spec.md#requirement-linux-native-clipboard-write-outcomes)：文本写入 SHALL 先arboard再所有可用CLI，以cli_ok或arboard_ok为any_ok；Wayland CLI在无(data_control且arboard成功)时需回读。
- [Linux image writing and RGBA conversion](../specs/client-surfaces/spec.md#requirement-linux-image-writing-and-rgba-conversion)：Linux图片文件写入 SHALL 读取全部字节并发送支持PNG的CLI，不转码不回读，任一成功即Ok；非macOS非Linux不支持图片文件写入。
- [Clipboard probe example and ignored integration tests](../specs/client-surfaces/spec.md#requirement-clipboard-probe-example-and-ignored-integration-tests)：clipboard_probe示例 SHALL 执行一次附件读取并打印耗时、MIME/字节数和路径行数，错误返回非零。

## 边界

- serde(default)使用默认值，未知字段不拒绝；max_thoughts_width=120、fork模型空串、compact=false，多数optional字段None且不序列化；字符串枚举与scroll注释范围不由本层统一校验。
- 返回false，不trim；这些resolver不代表所有UiConfig字段的产品默认已在此解析。
- 未知字符串回退Queue，非字符串失败；实际FIFO或steer执行属于shell。
- 全部None整体省略，Some(false)仍为显式选择；默认display_refresh整体省略。
- 优先dup_tui_stderr，失败回退系统stderr；锁不覆盖外部直接写入，session_dir不额外验证id路径。
- extract跳过无效项但仍消耗匹配名额，编号0允许；strip独立保留原数字锚点，不解析usize，超cap文本原样保留。
- 0可返回；缺失、负数、非整数或无法转usize返回None。
- 本层没有额外缩窄cwd；不会因为HOME参数而直接加入整个HOME，但cwd仍独立加入。
- 不存在先CanonicalizeFailed，存在范围外OutsideAllowedPrefixes；canonical入口信任调用方，metadata/read非句柄绑定且read后cap不保证峰值内存有界。
- 调用validate_image_bytes_with(data,false)验证header并sniff MIME，不要求扩展和MIME相等、不重编码原字节。
- 读完该图才检查，超cap即break，等于允许；只计本轮原字节，已有附件不计。去重集合只取入口已有URI，本轮重复orphan会重复加载。
- decode成功后不再尝试原literal路径；placeholder文本本身不percent decode，URI辅助不是完整authority解析器。
- 未知回退octet-stream/bin，前缀识别不等于decoder验证。
- 先返回已有exit status；调用方负责关闭stdin，deadline不证明后代或stdout reader join有界。
- 成功只证明输出，不确认终端接受；SSH_CONNECTION/SSH_TTY/SSH_CLIENT存在即remote，空值也算。容器探测被存在的DISPLAY/WAYLAND_DISPLAY阻止，否则检查Docker/Podman文件及container变量。
- 变量存在即禁内容读取并fallback；probe仍可运行。snapshot先count后types不保证外部变化原子性，native bytes不做cap或decoder验证。
- 空字节None，非空lossy UTF8原样；读侧output无deadline，pbcopy写侧2秒。get_file_urls调用get_attachments，可能读取并丢弃图片。
- 固定名未提供进程间隔离；set_image_file按jpg/jpeg、tif/tiff及其他选择JPEG/TIFF/PNGf，仅转义双引号、无内容校验和deadline。
- parser保留两者，由get_attachments选择file优先；contains但不在前缀的FURL marker不被strip_prefix去掉。
- 超时放弃worker而非取消，无累计worker上限；lease.set_text自身无deadline。Linux kill switch在Wayland下绕过arboard。
- bool缓存本次false并重试，累计3次永久false；确定答案永久缓存，锁内串行probe；显式probe不改该缓存，kill switch首次读取缓存。
- PRIMARY只缓存成功发现；write列表可同时wl-copy和一个X11工具，xclip优先xsel。
- arboard Some可遮住Wayland；get_attachments先file-list错误即传播而不读image，file-list无CLI fallback。非macOS元数据probe返回不可用。
- 返回None；只有无Wayland的纯X11可arboard fallback，避免读到Wayland PRIMARY。
- 最多3次精确字节比较，间隔100ms，每次1秒；非零read转换为空Vec，空文本可能匹配。X11不回读，数据控制成功时wl-copy也不回读。
- arboard RGBA转PNG，短buffer报错、长buffer直接传encoder，尺寸转换/乘法未单独检查；无CLI图片后端返回错误。
- 真实剪贴板roundtrip、text-only image及macOS两项native smoke保持ignored，不据此推断实际桌面服务验证通过。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
