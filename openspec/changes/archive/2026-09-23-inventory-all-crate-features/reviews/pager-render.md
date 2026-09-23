# pager-render 逐包审阅

## Manifest 与模块入口

Cargo.toml及lib.rs完整读取；67份Rust、37561行的inventory仍pending。模块为appearance、clipboard、gboom、glyphs、host、link_opener、modal_window_state、prompt_images、render、syntax、terminal、theme、util。test-support为空声明，具体cfg使用后续核对；default-bazel启用test-support，无default feature。notify optional依赖还产生隐式notify feature，inventory当前features只列显式项，正式契约需补齐这一开关。Unix用libc，Windows用windows-sys GDI；渲染依赖ratatui/crossterm、VTE、markdown/syntect，图片支持PNG/JPEG/TIFF/GIF/WebP。依赖声明不当作运行时行为证据。

## host/mod.rs 完整阅读

collect_unicode_env通过vars_os过滤非Unicode key/value，不因std::env::vars遇非Unicode而panic。HostOs由编译target_os选择macOS/Linux/Windows/Other；is_wsl重导出tty-utils实现。DisplayServer current通过OnceLock缓存首次判断；macOS固定Quartz，Windows固定Win32，Linux优先非空WAYLAND_DISPLAY，再非空DISPLAY，否则Unknown，其他OS Unknown。不探测实际display socket可用性，不trim空白变量。Unix/Windows非Unicode测试和Linux五种display变量测试已读，尚未执行本包测试。display_refresh只见重导出，具体探测尚未读。

## glyphs.rs 初读（1–160行）

已读prompt_arrow、collapsed_accent、ballot_x、check_mark、enlarge、copy_icon、token_arrow、monitor_icon_frames及diamond_filled；按is_legacy_windows_console选择普通Unicode与ASCII/CP437替代。PROMPT_ARROW_WIDTH为2，宽度声明待tests/terminal检测实现核验，不当作所有字体保证。monitor四帧两组固定字符；实际动画调度不在这些函数内。首次整文件输出截断，未读中间不计完成；后续从161行继续。

当前完整读取2/67 Rust（lib.rs、host/mod.rs）及manifest；glyphs仅1–160行。本轮无Cargo构建，包保持pending。

- `crates/codegen/pager-render/Cargo.toml` SHA256 `e8140e40f4418fa416fbcaffb89bd87e08fe18876d2bfd60c43eee7687accf6c`

- `crates/codegen/pager-render/src/lib.rs` SHA256 `2fdc9415afb5ad994113ed331b490313a63c1d022966bfb28a88667087c38fc0`

- `crates/codegen/pager-render/src/host/mod.rs` SHA256 `5d570dd4cd98abcbbc48c8e9e104af242f759606a390d9777baa72028272fcd2`


## glyphs.rs 完整阅读（161行至文件末）

补齐diamond、braille/dot spinner、accent/selection bar、timeline chevron与双列tick、filled dot、左右下chevron、disclosure及预组合按钮。普通/legacy两套字符由同一判定选择；light horizontal与hover tick固定，active tick legacy用双线区分hover。char helper取字符串首char并有默认fallback。

is_legacy_windows_console以OnceLock缓存首次判定；GROW_FORCE_LEGACY_CONSOLE只接受精确1/true/0/false，不trim或大小写归一，可跨平台强制。无override时非Windows永不legacy，Windows仅WindowsTerminal/VSCode/Cursor/Windsurf/Zed/WezTerm/Kitty/Alacritty/Ghostty/Rio白名单非legacy，其余（包括Unknown）按legacy；使用env_brand而不是可能乐观回退的brand。不能把判定等同真实字体探测。

自由文本fallback仅替换✓→√、✗→x、⚠→!；非legacy或没有这些字符返回Cow借用，不统一替换全部chrome字符。sanitize_toast_message先字形回退，再将char::is_control字符逐个换空格，干净输入借用；不删除其他Unicode格式字符，不解析终端转义序列的整个载荷。

全部单测已阅读，unicode-width验证表中固定字符列宽、两种按钮宽度、纯decision/override解析、三个字形替换和控制字符清理。部分helper测试直接assert非legacy，未加平台条件，因此依赖测试环境/首次缓存值；不代表Windows legacy或强制override环境可直接运行全部样例。当前未执行本包测试，不据unicode-width断言保证所有真实字体视觉相同。

pager-render完整读取3/67 Rust及manifest；下一步host/display_refresh。无Cargo构建。

- `crates/codegen/pager-render/src/glyphs.rs` SHA256 `1a15c1c634521d6c436261ea941b1b96014439eb17994dae33ac58ca02b55d93`


## host/display_refresh.rs 完整阅读（505行）

probe_display_refresh使用OnceLock缓存整个首次结果，包括耗时；每次调用不重新测量。先通过client-support远程会话与tty-utils WSL识别，SSH优先于WSL，跳过时不调用平台FFI。非远程macOS通过CoreGraphics主显示器CopyDisplayMode/GetRefreshRate/Release查询，null或非有限/负数返回error，0为indeterminate，其他浮点round后转u32；最终只接受30..=500Hz。Windows最多枚举32个桌面adapter，选择PRIMARY_DEVICE，再以该DeviceName查询CURRENT_SETTINGS，失败或频率小于2返回error；最终同范围过滤。Linux不探测，按display返回wayland_unsupported/x11_unsupported/no_display，Other unsupported。

FFI调用外围catch_unwind仅处理Rust unwind，abort构建仍可中止，不是任何系统故障都保证不panic/不退出。未看到TTY读写或模式修改。outcome仅看hz Some优先ok，否则skip_reason精确error才error，其余skipped；字段public可人工构造不一致对象，本层不校验其相互关系。duration取elapsed毫秒as u64，不是饱和转换。

17项测试全部读取：一个真实OS烟测与缓存结果复用，其他为纯矩阵注入平台结果、SSH/WSL/Linux/Other、上下界、错误/indeterminate、字符串枚举和outcome。真实烟测允许ok/skipped/error，不证明真实硬件刷新率准确或多显示器/VRR变化后刷新缓存。尚未执行本包测试。

当前完整读取4/67 Rust及manifest；本轮无Cargo构建。

- `crates/codegen/pager-render/src/host/display_refresh.rs` SHA256 `ee2f8cd4e76be95ba15e412064573149cc746bb38e612d16891980b6d48e1eab`


## link_opener.rs 完整阅读（531行）

open_url本身不校验scheme；try_open_url先is_safe_to_open，区分RejectedScheme/BrowserUnavailable/Opened。校验trim后的URL，先Url解析scheme，失败用://前缀或mailto冒号回退并小写；实际open仍传原始未trim字符串。SchemeFilter具体白名单后续terminal/hyperlinks核对。允许scheme不代表完整URL语法或目标可信。

GROW_TEST_OPEN_URL_FILE不受cfg(test)限制，存在时追加URL换行并跳过OS opener，写失败返回false。普通路径macOS/Windows环境层固定available，其他平台需非空BROWSER/DISPLAY/WAYLAND_DISPLAY；BROWSER只影响是否尝试，实际仍调用xdg-open。macOS用open，Windows用cmd /c start空标题，其他用xdg-open；stdio置null并detach，只以spawn成功返回true，不等浏览器打开或子进程退出。spawn失败日志去query/fragment但未清userinfo/path；拒scheme debug日志直接记录完整URL，不能笼统称所有日志脱敏。

open_path接收可信路径不做scheme检查；非Windows单参数传open/xdg-open，无GUI availability门禁；cfg(test)仅返回路径非空不真正打开。Windows直接Explorer raw_arg，文件select、目录打开，路径缺失但parent存在则打开parent；不会canonicalize或强制转绝对路径，注释absolute不是实现保证。引号双写，百分号不经cmd展开；spawn成功仍不是目标展示确认。

人工打开提示保留完整URL，多行版单列URL，单行版URL优先且仅copied=true添加已复制说明。测试已完整读取：scheme案例、环境可用性、手动提示、拒scheme及非Windows命令单参数。fallback案例未证明Url解析真的失败；Windows raw_arg路径不在本文件测试执行，首个command测试引用cfg非Windows函数却无测试cfg保护，待跨平台编译核验，不附带修复。

当前完整读取5/67 Rust及manifest，未运行本包测试或打开任何真实URL。

- `crates/codegen/pager-render/src/link_opener.rs` SHA256 `fff75d64388c9876a9eaadfd69d300e9efda5723f049e85df432047395060386`


## terminal/hyperlinks.rs 完整阅读（297行）

SchemeFilter精确匹配小写：Standard仅http/https/mailto，EditorExtended另加file/vscode/cursor/idea/zed；本方法不自行归一大小写。品牌映射全部选择Standard，没有品牌自动启用EditorExtended。AppleTerminal HostileParser，Warp Unsupported但native_plain_url_open=true，JetBrains/Otty/Unknown为Unknown，其余枚举品牌Native且id_param=true。OSC22仅Iterm2/Ghostty/Kitty启用；native_link_hover仅VSCode/Cursor/Windsurf/Zed。此处纯品牌表，不探测版本；注释中的最低版本及VTE resolver限制后续核对，不当成本函数已经执行。

SetPointerCursor/SetDefaultCursor只生成OSC22 pointer/default串，Windows execute_winapi无操作成功，不自己检查capability。7项测试覆盖部分品牌、Warp裸URL、Standard和Extended白名单；未测试全部品牌字段或发送真实终端序列，未执行本包测试。

## modal_window_state.rs 完整阅读（84行）

纯公开数据：close hover/hit rect、popup area、active tab/count/rects/focus及footer shortcut hit/hover。new/default全部空或false且active_tab0；with_tabs只设置count和等长None rects，不分配内容/注册事件。ShortcutHitArea保存rect、调用方id、原shortcuts_idx与clickable；类型不执行点击、边界校验或渲染，public字段也不强制active_tab小于count。无内联测试，行为消费者仍属pager审阅。

当前完整读取7/67 Rust及manifest；本轮未构建。

- `crates/codegen/pager-render/src/terminal/hyperlinks.rs` SHA256 `83c801f19c15193f3c9b3575dfdc299bced133b9ef7d655c8316f4f24e1c5dbe`

- `crates/codegen/pager-render/src/modal_window_state.rs` SHA256 `023d33dbbacb445a6772eeb687a0e10e79bb93e50dda20ba9f55f23ab1c642df`


## util.rs 完整阅读（558行）

grow_home重导出config，pager路径为其下pager.toml；展示前缀按解析后路径等于default home选择~/.grow或$GROW_HOME，不按变量是否设置。display_user_grow_path为显示字符串拼接，不校验relative参数。abbreviate优先Path strip_prefix grow home，再以HOME字符串及/边界缩写；is_under_user_grow_home仅词法starts_with，不canonicalize链接或..，不能作安全边界。

format_duration按as_secs分档：低于10用一位小数、其余秒/分/小时截断。format_time_ago按分钟/小时/天、30天月、365天年，360至364天可显示12mo。time_until_time_ago_change按当前单位下个倍数算截止；360天时算390天而format在365天切1y，故跨年阈值存在不一致候选，现有截止测试只覆盖小时内。Unix毫秒非正/加法溢出回退now，未来时间保留，调用方elapsed错误默认0才显示just now；Instant投影用捕获clock pair，未来anchor夹到wall_now，减法失败也回退wall_now。

decode_html_entities遇无&借用，否则依次替换amp/lt/gt/quot/39/x27/apos，可能将&amp;lt;继续解成<，不是完整HTML实体解析或安全转义。group_thousands从右每三位逗号，u64不涉及符号/小数。

parse_schedule_interval_secs要求trim_start后精确every空格；number u64，单位s/second(s)、m/minute(s)、h/hour(s)、d/day(s)，允许0，普通乘法无checked overflow。查找Unicode whitespace后却按sp+1字节切片，以及无空格按末字节split_at，含多字节分隔/末字符可能panic；只是静态候选，后续需结合调用方输入核对，不能把返回Option视作所有字符串均安全失败。测试覆盖常规单位与非法ASCII，未覆盖这些Unicode/极大值。HOME测试手动修改/恢复环境且仅自身serial，不是panic-safe guard；本轮未运行测试。

当前完整读取8/67 Rust及manifest，无Cargo构建。

- `crates/codegen/pager-render/src/util.rs` SHA256 `308d6d337602e0eb02db8f90cce35a273ff8ee6641408e27b814958e1eeeb1be`


## clipboard/trust.rs 完整阅读（602行）

ClipboardDelivery分Confirmed/Unverified/Failed；reported_success对前两者均true，不能把旧bool成功解释为已确认。OSC52能力以显式sink或brand支持为Supported，Unknown品牌为Unknown，其余Unsupported；Supported即策略Confirmed，Unknown仅remote/container时Unverified，本地Unknown Failed，不是剪贴板读回确认。

native preflight先按route关闭Disabled，再remote/container RemoteOnly；本地macOS/Windows LocalAvailable，Linux X11可用，Wayland需wl-copy存在或data-control，其他Unavailable。实际trusted_native拒remote/container及未选native，Wayland要求wl_copy成功或arboard成功且data_control；Linux其他display、macOS/Windows/Other只看cli/arboard成功。故preflight与实际成功判定并非完全同一平台矩阵。

expected_delivery仅预期：native LocalAvailable或启用tmux即可Confirmed；否则按启用OSC的策略。resolve_copy_decision先可信native，后成功OSC；Confirmed容器优先，remote VSCode family且非ASCII给特殊警告，其他remote/local各反馈；Unknown OSC在没有tmux成功时Unverified，有tmux成功则CopiedTmux。最终无可信结果remote/container FailedRemote，否则Failed。各腿是否真的写成功由clipboard/mod提供，此文件不执行写入。

测试全部读取（首次输出截断段230–340已补读）：分类标签/历史bool、本地/Wayland信任、远程native不确认、已知支持与不支持OSC、未知远程/容器、sink、tmux优先、双重环境容器优先、VSCode非ASCII和preflight矩阵。均为纯注入，不读取真实剪贴板，尚未运行本包测试。当前完整读取9/67 Rust，clipboard/mod.rs2331行待审，无构建。

- `crates/codegen/pager-render/src/clipboard/trust.rs` SHA256 `34b5f003e4245e3dc9f75b87713ea0d3f25a483e88a206773c6d63ef1e7fa008`


## clipboard/mod.rs 初读（1–470/2331行）

remote、container-no-display、OSC sink与NO_OSC52均首次OnceLock缓存；sink接受GROW_OSC52_SINK或LC_GROW_OSC52_SINK存在（任意值），禁用开关同样按存在。route首次从terminal context解析，native恒true，tmux仅multiplexer=Tmux；OSC在未禁用且Linux/tmux/remote/container/sink任一成立时启用。DCS passthrough仅OSC启用、tmux且无embedded editor；所谓pure resolve_with仍读取其余ambient缓存，不完全纯。

写入按native→tmux→OSC同步依次执行，前一成功不短路后续；native委托client-support并保存每腿结果，tmux先spool stdin再detach运行load-buffer -，最多等待2秒且exit成功才true，OSC调用成功只说明发出。SystemClipboard get将底层错误与无文本都变None，set忽略结果，try_set返回策略delivery。

CopyResult静态message/lead、ticks与delivery。普通Copied 30 ticks，其余120；tick注释按30fps估时不证明实际时长。环境用terminal brand（不是glyph的env_brand）；copy_text记录开始时间、同步全部写入、失败warn仅长度与display，随后diagnostics记录具体字段待后续函数核对。无备份逻辑属于copy_text本身。

CopyDelivery类型分Clipboard(result, optional backup)、File(path)、Failed(clipboard,file_error)，success仅非Failed；“总写备份”等注释尚未核对实现，后续从471行继续。当前完整9/67 Rust，主模块未完，无Cargo构建。


## clipboard/mod.rs 续读（471–920行）

copy_text_or_file先完整copy_text再总尝试备份。resolve_delivery把Unverified也视reported_success，备份失败仍Clipboard且success=true，因此success注释“能取回”并非严格保证。Confirmed toast不显示backup；Unverified有backup则显示路径，无backup仅Copy sent；剪贴板Failed但备份成功才File。默认路径GROW_COPY_FILE trim/tilde或user_grow_home/last-copy.txt，无home返回NotFound。Unix备份先create/truncate再chmod0600后write_all，无原子rename、fsync或nofollow；已有symlink可跟随，失败可能留下截断文件。默认fallback新建父目录0700，通用write_text_to_copy_file的create_dir_all无此mode，已有目录不收紧。非Unix普通fs::write。

copy diagnostics记录text.len字节数及各route/outcome/耗时，不含正文。clipboard_stats_suffix同样用text.len却标chars；与main协作消息声称已修Unicode统计不同，当前工作树事实保持字节计数，整合时再比对main实际源码。

system_clipboard_read_text保留Result<Option<String>>，test-support可注入；get合并错误为None。Linux PRIMARY读取仅检查X11 display env，过滤空串不trim；空粘贴提示要求实际DisplayServer X11且env存在。pasteable按trim非空。lone URL只是trim后小写http(s)前缀且无LF，不验证完整URL、空格或CR；满足时跳过attachment探测。plain_text_skips_furl允许file://、>=4096字节或无://文本；并不同时禁止图片探测。bracketed payload探测规则空true、lone URL false、>=4096 false，其余<=4行或含://。来源比对将CRLF/CR转LF并trim_end，None剪贴板仅匹配trim空payload；bool包装将读取失败当false，typed保留错误。AttachmentProbeRoute三变体已读，路由实现从921继续。

当前9/67 Rust完整，主模块已读1–920/2331；本轮无Cargo构建。


## clipboard/mod.rs 续读（921–1390行）

attachment_probe_route：lone URL Skip；空文本或不能跳furl的文本FileUrlsThenImage；其余ImageOnly。gate总读取一次snapshot，Skip false，FileUrlsThenImage恒true，ImageOnly仅supported+available+无图时跳过。返回Option<Option<changeCount>>区分不探测与需探测但基线不可用；本函数不执行延迟任务的过期校验，需调用方使用该基线。

system_clipboard_probe_attachments先重新gate，才读test hook或实际route；FileUrlsThenImage优先file_urls Some（不检查空串），否则image。底层get_attachments同时有image/file时diagnostics按image记录，与向调用者优先返回file_urls不同。log_clipboard_paste_event虽注释关闭diagnostics无terminal检测，实参构建中无显式gate而直接调用terminal_context().diagnostics_snapshot，不能仅按注释证明零开销。

snapshot/changeCount/support委托client-support，test hook可覆盖；prewarm在supported时通过Once启动后台线程，无join/完成确认，随后同步probe仍需底层处理并发初始化。直接system_clipboard_get_image不使用统一attachment hook，错误降为None。thread-local hook只影响声明的包装入口，不传播到spawn_blocking；set/clear重置计数，未提供RAII恢复。attachments hook在gate之后原样返回(image,file_urls)，不执行真实route的file优先转换；计数仅表示模拟分支调用，不证明实际osascript单次读取。snapshot默认(Some1, image.is_some)，支持None时仍用真实平台support。

已读至测试context builders中byobu_screen构造；主体功能与test-support已读完，剩余测试1391–2331待读。完整9/67 Rust，未运行Cargo。


## clipboard/mod.rs 测试续读（1391–1900行）

已读lone URL/短caption/多行URL/空文本/file URL路由，CRLF/CR与尾部换行匹配、IME中文与不同clipboard不匹配、空payload对无文本、typed读错误和Linux PRIMARY独立/禁用不读。IME测试只覆盖不同文本，不证明相同文本也能判断真实来源。gate矩阵明确Skip恒不探测、FileUrls恒探测，ImageOnly只在支持且快照可用无图时跳过；baseline42/无图None/快照失败SomeNone均覆盖，但静态hook不模拟两次真实读取之间变化。

三个non-mac附件测试安装hook，实际不调用arboard或osascript；注释“非macOS组合”不是平台集成证据。4095/4096、4/5行边界只核对furl helper。route矩阵注入NO_OSC52=false并故意不断言非tmux OSC（ambient依赖）；native恒true、仅tmux buffer、tmux OSC及禁用优先均测试，kill switch也关闭DCS且保留native/tmux。禁用测试末尾从1901继续。

主模块已读1–1900/2331，尚未完整；9/67 Rust完成，无Cargo构建或真实剪贴板操作。


## clipboard/mod.rs 完整阅读收尾（1901–2331行）

DCS表覆盖tmux/embedded Neovim/非tmux/禁用，Byobu-screen测试名含no_osc52但实际只断言native及无tmux，不能据测试名推断OSC关闭。9种反馈逐项断言delivery、文案、diagnostic label、ticks及lead前缀。

文件测试真实写临时目录，验证父目录创建、内容及Unix新文件0600、旧0644收紧到0600；未覆盖symlink、部分写失败、fsync、父目录0700。GROW_COPY_FILE测试手动设置后remove，不保留原值且不保证panic时恢复，未来运行需隔离进程环境。默认路径样例假定home存在。路径缩写覆盖日文但非跨平台所有分隔符。

resolve_delivery矩阵纯注入验证Confirmed+文件成功/失败、Failed+文件成功/失败；Unverified另验证与成功备份组合，toast验证无备份也只显示静态提示，但未直接断言Unverified+备份失败的success。该行为已由实现确认，不能把可取回保证写成绝对。完整模块2331行及所有内联测试已读，尚未执行本包测试。

pager-render现完整10/67 Rust及manifest；无Cargo构建。

- `crates/codegen/pager-render/src/clipboard/mod.rs` SHA256 `f5a6f23060623948b47fa5ae0fc5369d30d0dda7a80712f439570fc1f343a6f3`


## prompt_images.rs 初读（1–450/4404行）

ImageViewerState持原始/显示bytes、mime/dimensions/序号/title/loading/overlay owner和modal状态。open优先内存encoded_bytes否则session_image_path（不读source_path），同步读取/验证尺寸，显示转换失败回退原bytes；path入口按实际bytes识别mime。deferred入口只建空loading状态和source_path，不自行启动线程。take_source_path仅loading时消费一次；apply_loaded直接覆盖数据并置false，无身份/版本或尺寸校验；finish失败已消费路径但loading仍true，调用方处理失败态，不隐式重试。load_image_data同步完整读文件、验证尺寸、准备显示，无本函数文件大小上限；工具validator的真实限制待tools审阅。

PastedImage公开字段含ElementId、显示序号、mime/尺寸/字节数、Arc内存、原source/staging/session路径与preview；字段注释的持久释放和清理尚需后续实现确认。preview_preparation仅pending且内存bytes存在时创建工作，捕获当前graphics protocol与尺寸；无bytes时不从session文件读取。preview_dimensions优先公开dimensions。

PromptImagePreview共享Arc OnceLock，identity独立分配；pending表示未设置，Ready/Failed/Unsupported一次完成，重复set忽略，mark_failed不能覆盖已Ready。Unsupported保留尺寸但非failed且prepared None。Preparation.run是同步方法，名字不强制离开UI线程；优先已有尺寸否则验证，Kitty原格式可用直接复用否则转换失败置Failed，ITerm2复用原bytes，None为Unsupported。相同preview可创建多个工作，首次完成胜出；后续协议变化不自动重置当前OnceLock。

当前10/67 Rust完整，prompt_images已读1–450/4404，未运行Cargo。


## prompt_images.rs 续读（451–880行）

chip文本固定[Image #N]无路径。reconcile按live ElementId保留，其余cleanup后移除；drain清列表不重置counter，clear组合两者。cleanup仅session_image_path None时尝试删除staged_temp_path，删除错误忽略；已session持久化项连staged路径也不删除，不证明自动孤儿回收。

图片扩展名单png/jpg/jpeg/gif/webp/bmp/tiff/tif；read_image_at_path依次扩展、is_file、完整read非空、mime非octet-stream及尺寸验证，保留原路径为source_path，不canonicalize，初始ElementId/序号0。strip_verbatim_prefix实际上无cfg约束，匹配字符串\\?\与UNC前缀即改写，注释“off Windows no-op”不是函数保证，调用点待审。shell_unescape仅Windows盘符/UNC免处理，其他逐反斜杠去转义且尾孤反斜杠保留。token_to_path trim/剥一对匹配引号，仅小写file://走URL本地路径转换，其他PathBuf；~/锚点被认可但本函数不展开tilde，后续顶层是否处理待核。

空格分割只在ASCII空格后新drop anchor处分割，所有分片均有anchor才接受拆分，非通用shell lexer。CRLF/CR归一LF，无CR借用。单drop先检查anchor/file，再拒空或精确/及NUL/CR/LF字节，允许TAB等控制字符；图片分支优先。非图片bare path必须exists，file URL不存在也可NonImage；NonImage才dunce canonicalize并失败回退原路径。因此注释称与image canonicalize一致不符合已读read_image_at_path实现。根路径检查只见精确/，不推导所有Windows根被拒。

主文件已读1–880/4404，完整10/67 Rust，无Cargo构建。


## prompt_images.rs 续读（881–1360行）

批量drop逐行分词、任一token未解析则整批返回空，保留调用者全文回退机会；顶层仍无tilde展开，~/只是锚点并按相对PathBuf尝试。try_read_image_from_path先过滤NonImage再要求图片恰1，因此一图加非图路径仍可Some，不等同整payload仅一个路径。clipboard构造不返回错误，无法解码时dimensions None仍保存bytes/mime。

persist_to_session要求encoded_bytes存在，新建images目录，UUIDv4名字，临时文件write_all+sync_all后rename，失败尝试清tmp，成功才设session路径并清内存；不fsync目录、不显式0600，也不移除原staged_temp。session images/mermaid目录均需session ID并委托client-support session_dir，cwd用lossy字符串。load_for_send优先内存否则session文件，读完整后才检查50,000,000字节上限；已知尺寸任边<8拒绝，尺寸None不在此重验，不验证bytes/mime一致性。

ACP构建先恢复孤儿placeholder，再总剥除placeholder路径；即便allowed_prefixes None，最终也不是保留带路径文本。先一个Text block（可空），再所有可load附图，最后orphan；附图失败仅skip，匹配该display_number的placeholder已被当成有后端，因此可能保留无实际Image的标签。URI优先session路径，source才canonicalize；直接file://加display未URL编码。meta携display_number，不靠位置。

orphan按display_number跳过已附图，每个placeholder独立加载，不去重；只对恢复图片累计预算，saturating_add后严格>cap则break，超额与其后placeholder未加入删除span，最终留下无附件标签。失败加载才记删除区间；成功URI另canonicalize原ph.path。预算不覆盖已有PastedImage总和。末尾从1361继续splice细节；当前主文件1–1360/4404，完整10/67 Rust，无Cargo构建。


## prompt_images.rs 续读（1361–1850行）

collapse_strip_seam删除span后仅扫描邻接ASCII空格，连续多于1才合并（首尾则全删）；单个首尾空格仍保留，不碰tab/CR/LF，也不要求左右都存在空格，注释较实现笼统。ScrollbackImageRef剥verbatim、按扩展/文件/读bytes/可解码校验，再重复尺寸验证；保存原路径不强制absolute/canonical，不提供大小上限。公开字段可人工构造，类型本身不是永久有效文件保证。

markdown图片regex不支持含空格/右括号的path，alt不能含右方括号。extract先全部Markdown后全部bare路径，按原始path字符串去重（失败也登记seen），不是全文出现顺序或canonical同文件去重；bare regex扩展小写且不启用不区分大小写。is_media_only_markdown要求全串仅匹配引用，比较唯一原始路径数与调用者resolved_ref_count，不能单凭数值证明对应身份。

生产逻辑已读完。测试目前覆盖session cwd影响目录、无ID None、跨平台verbatim字符串、chip/mime、临时文件持久化完整字节/无tmp/释放内存、缺bytes失败及保留source。persist测试用假PNG bytes，证明写入不验证图像；readonly测试Unix非root才执行，不覆盖rename/sync故障或崩溃。Windows路径/UNC及shell转义是纯字符串测试，另临时真实PNG路径带空格括号可读取。后续从1851继续其余测试。

当前主文件已读1–1850/4404，完整10/67 Rust；本轮无Cargo构建。


## prompt_images.rs 测试续读（1851–2360行）

临时真实PNG测试覆盖absolute、尾LF/CRLF、单/双引号、file URL及%20、换行/空格/混合批次，断言图片source路径与顺序；批次缺bare路径整批空，非图文件仍保留中间NonImage，正文含路径/! bash前缀不截取图片。多用2x2 PNG，说明拖放识别与load_for_send的8x8发送门禁不是同一步，不能由识别成功推断可发送。

newline_wins测试注释称多行内不再space split，但实际例只有不带新anchor的文件名空格；后续mixed_newline_and_space测试明确每行继续按anchor拆分为4项。以实现及后者为准。引号内仍shell_unescape，非POSIX完整引号语义。file localhost/query/fragment均有成功案例，依赖Url to_file_path忽略query/fragment；URL加caption正文导致整批空，保存全文回退机会。相对路径测试未创建同名文件，单靠该样例不能证明已有相对文件也拒绝，但生产anchor gate已证明。

已读1–2360/4404；file_url_single_slash测试从2361继续。当前完整10/67 Rust，未运行Cargo。


## prompt_images.rs 测试续读（2361–3230行）

图片 source 保留用户可见 symlink 路径（Unix 真链接测试），非图路径 canonicalize；真实临时文件验证 file URL 的 %20/%23/%3F、多字节 UTF-8、大小写十六进制等价和加号保留。%ZZ 测试接受空结果或包含原始 %ZZ 的单个 NonImage，并未验证调用者是否实际执行文本回退，注释中的更强保证不能计入已验证行为。

损坏 PNG、SVG 和 heic/heif/avif/ico 扩展回退 NonImage；后四项使用任意字节，测试证明扩展排除而非有效该格式图像解码能力。目录的 URI 和存在的绝对路径都产生 NonImage；不存在的 URI 仍返回解码路径，缺失 bare 路径返回空。file://、file:///、解码 NUL/CR/LF 被拒绝。尾空白、空行、双空格及混合图片/非图批次有对应样例。

路径加说明文字、单行部分解析失败、跨行 caption 均返回整批空；这里只证明解析器返回值，尚未验证 TUI 插入文本。图片包装器在七种输入上等于 DroppedPath 的 Image 过滤结果。相对文件名测试在 tempdir 创建图片但未切换 cwd，不能单独证明当前目录存在同名图片的情况，拒绝依据仍是已读 anchor gate。部分解析测试显式要求 TMPDIR 无空格；固定 /tmp 缺失路径用例依赖环境，不应描述为全套完全隔离。

已读 1–3230/4404；下一段为 reconcile 测试。完整 Rust 文件仍为 10/67，本轮没有运行 Cargo 或产生构建缓存。


## prompt_images.rs 完整读取（3231–4404行）

reconcile 的 live ID 保留顺序、空集合移除全部；clear 清空并将计数归零。真实临时文件测试覆盖未持久化 staged 文件删除和 session 文件保留，但未覆盖两种路径同时存在或删除失败。persist 成功写入并释放内存、创建多级目录、缺字节失败，原始 source 与 durable session 路径分离；部分样例字节并非有效图片，不能推导持久化阶段验证解码。

Kitty 测试显式运行 preview preparation，验证有效 PNG 就绪及损坏预览失败仍保留发送字节；它不验证后台执行或真实终端显示。load_for_send 样例覆盖内存/会话文件、缺数据、1x1 拒绝，没有精确 8 像素及 50MB 边界测试。内容块保持 Text 首位、可用图片随后、坏图片跳过、display_number metadata；session URI 优先且测试允许路径不存在（由内存供字节），source 仅在 canonicalize 成功后变成 URI。真实 symlink 测试覆盖 wire URI 规范化。

孤立占位符通过显式 allowed prefixes 测试加载及 base64 字节一致性；匹配现有附件不重复恢复。缺失文件移除整个占位符且只收拢删除接缝空格，不影响远处代码缩进/双空格；测试名 with_warn 未捕获日志。无 workspace 仍去掉路径、留下编号锚点，无图片块。聚合上限等于图片字节数允许、少一字节拒绝；第二图超限停止循环，保留去路径锚点。这些断言证明可能存在无图片的锚点，不能描述为所有占位符都已解析；测试未证明已有附件计入预算。

ScrollbackImageRef/提取测试覆盖 PNG/JPEG、损坏/缺失/非图扩展、Markdown 与裸绝对路径、同字符串去重；不覆盖跨语法顺序或 symlink 别名去重。media-only 对纯图片及混合正文有样例。viewer 同步有效 PNG/JPEG 和缺失路径；deferred 初始 loading、成功填充、再次完成幂等、失败 false，但失败测试未断言 loading 清除或可重试。

本文件 4404 行已完整读取；当前完整 Rust 文件 11/67。静态审阅不等于本轮执行测试，尚未完成 pager-render 正式契约映射。

证据 SHA256：`crates/codegen/pager-render/src/prompt_images.rs` = `67480533849b95a93a53e23644125084793856a562bef70980a81c235c376347`。


## terminal 版本与嵌入编辑器（3 文件完整阅读）

term_version.rs：版本依次选择 DA2、环境版本、空字符串/none。环境值 trim 后拒空，TERM_PROGRAM_VERSION 需命名品牌佐证；VSCode 单向允许 Cursor/Windsurf，Zed 不在扩展范围。其次 LC_TERMINAL_VERSION 需 LC_TERMINAL=iterm2（忽略大小写）且 env_brand 为 Iterm2，然后品牌一致的 WEZTERM_VERSION、VTE 品牌 VTE_VERSION。不读取 ZELLIJ_VERSION/KONSOLE_VERSION。版本保存原始 trim 字符串，不校验 semver；DA2 Some 即优先，本 helper 不再校验空值或品牌。XTVERSION 不参与该选择。纯注入环境/优先级/诊断字段测试完整已读；diagnostics_snapshot 测试会读取宿主状态并预热全局缓存，不是全部纯环境隔离。

embedded_editor.rs：非空 NVIM 或 NVIM_LISTEN_ADDRESS 优先 Neovim，再 VIM_TERMINAL，再 INSIDE_EMACS；无标记 None。该分类只取环境证据，无法识别 tmux 与 editor 嵌套方向；注释要求新增标记同步 pager-pty-harness 清理清单。7 个测试覆盖各标记、空值、无标记和 Neovim 优先，不验证真实编辑器或 OSC 交付。

da2.rs：全局 OnceLock 缓存成功或 None；只对 Alacritty、无 CSI 拦截 multiplexer、stdin TTY 探测，Unix 向共享 probe 写 CSI >0c 并按 500ms timeout 读取，非 Unix 缓存 None。raw mode 已开启/EventStream 尚未建立是调用者前置约束，本函数不强制。读取截止谓词需累计缓冲出现 ESC[> 且当前字符 c；超时完全无字节的迟到回复可能进入 composer，不能宣称绝无输入污染。parse_version 取最后 > 后字段，要求 Pp=0/Pc=1、packed 在 1..999999；输出 major/minor/patch 和原 packed。所谓 exact shape 注释比实现严格：parser 本身不要求 CSI 前缀或终止 c，未拒绝额外字段，trim_end_matches 可移除多个 c；完整包络只能部分依赖读取器。该偏差保留为事实，未修改运行时。测试覆盖典型 Alacritty 库版本、前置输入、其他 emulator、损坏值和 DA2/XTVERSION 品牌互斥，不覆盖上述宽松形状、真实 fd 或并发调用（get/set 非原子 get_or_init，不能保证并发只发一次查询）。

当前完整 Rust 文件 14/67；这些模块已静态读完，未运行本包测试。

- `crates/codegen/pager-render/src/terminal/term_version.rs` SHA256 `f7bfd44385cbaf848b897c0bdde7a082dfb34ee70749a45cded6749439e9a777`

- `crates/codegen/pager-render/src/terminal/embedded_editor.rs` SHA256 `6301e3a5d1851a62a580dc0a92aad26050a0a69d9d8eea0502e70d5d2398b3fa`

- `crates/codegen/pager-render/src/terminal/da2.rs` SHA256 `36dbb9e6df32f615a335c03d77fddb76e762eeae61f066cb608fcc94a2bf35c5`


## terminal 探测原语、XTVERSION 与键盘（4 文件完整阅读）

probe.rs：write_query 在共享 stderr 锁内检查实际复制的 render fd 是否 TTY，再 write_all/flush，错误折为 false。Unix read_tty_reply 逐字节读取 stdin，最大 256 字节，任意已读字节返回 Some，静默或首字节前错误 None；普通按键也被消费且不重注入。到期仅已见 ESC 时续读，25ms 静默窗、100ms grace 循环；单次 poll 未裁剪到剩余 grace，不能宣称绝对 100ms 硬墙。主循环 poll EINTR 重新算时间，但 read EINTR 内部重试不重新算 deadline，且不检查 poll revents；没有本文件单测。文件首注释 OSC11-only 已过时，已读 da2.rs 明确调用它。raw mode/独占 stdin 是外部时序约束。

xtversion.rs：Unknown/Kitty/WezTerm/Ghostty/Iterm2/Rio 且无 CSI 拦截 mux 才可发查询，stdin 和 render fd 均须 TTY；Unix 发 CSI >0q 不同步读，其他平台记 Skipped。QUERY_SENT 与 OnceLock 分别记录发送和终局，reply_pending 仅发送成功且未有终局；首次 set 胜出，record_reply 可在未发送时被公开调用，不验证来源。payload 移除所有 control 字符再 trim、空值 NoReply，无长度上限或品牌解析。启动检查不是 compare_exchange，不能保证并发启动只发一次。5 秒过滤和迟到回复行为只是此文件对外部 filter 的描述，需在 pager 核验；无输入时未收到回复可一直 pending。测试为 sanitize、品牌/mux gate 和诊断字段；最后一项写全局 OnceLock，注释明确依赖 nextest 每测试进程隔离，不能把普通 cargo test 共享进程当同等验证。

keyboard.rs：macOS 固定分类，其他 HostOs 全 Unknown；只有 Dropped 触发 rescue，Native/Unrecoverable/Unknown 不触发。Ghostty/Kitty/Foot/Iterm2/VSCode/Cursor/Windsurf/Zed 全 Native；WezTerm Cmd Dropped；Alacritty/Rio Opt Dropped；Warp Cmd/Opt Dropped；Apple Cmd Unrecoverable、Opt/Enter Dropped；其余 Unknown。分类不是运行时按键探测，公开 explicit-host API 可注入；ModifierDelivery 测试构造器仅 test/test-support。测试仅 macOS 编译且是表断言，没有真实按键交付证据。

kitty_keyboard.rs：skip_reason 任意 Some（包括空字符串）返回空 flags；否则 DISAMBIGUATE，只有 DA2 packed <=2401 才去掉 REPORT_EVENT_TYPES，无版本保持两项。此函数不再检查品牌或排除 0，依赖 DA2 上游；多路复用器下未知版本不会触发降级。AtomicU8 保存调用者传入的实际 pushed flags；pushed/release/withheld 三谓词区分协议启用与 release 标志，不能证明真实 release 已到达。take 通过 swap(0) 让并发 teardown 至多一次取得非空记录，不直接发送 pop。测试只覆盖 2401/2402、None 和 skip 优先，未测全局记录或真实终端协商。

当前完整 Rust 文件 18/67；本轮仅静态读取与记录，没有运行 Cargo。

- `crates/codegen/pager-render/src/terminal/probe.rs` SHA256 `02c5287bc62ac9e3d21ba024e5df7e20eff50716cf1172c49a2336776bb46596`

- `crates/codegen/pager-render/src/terminal/xtversion.rs` SHA256 `7aa838e226d47d60bd1375c8c2f0b30c7ebc1c150a7603155d85c8f2610bb8f8`

- `crates/codegen/pager-render/src/terminal/keyboard.rs` SHA256 `257fbb455b5b6932e62d4ec2415c679c53d91e202bec6fab25fc4173a4042cda`

- `crates/codegen/pager-render/src/terminal/kitty_keyboard.rs` SHA256 `15f0325548e8a105361ea4b68b034d6659f41be3be5ea044ea9bb0c7cd0fa651`


## terminal/mod.rs 完整阅读（1093行）

TerminalContext 由环境事实构造，生产入口 OnceLock 首次缓存并仅在 tmux 下查询版本/extended-keys；standalone 入口不查询 tmux。native Windows 仅 effective brand Unknown→WindowsTerminal，env_brand 保留 Unknown；注入环境 builder 不做该宿主修正。公开字段可被外部构造或修改，env_term_version 不随品牌修改重算；diagnostics_snapshot 还混入宿主、全局剪贴板路由、DA2/XTVERSION/pushed flags，不能作为纯 context 序列化。

品牌优先序：CURSOR_TRACE_ID；VSCODE_GIT_ASKPASS_MAIN 按不区分 ASCII 大小写的 cursor/windsurf 子串，否则 VSCode；可识别 TERM_PROGRAM；TERMINAL_EMULATOR 含 jetbrains/jediterm；WEZTERM_VERSION；ITERM_SESSION_ID/PROFILE 或 LC_TERMINAL=iterm2；TERM_SESSION_ID；KITTY_WINDOW_ID/TERM含kitty；ALACRITTY_SOCKET/TERM=alacritty；TERM=rio；TERM精确 foot/foot-extra/foot-direct；TERMINATOR_UUID；VTE_VERSION；WT_SESSION；Unknown。env_get 只拒空不 trim。TERM_PROGRAM 单独 trim、去空格/横线/下划线/点并 ASCII 小写；表中没有 Cursor/Windsurf/Foot 名称映射，不能把任意品牌名当有效 TERM_PROGRAM。official vscode remote 要求 SSH 标记及 askpass 路径中精确 .vscode-server 或 .vscode-server-insiders 组件，不做路径存在或来源真实性验证。

Byobu 标记需能确定后端，未知且无 TMUX/STY 则 None；显式 backend ASCII 小写但不 trim，推断 TMUX 优先 STY。multiplexer 先接受任何已检测 Byobu 后端（含推断，并非仅注释所称显式），再 TMUX、ZELLIJ/SESSION_NAME、STY、HERDR_ENV、三个 CMUX 标记；CMUX_SOCKET 不参与。这是固定环境优先序，不能由注释推导实际嵌套拓扑。CSI 拦截类别 tmux/screen/Zellij/herdr，cmux 不拦截；pane 外部 repaint 判定任意 mux 或 embedded editor。

KKP skip 顺序 VSCode家族、Apple、VTE（包括任意遗留 VTE_VERSION）、WindowsTerminal、JetBrains、Screen、tmux<3.3/未知、tmux>=3.3 且 extended-keys 精确 off、unclassified 且无mux；graphics 仅 tmux skip。ShiftEnter 判定独立：VTE parse<8200/失败不可用，VSCode家族不可用，env_brand unclassified 且无mux不可用，其余 false；VTE>=8200 尽管 KKP skip 仍返回 false，不能把两个 helper 当同义。CtrlDot 跟随 KKP skip；鼠标泄露只分类 Windows+JetBrains。

OSC8 skip 顺序 hostile Apple、unsupported、可解析 VTE<5004、unknown能力、screen、tmux<3.4/未知。上下文 VTE_VERSION 不要求品牌佐证，因此可影响其他品牌能力；term_program_version 也是原始无佐证 fallback，版本阈值 helper 使用它而不是 env_term_version。tmux parser 要求前缀 tmux 空格，minor 取起始数字、允许后缀/忽略更多点段；semver helper 只读前两段 u32、无 trim、不检查其余段。

AltScreen CLI no-alt-screen 最高优先 false，config Always/Never 覆盖自动；Auto 只在 Zellij 或 tmux且control=true 时 false，其余 true。控制模式查询失败默认为 false，不保证不进入 alternate screen。tmux config 路径为展示用波浪号字符串，ByobuTmux 特殊 ~/.byobu/.tmux.conf。外部 terminal/test.rs 尚未审阅，不能将此模块阅读计入测试完成。

当前完整 Rust 文件 19/67；本轮没有构建。

- `crates/codegen/pager-render/src/terminal/mod.rs` SHA256 `c6b6aa3f2486d5ca2864b070ddc3c154c999f10c297e9044e318565baa7eb9cf`


## tmux_probe.rs 与 overlay.rs 完整阅读

tmux 查询固定 argv：-V、show-option -gqv（值）/-gv（支持）、display-message -p #{client_flags}；stdin null，stdout/stderr piped，附加 pager_env 并 detach，没有 env_clear 或 shell 拼接。option 直接独立参数传入，未做名称白名单。主进程 2 秒轮询（15ms sleep），进程组所有权建立失败 kill/wait；超时/等待错误终止组，TERM 后 sleep100ms 再无条件 KILL，child.wait 未另加期限。成功退出也清理后代，再在独立300ms窗口内收取两个管道结果；不能把总调用时间表述为硬性2秒。两个 drain 线程 read_to_end 无字节上限，超时 receiver 丢弃不 join 线程；有时间限制不代表输出内存受限。

Available/Unsupported/Unavailable/Error 四态；into_option 将后三者合并 None。成功值 lossy UTF8+trim 非空才 Available；support 成功退出即支持，失败 stderr 任意一条 trim 后精确 invalid option: NAME 或 unknown option: NAME 才 Unsupported，其他失败 Unavailable。control-mode 是 stdout 子串匹配而非 token 解析，成功空输出 Available(false)。四个 fake runner/argv 测试及一个 Unix 实进程近截止成功测试完整已读；后者替换 PATH，catch_unwind 后恢复，依赖 /usr/bin/perl、sleep 和固定时序，只在相同 serial 锁内互斥。未覆盖大输出、永久挂起、kill失败、管道超时或真实tmux服务器；本轮没有执行。

overlay：owner 为线程局部 Option<u64>，next_owner_id 全局 AtomicU64 fetch_add（没有溢出处理）。构造 Escapes 不提交 owner，as_str/into_string 也不提交；commit 先变更 owner 再返回字节，依赖调用方正确交付。PostFlush write_to 在 write_all 成功后才提交但不 flush；append 合并字节并以最后一个带 ownership 的项为最终 owner，plain 不改变记录。写入部分成功再失败不会更新 owner，不能保证终端已显示状态回滚。Static 只比较 owner_id 是否相同决定 retransmit，不比较图片 bytes、位置或 protocol；volatile 总重传并清空 owner。clear 仅 Kitty 生成删除指令，ITerm2/None 返回 None；reset_owner 只改内存。

centered 先为边框减2，内部小于4列/2行拒绝；fit后居中，坐标加法非饱和，公开 Rect 可构造导致溢出的值。五个测试用仅PNG头字节与字符串匹配，覆盖已commit复用、丢弃构造不改owner、写失败不commit、clear/volatile失效；不证明真实图片解码、终端显示、flush或部分写入行为。

当前完整 Rust 文件 21/67；本轮无 Cargo 构建。

- `crates/codegen/pager-render/src/terminal/tmux_probe.rs` SHA256 `e86cce5262141810c40ed31cf49772f6ae21ff3c68b11625d82a0e2bfdf35114`

- `crates/codegen/pager-render/src/terminal/overlay.rs` SHA256 `ac78541e5ebb33191d827ed7370a3da9cc5085e5533869108c342c3c86f18ce6`


## terminal/image.rs 完整阅读（665行）

协议缓存首次 context 判定，tmux 或 Windows 禁用；其他平台仅 Kitty/Ghostty/WezTerm/Warp 选择 Kitty，iTerm2 实际禁用，虽保留公开 ITerm2 编码 helper。scrollback 更窄只准 Kitty/Ghostty/WezTerm，进程全局 force_off 优先；该开关不改 detect_graphics_protocol，因此不等于禁用所有图片 helper。test/test-support 线程局部 override 绕过品牌缓存且让任何 Kitty override 激活 scrollback；guard drop 清 None，不恢复嵌套旧值。

Kitty 原始格式只认 MIME sniff 为 PNG，prepare 直接 clone 不解码验证；其他格式 macOS 先 sips，失败回 image crate 解码并 RGBA8 Fast/Adaptive PNG。sips 临时路径 pid+纳秒，File::create 非 create_new/0600，写入及 sync 后同步 status 无超时；写/sync/spawn/read 的提前 ? 可遗留临时文件，成功路径删除错误忽略，不验证输出确为 PNG。不能描述为所有失败都会清理或转换耗时受控。

Kitty 基础 render 使用固定 image/placement ID1，a=T；transmit 为 a=t，base64 字符串按4096字节分块，首块全header、后块q=2及m，空输入返回空串；无输入大小上限。place/cropped 原样格式化调用者坐标、尺寸、z、ID，不验证是否已经传输或裁剪范围。delete d=i，ITerm2 使用 OSC1337/BEL 与 base64。overlay 先写1基CUP，Kitty retransmit才检查 PNG并上传，否则只place；ITerm2 retransmit=false仍产生CUP，并非字节级 no-op，注释需按代码收窄。此层仅生成字符串，不验证实际终端写入或图像显示。

inline transmit ITerm2返回Some空串，None协议返回None；place按full_rows计算fit、水平居中，Kitty z=-1并按可见行数/top_crop换算source crop，src_h至少1；不夹紧top_crop至图片高度，u32乘法/坐标u16加法非饱和，极值需单独审计。ITerm2不裁剪，emit=false仍有CUP。fit假设固定cell宽高比0.5，先宽后高、round、不读实际像素；任意图片/边界维度为0则返回max_cols.max(1),max_rows.max(1)，零边界时可超出该边界，不能承诺始终落在bounding box内。

外置 image/tests.rs 尚未读取。当前完整 Rust 文件22/67，未运行本包测试或生成构建缓存。

- `crates/codegen/pager-render/src/terminal/image.rs` SHA256 `72d04f964ec09feb8cd30075cb59c3300c345ef6a40f40537c7abb7bf232e61e`


## terminal/image/tests.rs 完整阅读；terminal/test.rs 1–320

图片协议9项测试：6品牌双Windows状态表；scrollback只比较Kitty/Warp；force_off全局状态手工重置，协议guard仅清线程override，panic可遗留force_off。Kitty小块检查header、5000字节只断言多块，没有重组base64/精确4096分界断言；图片输入有零字节和仅PNG头，不证明解码或终端视觉。真实4x3 JPEG转换后只sniff PNG MIME，没有验证尺寸/像素/ICC，也未区分sips成功或Rust fallback。ITerm2几何字符串、首次upload随后place和200KB载荷后续<200字节有断言；不包含真实终端、裁剪/零尺寸/极值、临时文件错误清理测试。

terminal/test.rs前320行已读：TERM_PROGRAM映射及Otty IME分类、品牌marker、foot精确集合与反例、Terminator环境组合、Windows effective/raw品牌分离、JetBrains鼠标OS矩阵及Byobu显式后端。全部为注入环境/表断言；名为terminator_focus_tracking仅检查品牌与VTE分类，未发focus事件，不能视为真实focus能力验证。第320行仅开始下一项inferred Byobu fixture，后续未读。

当前完整Rust文件23/67；terminal/test.rs仍部分读取1–320/2022。本轮未运行Cargo。

- `crates/codegen/pager-render/src/terminal/image/tests.rs` SHA256 `4ee53b942df59ce04023c9c34423c63ef3d6c5f8409ff4d7b1e46e0bec976b1e`


## terminal/test.rs 续读（321–1000）

Byobu 推断/缺mux返回None、tmux/screen/Zellij/cmux/herdr各环境标记及冲突优先序有注入样例；ZELLIJ=0 仍分类为Zellij，按非空而非布尔值处理。tmux胜herdr样例明确承认真实嵌套与遗留TMUX不可区分。CMUX_SOCKET测试只传空值，不证明非空该变量有效（实现根本不读取）。tmux_meta直接helper只测有值/空map，builder另有非tmux空元数据断言。

standalone样例断言tmux版本和选项为None，没有注入计数runner来断言零子进程，零查询依据来自已读实现。context组合检查品牌、Byobu、mux、展示配置路径；所谓over_ssh版本fallback样例未设置SSH变量，只能证明LC变量fallback，不能证明传输。

AltScreen单测含26个表内案例：Auto对普通/screen/Byobu-screen全屏，Zellij及tmux-control内联；Never各环境内联，Always覆盖自动限制，CLI no-alt覆盖三种config及部分mux。标题all modes×contexts×CLI不是完整笛卡尔积，不把它报告为穷举所有组合或真实屏幕切换。以上都是纯helper检查；没有新执行测试。

terminal/test.rs 已读1–1000/2022，第1000行开始Apple session-id样例未读完。完整Rust仍23/67，本轮无构建缓存。


## terminal/test.rs 完整读取（1001–2022）

补齐IDE品牌优先、official VSCode remote精确目录组件正反例及SSH必需条件；路径仅字符串注入，未验证实际安装或远程链路。ZELLIJ_VERSION单独不识别mux且不归属终端版本，term_version断言仍依赖全局DA2未写。未知Byobu后端回退TMUX/STY、非tmux忽略遗留TMUX_PANE、配置路径及版本parser基础/字母后缀/缺字段样例齐备，没有极值或完整版本语法验证。

KKP矩阵验证品牌理由优先于mux、tmux3.2/未知拒绝与3.3/3.3a/4.0允许、Screen拒绝、Zellij/Herdr可进入后续探测。extended-keys只有精确off拒绝，on/always/空/OFF/None都允许，有直接断言。ShiftEnter单独测试VTE8199/8200边界、缺失/坏版本、VSCode家族、native Windows effective/raw分离、未知加mux；这些是静态策略而非真实协商。已确认现代VTE仍被KKP skip但ShiftEnter返回可用；正识别WindowsTerminal也KKP skip但ShiftEnter可用，文档不能把可用判定等同已协商。CtrlDot按skip理由验证，包含tmux选项on/off。

JetBrains大小写与优先TERM_SESSION_ID、KKP/CtrlDot拒绝均有表断言。repaints_pane_out_of_band测试名per_arm只覆盖Neovim、Tmux/Screen/Zellij及plain，遗漏Cmux/Herdr/Vim/Emacs组合；不应称每个枚举分支均有测试，更不证明真实focus重绘。整个2022行测试已读完，本轮没有执行。

当前完整Rust文件24/67；终端目录全部Rust已完成静态审阅，pager-render其余模块仍待读及正式契约映射。

- `crates/codegen/pager-render/src/terminal/test.rs` SHA256 `4a05a480ff31629a682c6fde147175db502b5f1e1b393b9f91d6151fd750bce4`


## syntax.rs 与 appearance/mod.rs 完整阅读

syntax 使用3个OnceLock：GrowNight/RosePineMoon/OscuraMidnight/Auto共用grow-night，TokyoNight实际独立tokyo-night，GrowDay独立grow-day；文件头称TokyoNight与GrowNight共用的注释已过时。三个include_bytes资源各1311行，当前仅确认路径及TokyoNight头部，资源内容尚需核对，未计入完整证据哈希。

syntect_to_ratatui_fg忽略背景/alpha，只映射前景及bold/italic/underline。native lock下不检测极性：chroma<40返回Reset，其余整数HSV分区为基础Red/Yellow/Green/Cyan/Blue/Magenta（255度起紫），不输出亮色/黑白；非lock走theme quantize。颜色分类不保证所有用户自定义ANSI palette对比度。highlight_line总附加换行给有状态highlighter，逐segment删除尾CR/LF，空segment跳过；无highlighter/错误/全部空时返回原text+fallback。错误后未重建highlighter状态。

6个测试包含灰色/色相样例、锁定路径、无highlighter fallback、Rust行无White；最后一个仅断言非空和非White，fallback也可满足，不证明语法匹配成功。with_native_lock手工恢复全局状态，panic不会执行恢复（mutex只释放），测试隔离边界需保留。

appearance/mod暴露config、watcher、用户显示缓存、permission_cursor、render_mermaid/scroll_mode/text_selection；其头部dev-only描述待config/watcher实现核验，不能先作为生产事实。tab_width为全局AtomicU8默认4，Relaxed读写，不验证0或范围、不持久化、不在setter触发重绘；reload更新为调用者职责。

当前完整Rust文件26/67；剪贴板目录经清单核对只有此前已审阅的mod/trust，没有额外平台文件。后续继续appearance实现及三份嵌入主题资源。本轮无Cargo构建。

- `crates/codegen/pager-render/src/syntax.rs` SHA256 `f59ad45c2c4468091afdf1f63b58666eabd66891634ad845c7378e69f4d67ae2`

- `crates/codegen/pager-render/src/appearance/mod.rs` SHA256 `69b19485fb41efe6e1e279c236b966c539adff402f932ad70e15385f1fe9c3a3`


## appearance 值类型与 watcher 完整读取；cache 1–230

RenderMermaid 默认Auto，精确auto/on/off双向转换，未知None；此值类型本身不执行渲染，文件头关于按钮和OS图片查看行为待render消费者核验。ScrollMode默认Auto，精确auto/wheel/trackpad；不包含实际滚轮时序分类器。TextSelection默认Flash，精确flash/hold/word_select；holds仅Hold/WordSelect，selects_word仅WordSelect，不在本层处理点击或复制。三文件共10个测试覆盖roundtrip/default/无效大小写以及word_select蕴含hold，完整静态已读，未执行。

ConfigWatcher实际与头部dev热重载/prod无文件IO说明不符：start无条件start_static，唯一State是Static；若user_grow_home存在便同步read_to_string pager.toml并解析RawAppearanceConfig，失败全部默默default。没有创建文件、notify监听或热重载任务，没有feature分支；保留watch sender且从不send，因此持有期间changed持续等待。start虽async/Result但当前没有await及显式Err路径。唯一测试读取当前字段，是依赖宿主配置的启动烟测，未证明reload、坏文件fallback或无IO。应以实现为事实规范，不继承过时注释。

cache.rs前230行确认compact默认false、timestamps true、timeline/pageflip默认引用UiConfig常量、combine false、simple true；每项独立线程局部current/loaded，首次load调有效配置helper，set只更新本线程并标loaded，既不落盘也不通知其他线程。后续helper/prime未读，不先宣称所有设置一次统一读盘。声明还包含vim false、thinking/group verbs/prompt suggestions true、selection Flash、speed50范围1..100、scroll Auto/invert false、lines0 sentinel范围1..10，具体夹紧策略待读。

当前完整Rust30/67；cache.rs已读1–230/1048。未构建，未增加Cargo缓存。

- `crates/codegen/pager-render/src/appearance/render_mermaid.rs` SHA256 `c8c1a4eba556549a04872861480230c2a4c3cdee98f563538bf3a92f84b6192d`

- `crates/codegen/pager-render/src/appearance/scroll_mode.rs` SHA256 `905741004e168f5e5f3e9c8c4c61a7acb0d2c1152e8bc24f5f9611614a5f1aeb`

- `crates/codegen/pager-render/src/appearance/text_selection.rs` SHA256 `e03a7d5804a889795f3a500c7886204f9153511e50ec9eff2cf28649b23d27f2`

- `crates/codegen/pager-render/src/appearance/watcher.rs` SHA256 `225b46b1e7e605607ac721816eb4dc0ab832f9e33d52e4d5bb93648797e29703`


## appearance/cache.rs 完整阅读（231–1048）

所有设置各自线程局部current/loaded，无跨线程同步；vim注释process-wide source of truth不成立，只有同一UI线程一致。首次bool/string/u8读取各自调用load_effective_config_disk_only，错误或类型不符用默认，无热更新。selection读取先将整个ui表反序列化UiConfig，其他字段损坏也可能导致selection默认。这里不做磁盘写入；prompt suggestions环境override只在外部消费者，本缓存不处理。

scroll speed环境GROW_SCROLL_SPEED直接parse u8不trim，有效值优先再clamp1..100；超255/负数/带空白无效则回配置，配置先try u8再clamp，故300不是100而是默认50。mode环境trim后canonical，无效回配置（不是直接auto），配置不trim；invert环境trim接受精确1/true/0/false，无效回bool配置。lines环境trim parse u8，0保存sentinel返回None，其余clamp1..10；set_scroll_lines(0)却变Some(1)，setter不能恢复None。这些环境仅首次种子读取，set先标loaded会压过后续环境变化。

prime无条件覆盖UiConfig中的compact/timestamps/timeline/pageflip/combine/simple/selection；其余调用lazy load，已set则保留，未set可能逐项重新读盘，并prime permission_cursor。不是把传入UiConfig所有字段一次性复制，重入可覆盖前组在会话内的更改。render_mermaid解析精确canonical否则Auto。

测试使用fresh thread隔离Cell状态，覆盖多项setter往返、speed/lines夹紧、mermaid字符串种子、三项cache独立、首次读稳定、prime选择字段与word_select。prime_seeds_all_caches实际只断言4个字段，没有计数磁盘读取或验证全部loaded；首次读稳定依赖宿主配置且只比较两次值，不证明热改磁盘后不刷新。未发现环境优先及u8溢出配置的直接测试；此处仅静态阅读，不报告执行通过。

当前完整Rust31/67；本轮未构建。

- `crates/codegen/pager-render/src/appearance/cache.rs` SHA256 `70bbcfba712af7db15745eb0015379b0888c0a1cbf006caff3dca17ffd70b589`


## appearance/permission_cursor.rs 完整阅读（455行）

四值canonical为always_allow_all_sessions/allow_once/allow_command_always/reject，配置trim+ASCII小写，未知和空都映射全局always选中。仅控制高亮，不在本层提交授权。matches_kind让Reject同时匹配RejectOnce/RejectAlways，全局always不匹配kind而按workspace helper身份识别；from_kind未知ACP kind折Reject，调用者需排除全局always及allow-edits-session选项，本setter不强制。

配置缓存与last_used均thread_local，非真正process-wide，无session key或自动session重置。环境GROW_DEFAULT_SELECTED_PERMISSION解析后滤掉AlwaysAllowAllSessions，因此显式always_allow_all_sessions也不能覆盖磁盘的其他值；无效环境回磁盘，磁盘无效/错误回全局always。set配置标loaded但不清sticky、不落盘；prime只触发首次读取。

resolve先确定唯一target：sticky非sentinel优先，否则config；在options中找第一个匹配kind且非全局always；找不到直接回全局always行，再index0。不会在sticky匹配失败时再尝试configured kind。注释“具体target永不落always”过强：没有匹配项时仍回always，空options也返回0（无有效索引）。这些是当前行为，不能将高亮回退写成默认拒绝或已批准。

9项测试完整已读：canonical/显示非空、kind对应与reject合并、fresh-thread sticky、默认按identity定位、sticky跳过同kind全局项、仅RejectAlways、无全局项回0。未测试环境显式always覆盖、缺sticky目标、空options、session切换及真实确认操作。当前完整Rust32/67；未运行Cargo或新增缓存。

- `crates/codegen/pager-render/src/appearance/permission_cursor.rs` SHA256 `b269d4e18c37261570be55b0fbce2d916dde9ac7dbc38b88286f5454cb03cc1a`


## appearance/config.rs 初读（1–620/2445）

确认runtime配置包含animation/prompt/scrollback/todo/turn_status以及timestamps/timeline/plugins/plan-chip/alt-screen/minimal行数与thinking折叠；AppearanceConfig::default实际经Raw默认转换，尚不能把各runtime子类型Default直接视为所有最终配置默认。PromptView独立Default为collapse_unfocused/mouse_hover/show_prefix true、compact false；turn gap true。ScrollbackDisplay独立默认无尾线、glyph collapsed accent、dim0.5、selection split、无border覆盖、展开指示及running true、字符›、selection_buttons false、sticky true、tab4、group_max10。这些字段具体消费者待读。

Layout默认outer_vpad1、两侧与block左右pad2；compact effective vpad0/左右1；validated只把outer左右下限夹1，无上限及其余字段校验。Scrollbar默认启用/gap0/颜色None；total_width启用时u16直接相加，is_outside仅gap_right<outer_hpad_right不看enabled，不能推导所有布局必不溢出。Scroll默认margin0/pagefraction0、Center follow指示、auto_select/overscroll/anchor true、respect_manual_folds false；min_scroll按min(100)整数百分比向下取整，可能仍0。Animation独立默认30fps/wave32/showfps false，tick_interval按max(1)除1000整数毫秒，不是精确30Hz。

Todo默认done/total格式；Edit默认indent/line_summary true、其他背景/行号/默认展开开关false，hunk_separator任意String默认省略号；Prompt block vpad true/Light bg/min_lines2/prefix true。Thinking默认当前theme gray_dim、accent/animate/header true、blend0.7/truncated3，bright/body_dim_italic/expand_hint false；header注释称默认false与代码true冲突。body_dim_italic及collapsed_expand_hint仅runtime字段，尚待Raw核验无TOML键。ToolConfig默认muted/dim true、Diamond，而ToolBullet枚举自身Default None，两者区分。

本文件仅读至620，Raw与转换/持久化/测试尚未读；完整Rust仍32/67。本轮未构建。


## appearance/config.rs 续读（621–1260）

ToolBullet字符固定映射，Circle/Diamond调用glyph fallback，其余字符不统一legacy转换。ListDir独立Default terminal_bg=true，与字段注释false默认冲突；Execute独立默认first2/last3、accent true/当前theme running、Label、muted true，而ExecuteHeaderStyle枚举Default Shell，不能混用。

Raw根有terminal/animation/prompt/scrollback/todo及disable_plugins/show_plan_chip，serde(default)补缺，无deny_unknown_fields；不存在runtime根timestamps/timeline/turn_status的对应直接字段。Terminal alt_screen小写枚举auto/always/never；minimal行数Option默认None、collapse_thinking false。RawPromptView只三个true布尔，无compact键。RawScrollback含layout/scrollbar/scroll/blocks/display。Display Option默认大多Some，包含glyph依赖的collapsed accent、dim0.5、tab4/group10；不是所有Option缺失都保持None。RawLayout及Scrollbar默认与独立runtime相同，颜色用OptionalColor待其反序列化实现。

RawScroll min_page_fraction为u8但注释0..100，当前定义不夹紧，转换待读；FollowIndicator小写none/center默认center。RawToolBullet kebab-case支持none/dot/small-circle/circle/small-triangle/triangle/diamond；RawToolConfig实际默认diamond。RawTodo默认default，其文档称彩色计数与runtime说明done/total不同，最终消费者待核验。RawAnimation fps u8、wave u16、默认30/32，注释1..60范围尚不能当已执行校验。RawEdit line_summary/expanded_by_default默认None，其余背景/缩进等字段与runtime对应，hunk_separator默认Some省略号；RawPrompt block默认Light/vpad true/min2/prefix true。RawThinking默认accent None/enabled true/blend70/truncated3/animate/header true/bright false，没有runtime body_dim_italic及collapsed_expand_hint。

config.rs已读1–1260/2445；RawToolConfig default末尾尚未读，后续继续类型转换与持久化。完整Rust仍32/67，本轮未构建。


## appearance/config.rs 续读（1261–1900）

Raw→runtime确认fps clamp1..60、wave_rows至少1、pagefraction最多100、layout outer左右至少1、execute首尾行至少1、thinking blend最多100后除100/truncated至少1。display dim_accent、字符String、tab/group/minimal行数未夹紧；minimal缺省10/2000，compact固定false、timestamps true、timeline取UiConfig常量、turn_status default、thinking两个runtime-only字段固定false。颜色None对Edit/Scrollbar表示None，对Thinking/Execute回当前theme accent；转换依赖当时主题。

OptionalColor接受RGB三元u8数组或字符串，trim并Unicode小写，none/null→None；default不在命名表，尽管RawThinking注释建议它。RGB保留到to_option才quantize；raw读取不量化。序列化None为none、RGB为数组、Indexed换RGB（失去索引身份），其他Color包括可解析BLACK/WHITE/GRAY写unknown，而unknown无法再解析，不能承诺所有颜色roundtrip。hex接受3/6字节并去重复#，按字节切片，非ASCII恰为3/6字节可触发UTF8边界panic；尚未执行复现，不扩大运行时修改范围。

命名色固定表不随当前theme改变；ERROR/SUCCESS/WARNING与RED/GREEN/YELLOW实际RGB不同，生成模板头却写same-as且多处色值过时。to_toml_with_comments是关联函数，总从Self::default生成，显式materialize Edit展开false与line_summary true；按固定子表列表插入DocumentedFields注释，serialize/parse用expect；最终调用comment_out_values。header声称删除文件会重新生成，但已读watcher不创建，创建路径需外部消费者核验。模板含大量注释不作为实际色值证据。

config.rs已读1–1900/2445；comment_out_values、持久化与测试接续。完整Rust仍32/67，本轮未构建。


## appearance/config.rs 完整阅读（1901–2445）

comment_out_values逐行在非空/非注释/非[开头行前加#，保留section并统一尾换行；只用于默认模板，非通用多行TOML转换器。annotate_table按字段doc前缀注释，不改值。persist_respect_manual_folds先拒绝无user grow home，进程内Mutex覆盖读改写，毒锁恢复；缺文件用空文档，其他读错/坏TOML返回错误。upsert要求scrollback和scroll为普通table，缺失新建、更新单个active布尔键，保留其他可解析字段，不验证整份Raw schema。

写入建父目录、同路径扩展名pid+纳秒临时文件，std::fs::write后Unix尝试复制旧mode（错误忽略），最后rename。非create_new、无fsync/目录sync、无跨进程锁、无失败临时清理；旧路径metadata跟随symlink，而rename替换目录项。不能承诺崩溃持久性、所有权限恢复或跨进程更新不丢失。与主分支异步transcript写入消息无关，本分支未改实现。

测试完整读完：hex/命名/RGB/none基础解析、Edit字段转换、未知invert忽略、respect_manual_folds默认及字符串upsert保留sibling/拒非table、注释模板字段存在、空模板与默认Raw序列化相同、无active值、模板插入active键、alt-screen三值与minimal thinking默认/opt-in。没有真实persist文件IO、权限或失败清理测试。full_config颜色只断言Some，未比较实际量化色；thinking“不允许TOML设置”只查默认false/模板不含键，没有向Raw输入该键（实现未知字段忽略已确认）。没有hex Unicode、named基本色序列化逆向测试，也没有数值夹紧矩阵，不能将阅读现有测试当覆盖这些边界。

当前完整Rust33/67；appearance目录所有Rust已完整静态审阅。本轮未执行Cargo测试。

- `crates/codegen/pager-render/src/appearance/config.rs` SHA256 `f52c7c39b944c660bee40259b4fba045b464d0b57d8d3169f0def4e1da7fde2b`


## theme/color_support.rs 完整读取（513行）

ColorLevel有序None/Basic/Ansi256/TrueColor，显示none/basic/256/truecolor。raw全局OnceLock首次检测，NO_COLOR任意Os值存在即None，否则supports_color stdout结果映射，None结果直接乐观TrueColor（不只非TTY原因）；低于truecolor可按品牌升级。白名单Iterm2/Ghostty/Kitty/WezTerm/Alacritty/Rio/Warp/VSCode/WindowsTerminal/Foot，不含Cursor/Windsurf/Zed；native Windows兜底所有品牌truecolor。get/detect每次在raw上按terminal_native_lock最多Basic，开关不重置缓存。set仅首次成功，因此预先set也可使后续NO_COLOR不再被读取。这里无GROW_FORCE_COLOR_LEVEL解析，名称出现在说明但处理点待theme/cache。

standalone不读stdout缓存，使用当前Unicode env、stderr TTY与控制终端可打开证据（Unix /dev/tty读写，Windows CONIN$读）。NO_COLOR先于无TTY返回Available(None)，否则双TTY证据均无为Unavailable；COLORTERM ASCII小写不trim、truecolor/24bit或品牌推断优先，之后TERM含256color→Ansi256，空/dumb→Unavailable，其余Basic。不是实际颜色查询，并且Windows兜底参与同一helper，Unknown+TERM256测试在Windows可能期待Ansi256但实现升级TrueColor，待跨平台验证，不宣称已失败。

量化TrueColor原样；Ansi256仅RGB→nearest_indexed；Basic RGB先索引再ANSI16、Indexed直接ANSI16、named原样；None全部Reset。前16索引固定映射7和15都White；其余按标准xterm RGB欧氏平方距离找最小，等距保留先遇到项，不读用户palette。12测试为注入诊断与纯转换样例；ANSI16 roundtrip名只测5索引，RGB→256只检查类型，不是全部颜色最邻近验证。未执行。

当前完整Rust34/67；本轮无Cargo构建。

- `crates/codegen/pager-render/src/theme/color_support.rs` SHA256 `055ea13e0e5e69aaea978ff2cd8ee42c5721fbef2ea73767f2f0506950dfa268`


## theme/cache.rs 完整阅读（669行）

CURRENT/LOADED为全局Atomic，首次current_kind直接从disk-only ui.theme种子，可能保留Auto，不在此getter解析为具体主题；native lock则立即返回名义GrowNight而不读盘。set更新CURRENT/LOADED不写盘且不改AUTO_MODE。首次读与set不是一个原子事务，并发种子可能覆盖刚set的值；注释“竞态无害”只讨论重复读取，不能扩展为更新不丢失保证。

terminal_native_lock还更新markdown颜色cap及polarity-safe标志，多个atomic操作并非一致性快照；解锁恢复先前CURRENT。auto mapping用Mutex<Option>缓存，锁内读盘，毒锁恢复；invalidate只清mapping不改当前主题。disk解析过滤Auto映射防递归，公开AutoThemeConfig自身仍可构造Auto。

resolve_initial_theme注释声称GROW_THEME最高优先，但函数没有读取环境，直接resolve_from_config(load_from_disk,true)；no_osc11变体传false。Auto路径设AUTO_MODE=true再检测并映射，无检测结果固定GrowNight（不使用dark override）；显式主题/无配置路径不清已存在AUTO_MODE。resolve_auto仅检测桌面再映射，不设auto标志也不set CURRENT。是否由其他调用者处理环境和标志重置需后续核验。

测试覆盖锁定名义kind/默认palette部分字段/markdown标志/量化cap、mock深浅与失败、custom映射、set/loaded。invalidating测试清后立即重种，不证明磁盘reload；filter测试重写同一filter表达式，非实际配置加载。with_test_env手工reset无panic恢复；pin_theme设置night且尽力首次set truecolor，忽略已有OnceLock错误、不恢复之前主题，不能保证所有宿主状态下准确pin色级。当前完整Rust35/67，未执行Cargo测试。

- `crates/codegen/pager-render/src/theme/cache.rs` SHA256 `22ee0c1b62304340660184dd405bae0c70f23510bbaf12f539493eb0e235fe4b`


## theme/system_appearance.rs 完整阅读（415行）

detect同步调用dark_light::detect，Dark/Light映射，其余Unspecified/Error→None；本文件不直接实现平台API，头部OS细节不当作独立验证。启动fallback在桌面None后调用OSC11，raw模式且EventStream前是外部约束；runtime detect不触及OSC11。test/test-support全局Mutex双层Option mock可指定失败并绕过两条检测链，毒锁恢复。to_theme_kind只按深浅取对应override或GrowNight/GrowDay，不排除公开传入Auto，也不检查主题极性。

SystemAppearanceWatcher start_if_auto(false)直接None，true先同步detect再tokio::spawn；每轮sleep后同步detect，非spawn_blocking、无检测超时，实际周期是sleep+检测耗时。生产5秒，只有cfg(test)为50ms，test-support非test仍5秒。Option变化才send，包括Some→None；仅发送事实，不改主题缓存或auto标志，也不会因全局auto关闭而自行停止。Drop abort任务但不await，不保证中断正在同步执行的桌面API；watch保存最新值不提供每次变化事件历史。

18项测试覆盖映射、mock与清除后真实检测烟测、非auto不启动、初值/失败、Dark→Light、相同值200ms无变更、失败恢复；2秒timeout基于50ms mock轮询。未测实际OS自动切换、慢/挂API、Some→None、丢弃终止或通道合并；手工clear_mock遇panic可遗留，锁仅约束遵循该锁的测试。当前完整Rust36/67；本轮无Cargo构建。

- `crates/codegen/pager-render/src/theme/system_appearance.rs` SHA256 `88bec07e4c20d41a0b7317eb8bd33e0e6ac1050a41e41369e5100591a227bdf4`


## theme/osc11.rs 完整阅读（466行）

detect先stdin TTY，再通过共享render stderr写OSC11查询，然后设置局部termios并读500ms；查询早于raw切换，非Unix仍可能发送查询后read返回None。没有品牌/mux gate和结果缓存。TermiosGuard恢复原snapshot但忽略恢复错误，仅清ICANON/ECHO/ISIG/IEXTEN，保留其他flags及VMIN/VTIME，不等同完整cfmakeraw。fd参数只用于termios操作，实际共享probe始终读stdin，不能把with_fd当任意PTY读接口。启动独占stdin仍为调用者约束。

共享probe返回后要求BEL或ESC反斜线结尾、严格UTF8；parser只找首个rgb:再取三个按斜线/BEL/ESC切开的段，不验证OSC11前缀或多余通道，parser自身不要求终止符。channel trim后u16 hex，长度>2取高字节，否则原值，1位f和3位fff都为15（测试明确固定），非按位宽缩放。亮度用sRGB逆gamma及0.2126/0.7152/0.0722，<0.5 Dark否则Light。

测试覆盖1/2/3/4位、终止符、坏hex/缺通道、黑白灰与gamma两分支、真实stdin非TTY预期None、/dev/null ENOTTY、termios位掩码。非TTY测试依赖运行环境；没有真实响应PTY、termios恢复失败、宽松包络或读取竞态测试。亮度边界注释186约0.497不准确，断言只是186暗/188亮，不等于精确阈值数学证明。当前完整Rust37/67；未运行Cargo。

- `crates/codegen/pager-render/src/theme/osc11.rs` SHA256 `6e08f12185e927799bac9a84a00515ee16c868e7c0f9876aaba3f616c0db4304`


## theme/mod.rs 完整阅读（1176行）

ThemeKind 有五个具体主题和 Auto；ALL 排除 Auto，available 在非 truecolor 时只列 GrowNight/GrowDay。from_name 小写匹配别名但不 trim，FromStr 和 canonical_name 复用它；display_name_for_canonical 对 oscura-midnight 无专门映射，原样返回。Auto“从不存缓存”注释与已读 cache 的公开 set/磁盘加载不符。默认 Theme 是 GrowNight。

quantized 对所有颜色字段调用 quantize_color，保留六级标题 Modifier。current 每次读取检测级别，native lock 时直接 terminal_default 量化；否则由缓存选 palette，Auto 回退 GrowNight，并在量化前判断极性。Windows 对 RGB 结构颜色按通道和比较决定远离基色方向，饱和加减固定幅度；不是实测显示器对比度校准。量化后仅 has_color 且 Basic 或 legacy Windows 非 truecolor 才应用 ANSI16 覆盖。覆盖保留 bg_base（与函数内部“显式固定”注释不符），设置抬高/画布背景、三级边框、两级灰和六组语义色；依赖用户 ANSI palette，不能保证实际视觉对比度。

apply_kind 在 native lock 下返回当前 kind、不改缓存、不发光标序列；否则把需要 truecolor 的三种主题降为 GrowNight，set 内存再 apply_cursor_color，不持久化，不解析 Auto、不改变 auto 标志。current 本身不执行该 clamp，直接缓存写入可绕过。diff_uses_line_fg 仅要求两个 diff 背景均 Reset。ghost_text_style 为 gray_dim 前景加 italic。is_dark 支持 RGB 与标准 indexed 转 RGB，其他命名色/Reset 默认 dark。

apply_cursor_color 将当前 accent_user 经 resolve_to_rgb 转 RGB，Reset 时不发；其他情况锁 stderr 写 OSC12 并 flush，两步错误均忽略，无本地 TTY/品牌/mux 检查或读回。reset_cursor_color 无条件尝试 OSC112 与 flush，同样忽略错误，函数本身不保证退出路径调用。

测试覆盖别名、Auto 列表边界、五主题极性、ANSI16 字段/语义色/边框/滚动条层次、原始 GrowNight Basic 背景塌缩、命名/索引/RGB/Reset 转换。五主题滚动条断言是 RGB 通道和差至少30且方向正确，不是感知亮度或真实终端可读性证明。FromStr 名称矩阵漏 Oscura（from_name 单测有覆盖），没有 apply_kind 全局状态矩阵、OSC输出/失败、Windows boost 或真实终端验证。本轮未运行 Cargo；当前完整 Rust 38/67。

- `crates/codegen/pager-render/src/theme/mod.rs` SHA256 `ea25b542de37057552c9498090493ab060229b2634ff367f50825d75ae08051d`


## 剩余主题模块完整阅读（7个文件，1374行）

md_style.rs：每次 style 调 Theme::current 后新建 MarkdownStyle，无独立缓存。ratatui Reset→无 anstyle 颜色，RGB/Indexed/全部命名 ANSI 一一桥接，不重新量化；Gray 对应 ANSI White，White 对应 BrightWhite。Modifier 仅桥接 bold/italic/underline/dim/hidden/crossed-out，忽略 reverse/blink 等。六级标题内层合并主题颜色和指定效果，外层 dim+hidden；strong/emphasis/strike 内层使用 md_text 加效果，外层 dim+hidden；inline code md_code+bold，blockquote md_muted+dim，链接文本 link_fg+underline，代码语言及表格外层 hidden，math md_text+italic。NO_COLOR 去颜色不等于无 ANSI 效果；此模块不会清除样式 Modifier。3项测试仅 Reset、三个命名色和原生正文/代码背景无显式颜色，不证明完整输出或 quote-bar 消费者一致性。

terminal_default.rs：固定 const palette，所有背景、正文、次级灰、边框和光标 accent_user 都 Reset；状态与路径等用基本命名 ANSI，diff 背景 Reset、删除红/插入绿。标题均 Reset，H1 bold+underline，H2–H6 bold；代码 cyan、链接 blue。构造器无检测/锁/模式切换副作用，最小模式使用由上游控制。7项测试列举颜色槽检查无 RGB/Indexed、透明背景、光标无 RGB、三种有色级别保持命名颜色/None全Reset，以及 muted/dim 使用 DIM、RGB muted 不强制 DIM。字段枚举是手工清单，视觉可读性仍依赖终端调色板与 DIM 支持。

tokyonight.rs 定义公共可直接构造/改字段的 Theme，以及公开 palette 常量；const tokyonight 使用 Storm 主背景(36,40,59)，各语义角色和 Markdown 颜色为静态映射，无运行时读取。fg 直接接受任意颜色不量化；muted/dim 遇对应 gray Reset 时仅添加 DIM，否则显式前景；primary 使用 text_primary，bold 仅添加 bold，link_style 为 link_fg+underline，不输出 OSC8 本身。单项测试只断言主背景和 accent_user 两值；prompt_border RGB(60,75,120) 与旁边 hex 注释不符，依据数值。

GrowNight/GrowDay 是中性深浅灰背景配静态 RGB 强调色；主背景分别(20,20,20)/(238,238,238)，md_text 使用次级文本(200,200,200)/(68,68,68)，H1–H5 bold、H6无额外效果。GrowNight accent_user 实际(200,200,200)，唯一局部测试期望225且被 ignore=known broken 跳过；不能计入通过覆盖，文件旧注释 fg243也不等于当前 text_primary225。GrowDay 无本地测试。

RosePineMoon 主背景(35,33,54)，正文(224,222,244)，H2 bold+underline、H4 bold+italic，其余标题bold；成功/diff插入使用FOAM青色，不是统一绿色。OscuraMidnight 主背景(3,3,4)，正文(228,228,228)，H4 bold+italic，其余标题bold；滚动条track(18,16,28)、thumb(52,48,72)，不同于旧ELEVATED值。两者均为静态const调色板，无本地测试；源码所述设计来源/OKLCH转换是注释背景，不据此证明外部主题或转换程序一致性。五种主题颜色所有字段已逐项读取，构造器不做终端适配，需 current/quantized/apply_kind 的外层路径。

当前完整 Rust 45/67，主题目录 Rust 全部读完；嵌入 tmTheme 资源和 render/gboom 仍待审阅。本轮无 Cargo 构建，以上测试是源码覆盖分析，未新增执行结果。

- `crates/codegen/pager-render/src/theme/md_style.rs` SHA256 `7d7c58f21eccc3b39900bd8a2bbb0d97310034285308c6b930c404536b30ff16`
- `crates/codegen/pager-render/src/theme/terminal_default.rs` SHA256 `324b7a2ef7b4a48fdae1d73220c899b3de1a83ff619b40897b1703e253b6b90b`
- `crates/codegen/pager-render/src/theme/tokyonight.rs` SHA256 `31acb3db31f141fb9bf5be84d757bb11a530a30072fc9a00fe2ccbd083ac01a5`
- `crates/codegen/pager-render/src/theme/grownight.rs` SHA256 `73145b8709bcac3f06540bd924b0e010f7ef7ee974b4364cb32425d546140bd4`
- `crates/codegen/pager-render/src/theme/growday.rs` SHA256 `f9cf97adeb30552310a7a0ff4ae53284b15e5d90895abc79bda6117debaef105`
- `crates/codegen/pager-render/src/theme/rosepine.rs` SHA256 `c3e14d8990bc46345249c663cc41b12c92513ba5474ed9147ca0b752498eb738`
- `crates/codegen/pager-render/src/theme/oscura.rs` SHA256 `8f2d75ddd0195d2413ac913b5e86511360f455c320db69be5f7221e88939300a`


## render 基础抽象与滚动条完整阅读（4文件）

render/mod.rs 公开16个直属模块，并重导出 image/preview overlay、PreviewConfig/PreviewStyle、Renderable、SafeBuf；image_overlay 的子模块另计，当前实际 render Rust 总数18，与 gboom4 合计22，前期“render19”估计不采用。

safe_buf.rs 三个 Buffer 扩展方法检查 y>=area.y、y<bottom、x<right 才委派 set_line/set_span/set_string；不检查 x>=area.x，也不自行裁剪 width/文本或清空旧内容。无测试。本地检查不足以保证任意非零原点 Buffer 横向安全；具体 panic/裁剪行为需依赖实现或复现确认，不能把“panic-free resize”注释当全域保证。

renderable.rs 定义对象安全 render(area,buf) 与 desired_height(width)，没有强制缓存、复杂度或面积交集。RenderableItem 支持 Owned Box / Borrowed 引用，两方法委派，From仅Box；()不画且高0；&str/String/Span/Line一律高1、不按width换行，即空串或width0仍1，实际绘制委派WidgetRef；Option None不画高0、Some委派；Arc<R>要求隐含Sized，Box<R>允许?Sized，均委派。不直接实现Paragraph。9项测试只查高度与两类Item高度委派，没有实际render、零宽、多行或Arc/Box覆盖。

scrollbar.rs 全局Relaxed AtomicBool默认false，setter只控制两个render入口；split_area_for_scrollbar宽<=2保留原区无条，否则预留1列gap+末列track，maybe_split只在total>area.height时分割，不读取隐藏标志，因此隐藏仍可能预留空间。needs_scrollbar仅比较u16长度；is_at_bottom用饱和total-viewport且offset>=即true，不改offset。

scrollbar_click_to_offset：track0→Top，cell0优先Top（track1也如此），cell>=track-1→Bottom，其余使用ScrollMetrics与SUBCELL中心减半thumb再反解Offset(usize)。不检查全局隐藏/overflow，也无鼠标事件注册；头部TODO并不代表不存在点击数学辅助。实际render两个入口都在hidden/None/零宽高/不溢出时no-op，未清旧格。创建CoreBuffer scratch与ScrollBar，随后仅复制第一列：空格保持空格，其余符号一律整块█，丢失库子字符thumb精度，因此不能声称输出保留子字符平滑或click为输出的精确逆变换。调用者负责区域落在buf内，直接索引不做intersection；公开面积宽>1仍只复制首列。

following样式为主题scrollbar_bg/fg，非following为bg_highlight/gray，无本模块混色或DIM计算；styled使用调用方样式、不自动量化。10项测试覆盖分区、溢出、底部、None/无溢出绘制、跟随背景差异和顶底thumb位置/大小；未测click_to_offset、hidden开关、styled、偏移原点/越界面积、子字符还原。following测试依赖全局主题/颜色状态且仅has256时断言背景不同，不证明真实亮度。

当前完整 Rust49/67；本轮未运行Cargo，测试描述为源码覆盖分析。

- `crates/codegen/pager-render/src/render/mod.rs` SHA256 `5da879530904b77792653be045681720bebe6487df7a8feee76042cc0ec16b7d`
- `crates/codegen/pager-render/src/render/safe_buf.rs` SHA256 `394d2f692b29101c006a0c29c709af0b5ae81e534fcd8ff800248a3ac2ee8bc8`
- `crates/codegen/pager-render/src/render/renderable.rs` SHA256 `b8df1b54e1ddd980ee56c0c73288faf8c2c60035acb03d6f66383dcadc8cd2fc`
- `crates/codegen/pager-render/src/render/scrollbar.rs` SHA256 `e2066d411aa940ba620a8eff5bfd863dad60ea5336dd4323f0a53c3335066c24`


## render/color、highlight、gboom_overlay 完整阅读（898行）

color.rs：indexed_to_rgb 对0–15使用固定常见xterm值、16–231使用6级RGB立方、232–255使用8起步每级10灰阶；不查询实际终端palette。nearest_indexed只返回16–255，以通道最近cube及均值最近gray比较平方RGB距离，同距选择cube、cube通道同距取较小档，不用感知亮度。resolve_to_rgb支持所有命名色，Reset→None；混色专用color_to_rgb却只接受RGB/Indexed。blend_channel使用f32线性插值、round再as u8，没有opacity范围/有限性验证，不做gamma线性化。blend_color任一输入命名色/Reset→None；任一Indexed就重新量化到16–255，即使opacity0/1也不保证原始索引身份，均RGB才输出RGB，不读取terminal detect。

blend_line只混各span显式fg，保留span内容和style其他字段、line.style，但重建Line丢失原alignment；with_default对缺fg的span使用传入default_fg，不解析line.style继承fg，混色失败不补default。两者均不混背景，不能按注释“all span colors”理解。fade_region同时混前景背景；blend_area可独立选fg/bg，cell_mut跳过buf外坐标，但area.x+width/y+height使用普通u16加法，不覆盖溢出Rect的安全保证。dim_area先清全部modifier，再只混前景，不改背景；命名色不混但仍丢modifier。18项测试覆盖常规cube/gray抽样、插值、RGB/Indexed/命名色、整区和局部fg/bg；缺Line元数据/继承、dim清modifier、无效opacity、溢出Rect和真实终端palette验证。

highlight.rs：空text直接返回；regex.find_iter产生不重叠匹配，以wrapping辅助把byte范围转换显示列，插入REVERSED而非翻转/删除既有REVERSED。single_row按prefix偏移绘制，不检查skip或viewport_bottom；wrapped使用area.width.saturating_sub(prefix_w)、跳过skip行、y>=bottom停当前匹配。仅右边界判断，直接buf索引，无area/buf交集；usize列转u16及坐标普通加法可能截断/溢出，调用者负责合法窗口。零宽匹配无列不会着色。无本地测试；wrapping辅助的Unicode/零宽行为留到其源码核查，不能先宣称完整grapheme安全。

gboom_overlay.rs：面积宽<30或高<8返回None且不改buf；否则dim整区前景0.5并清modifier，按90%居中、下限30×8且不超过area构造popup，Clear、背景正文色、圆角框，返回整个popup Rect。标题GBOOM使用固定GBOOM_RED RGB+bold，HUD生命>60绿、>30黄、其余红，直接RGB不量化，故不是所有颜色都来自Theme或遵循NO_COLOR。HUD左侧HP与KILLS按字符数截到内部宽，右侧playing给WASD/箭头/SPACE/ESC提示，否则仅ESC；宽度容纳才画提示，不注册按键，也不渲染游戏图像或发送Kitty数据，后者由调用者负责。2项测试仅小区None及正常标题/HUD出现，不验证生命阈值、提示溢出、图片或终端效果。

当前完整Rust52/67。本轮无Cargo构建，测试仅完成源码审查，尚未执行该包测试。

- `crates/codegen/pager-render/src/render/color.rs` SHA256 `e2503529d312b6a6aa4477ab553610b3ba396d2cd43dc145c58c9d43dcd4793b`
- `crates/codegen/pager-render/src/render/highlight.rs` SHA256 `239d079e844cc2311b4d3957d2f488fbddf966941ac5f36f53e02d334a55480d`
- `crates/codegen/pager-render/src/render/gboom_overlay.rs` SHA256 `6d19067eb5c75238439446cd0fdb039f2114f428c4907ddc256b949f59b3342f`


## render/tool_paths 与 terminal_output 完整阅读（986行）

tool_paths.rs：home使用OnceLock<Option<PathBuf>>首次缓存，包括无home。仅原生Path首个Normal组件恰为~时展开，不支持~user；无home则目标None。绝对路径或Windows Prefix直接返回，其他目标有cwd则join、无cwd保留相对，不做存在性/权限检查或canonicalize。非tilde路径保留原拼写，tilde路径经components重建保留ParentDir，但不承诺保留所有重复斜线/CurDir拼写。显示副本才normalize_lexically；Expanded与normalize后的cwd做strip_prefix，非空相对才使用，相等则回退完整display，失败展开则保留原字符串；符号链接不解引用，因此显示词法“内部”不证明实际文件在cwd内。Filesystem target与显示目标不得互换。

Collapsed忽略cwd，使用支持正反斜线的basename并按width-reserved饱和预算truncate；Expanded/Fullscreen均忽略width和reserved，Fullscreen也不保证绝对（无cwd或Windows盘符相对仍相对）。legacy path_for_tool_header width=None返回原path，与Collapsed surface不同。shorten_path先按Unicode显示宽度判断，fish缩目录按/分割且保留文件名；中间终止条件却用String.len字节总和，非ASCII可能过度缩短。仍超宽则从原path查找可容纳的…/后缀，再退truncate_str；反斜线不参与fish缩短，仅basename识别。17个测试定义含平台条件：常规缩短、混合分隔、cwd内外/worktree、tilde失败/父段、Windows盘符相对与Unix真实symlink不解引用。部分home测试无home直接返回；不覆盖实际文件打开、Unicode缩短和链接输出，尚未执行。

terminal_output.rs：每次render_terminal_lines新建vte Parser/TermSink，接受UTF8 str并整段解析，空raw→空Vec；plain使用默认style后拼接换行。行数上限50000、列上限8192，与头部“unbounded”注释不符；每cell储存单char+Style，列每char加1，不按Unicode显示宽度或grapheme，遇宽字/组合字的光标覆盖不等同真实终端。列达上限put忽略，newline把col归0、行封顶后继续覆盖最后一行；稀疏写入补base空格，最多可能分配大规模格子，上游截断是外部约束。

执行LF/VT/FF均换行归零、CR归零、TAB到8倍数停靠（不立即补格）、BS饱和减1。CSI A向上，B最多到已有末行不创建新行，C/G封顶列，D减列；K0截尾/K1覆盖至当前列/K2清整行不移光标；J0截当前尾及后续行，J2/3清全部且光标复位，J1不支持。忽略绝对定位H、保存恢复、OSC等未实现回调；csi_dispatch丢弃intermediates和ignore参数，所以不能把所有私有/异常CSI无条件忽略写成契约。

SGR支持reset、bold/dim/italic/underline/reverse及对应关闭、标准/亮ANSI前背景、39/49恢复base、38/48分号或冒号索引/RGB；输出颜色通过全局quantize，base不主动量化。扩展值u16 as u8截断而非范围拒绝；冒号RGB>=4参数取最后3值，不校验colorspace。分号缺参失败不消费后续组，后续参数仍可能成为独立SGR。隐藏/闪烁/删除线等未处理。finish最多删除一个末尾空Vec，再row_to_line去掉与base完全相同style的尾空格（包括显式输入空格），按相邻相同Style合并span；有不同style的尾空格保留，因此plain也可能受样式和量化影响，不是单纯去转义。仅控制序列可得到空Vec或空行，不能笼统等同str::lines。

20项测试覆盖CR进度覆写、LF/CRLF、上移擦行、TAB、畸形SGR不panic、span分组、重复行数、颜色映射/扩展色、忽略指定DEC/OSC/绝对定位样本及Git Bash/PowerShell输入字符串。它们不是真实Windows终端测试；没有行列上限、宽字符/组合字符、尾空格样式依赖、J全部分支或异常参数范围测试。本轮未运行Cargo；当前完整Rust54/67。

- `crates/codegen/pager-render/src/render/tool_paths.rs` SHA256 `236a206612c5edda47d2e68ea7b02499e942328c2ace34483d47e8de2a0944f4`
- `crates/codegen/pager-render/src/render/terminal_output.rs` SHA256 `69136e3180cfca162d9d30edc7da3c30bf651c60e2e2c70d06cab83b12e996a5`


## render/line_utils.rs 完整阅读（605行）

line_to_static复制Line style/alignment及所有span内容和style，push_owned_lines按序追加、不清out。is_unsafe_display_char为纯分类：char.is_control或061C、200B–200F、202A–202E、2060–206F、FEFF；包括ZWJ/ZWNJ，普通RTL字母允许。函数自身不移除文本，不能由“shared every site”注释证明所有入口都调用。

floor_char_boundary先min(len)后回退到UTF8 char边界，不是grapheme边界。byte_offset_at_width逐UnicodeWidthChar累加（未知宽按0），返回第一超预算字符起点；不解析ANSI，不采用整串emoji宽度。truncate_str宽0空串；先取max_width内前缀，截断且预算>1时删除前缀最后一个char再加省略号，否则预算1只省略号；因此不是按省略号预留一列重新测量。源码推导反例：ab+U+0301+c，预算2，前缀ab+组合重音宽2，删零宽重音再加…得到ab…宽3，超过预算；宽字符被删时也可能浪费一列。此反例尚未调用实际Rust函数复现，保留为待验证边界，不改运行时代码。

truncate_line首先按每span UnicodeWidthStr求和，不超预算原样返回；宽0或实际截断重建Line，丢失line.style/alignment，不能说“All styles preserved”。截断预算预留1，完整span搬入，部分span使用逐char take_width（与前置UnicodeWidthStr对emoji的测量可能不一致），省略号用输出最后span的style，没有输出则default。跨span的grapheme也不合并。

fit_line_to_width使用每span UnicodeWidthStr总和；相等原样，短行加raw空格，长行逐span保持、仅跨界span按graphemes(true)截断，跨界宽字不足空间补raw空格。保留line style/alignment及保留span style，但grapheme只在单span内部识别，不能保证跨span组合不分裂。padding继承Line样式而非最后span样式；width无硬上限，调用者需限制分配。零宽且总宽0时返回原零宽内容，而非无条件空串。

cascade_truncate先算type/activity/meta overhead，<=avail时只按剩余截description；否则description清空，依次给type、activity、meta预算，meta可部分截断，并非注释“meta先整项丢弃”。返回顺序仍type/description/activity/meta；复用truncate_str故继承其组合字符宽度问题。

30项本地测试定义覆盖安全字符集合抽样、普通截断、行span切断、fit补空格/完整emoji跨界/后续span删除、部分样式保留、legacy路径重导出及cascade预算矩阵。没有floor_char_boundary直接测试、跨span组合、truncate_str组合重音、Line style/alignment截断保持或控制串宽度验证。本轮仅源码阅读，未执行Cargo；当前完整Rust55/67。

- `crates/codegen/pager-render/src/render/line_utils.rs` SHA256 `91ec8b5903577e1d8ac7c11f32d8f7bc92ce5092917271ac040a943ca7602a91`


## render/preview_overlay.rs 完整阅读（604行）

PreviewStyle只存调用方bg/text_fg/border_fg，不量化；PreviewConfig默认首尾各3行、宽比0.75、bottom_gap0、最小可用区20×5、无hint，字段均公开，无配置验证。render先content.lines收集全部行（不是仅取首尾的有界迭代），空内容或area低于配置阈值返回None。总行>2N时计划首N+分隔+尾N，否则全部；高度为计划内容行as u16加2后min(area.height)，宽为area.width*f32 ratio as u16，不clamp ratio或确保box_width达到min_width，min仅检查输入area。

按area底部减bottom_gap后饱和减box_height，横向居中；没有将box_area与area/buf交集，不保证非零area.y且gap较大时仍在area内。普通加减、preview_lines*2及u16转换对异常大配置有溢出/截断边界。Clear→背景→圆角框后按inner行数顺序画，文本逐行truncate_str，不换行、不解析Markdown/ANSI，不含滚动状态。高度不足时先消耗首N行，再分隔，再尾N；可能完全不显示尾部。分隔写total-2N more lines，仅计算逻辑省略区，不包含因高度再次裁剪的行数。N0且有文本只计划分隔。函数可能返回Some零宽/小框，不能将Some当完整可见内容证明。

hint画在底框，不增加高度；box.width-6饱和结果<8直接跳过，否则truncate_line，将各span背景改为box背景，两侧加raw样式空格，从x+2写width-4，正常几何下保留角与两侧一横线。重建Line只使用spans，原hint行级style/alignment不保留，即使未截断也如此；span其他样式保留。本文本函数不提供“enter展开”行为，hint只是内容。

13项测试覆盖空/小area、单行高度、首尾分隔/数量、自定义N和ratio、长行省略、hint底框/无高度开销/无hint/窄框截断与跳过；未测异常ratio/gap/N、非零原点、高度不足尾部丢失、hint行级样式或控制字符。测试辅助均假设buf原点0。当前完整Rust56/67，本轮无Cargo构建，未声称测试已执行。

- `crates/codegen/pager-render/src/render/preview_overlay.rs` SHA256 `9458bc0ad2e00be773242b918800d40c2bfcce43b79d8ea0b4d38f3c84035cae`


## render/image_overlay 全部4文件完整阅读（612行）

计划show_pixels仅由protocol.supports_images且preview.prepared存在决定，display_path只取source_path，绝不拿session_image_path或staged_temp_path作用户路径。不是仅encoded_bytes存在就可画像素。公开render返回Option<Escapes>，metadata已成功绘制也返回None；不能把None当完全没画。函数本身不做图片读取/解码/加载调度或终端写入，Escapes需要调用方在buffer flush后发送。

布局宽最少28，pixel高最少8、metadata高最少6；不足提前返回不调dim。像素按preview_dimensions或640×480估算，fit_image_to_cells限制内部宽高，路径预留底行，整个框和像素各自居中；框至少28×8。metadata宽75%再clamp到28..area.width，高固定6贴底。坐标普通u16加法，不单独与buf求交，调用者需合法面积。

通过Theme::current().bg_base而非传入bg淡化整个area前景0.5并清modifier，再Clear和指定bg/text/border圆角框。标题Image编号，可放下时拼MIME/尺寸/大小/文件名meta；判断和title_width使用UTF8字节len（含分隔符），非显示宽度，非ASCII会影响居中/可容纳判断；直接set_span且不先truncate。正常inner>=2时路径画在底行，像素不覆盖footer。metadata显示格式、可选尺寸、状态：failed优先Preview unavailable；pending且协议支持图片才Preview pending；其他显示Size。Paragraph wrap trim=false，可因宽度占多行截断后续字段。short-box正文path fallback按现有最小几何通常不可达。

像素路径先在buffer中心画Loading...，即使preview已ready；再根据placement和prepared字节构造static_image_for_protocol，使用preview.identity作为owner标识。此函数仅返回序列，不提交overlay owner；协议构造失败时仍可能留下Loading而没有像素，不能认定总有metadata回退。

content格式仅精确小写6种image MIME映射大写/品牌名，其余原样；bytes按1024进位显示B/一位KB/MB，超过GiB仍MB。path由display lossy转字符串，truncate_path_for_overlay按char数量保首尾三点，预算<=3直接前缀无省略；不按grapheme或显示列，不剥控制字符。footer随后再次truncate_str按显示预算，可能失去原先保留的尾部。meta文件名也未做unsafe字符分类过滤，此处只记录本地处理边界，不推断上游输入是否已过滤。

9项测试覆盖计划矩阵、source/session路径分离、带路径像素CUP与尺寸序列、无路径、metadata、失败回退、几何/最小尺寸及格式。计划矩阵虽名为pixels×path，其四项输入pixels均true，用protocol区分；pending准备分支没有直接覆盖。像素测试只用PNG签名8字节、mock ready/协议并检查序列文本，不证明真实图片可解码或终端显示；未测write/owner提交、Unicode标题、长路径和异常Rect。本轮未运行Cargo；当前完整Rust60/67。

- `crates/codegen/pager-render/src/render/image_overlay.rs` SHA256 `3c0e2921a9e65592763a3b5fb34157b93e74978f1817e0483661fa2b8ade123e`
- `crates/codegen/pager-render/src/render/image_overlay/content.rs` SHA256 `8fc5d0500fda2c3a72b46f839657c6dc5b2a6754b1e83cde1ced571e916498b7`
- `crates/codegen/pager-render/src/render/image_overlay/geometry.rs` SHA256 `367bd4f9aa96e6c79faf31cbb5fc3b51d9fb50ed132cb4b14bb147ffe41feaa7`
- `crates/codegen/pager-render/src/render/image_overlay/tests.rs` SHA256 `9974326af96f2582d45e86e5ddfc32bc2b684775443b17547883664847975d5e`


## render/draw.rs 完整阅读（771行）

PagerTerminal为ratatui_inline Terminal<CrosstermBackend<TermWriter>>。WriterSync用共享原子queued/written/failed/writer_active和可选tokio无界事件sender；reserve先Release递增，成功write_all+flush后才store written并发Written(seq)，首个failure CAS发Failed一次且状态不可复位。单个sync同一时刻只允许一个TermWriter，但构造器不校验传入tx是否属于同一sync/channel。wait_drained同步每1ms轮询written>=queued且未失败，失败返回泛化错误、超时返回TimedOut不取消写入，可重试；仅覆盖已flush提交数据，未提交的TermWriter.buf不计入，也不禁止后续producer提交。先停输入/提交是上游约束，不能仅凭drain保证独占TTY。

TermWriter write只追加Vec并报告全长，初始容量32KiB不是限制；flush空buf直接Ok，否则先reserve再mem::take和无界std mpsc发送，发送失败丢payload并标记BrokenPipe，成功仅代表入队。discard只清当前buf，不清已排队帧；Drop尽力flush后释放active，即使失败也不重试。未检查sync.failed才允许新写入/构造。

spawn_writer_thread创建无界payload和事件队列，逐条处理，不合并帧/限制积压。GROW_TEST_FRAME_WRITE_DELAY_MS在生产也读取，parse u64无trim且每payload睡眠。非Windows优先dup_tui_stderr，失败退libc::dup(stderr)后直接from_raw_fd，未检查dup=-1，是独立待核验错误路径；Windows用stderr。64KiB BufWriter，每payload持有client_support stderr锁完成write_all和flush，失败记录日志并结束；线程spawn失败expect panic。Written意味着用户态底层flush成功，不是终端已经显示/同步更新生效。

WriterThread显式join需要先drop所有sender，否则rx等待关闭可无限等待；没有join超时，Drop也直接join且忽略错误。线程panic由显式join转换错误，但panic路径不保证mark_failed，wait_drained可超时而非收到Failed。慢PTY或其他持锁者也可能令join不返回，不能将wait_drained的超时泛化到所有teardown。

CursorState初始last_pos=None只是本地假定隐藏；action相同位置无变更→None，有变更且可见→Reposition，显隐转换Show/Hide，位置变更Reposition。apply queue MoveTo/Show/Hide后无论错误都更新本地状态，不flush、不确认实际终端。draw_frame依次queue BeginSync、autoresize、调用render闭包收cursor/postflush和LinkSpan、set_frame_links、flush_with_links、swap_buffers。auto/queue错误忽略，cell flush错误当false但仍swap；任意Some postflush都按会动光标计算。无cell变化/无postflush/无cursor动作则discard当前writer缓冲并返回，可能也丢弃之前尚未提交的外部缓冲内容。其他路径写postflush、cursor、EndSync、backend.flush，错误均忽略；无panic安全同步结束guard。overlay owner可能在写入TermWriter缓冲成功时已提交，非物理PTY确认。

23项测试：固定viewport相同帧第二次无payload；write_payload成功/写失败/flush失败与事件；drain超时重试、send断连、单producer、reserve先可见；光标动作矩阵及本地状态。最后名为writer_thread_preserves_multibyte_utf8的测试自行建Vec<u8>通道线程，未调用生产TermWriter/spawn_writer_thread，只证明标准通道样本字节保留。未覆盖真实PTY背压/无界积压、dup失败、join生命周期、链接变更/postflush错误和apply写失败。本轮未运行Cargo，当前完整Rust61/67。

- `crates/codegen/pager-render/src/render/draw.rs` SHA256 `a859b7b147e3ebcd226ce326cb8696b51f4164aeb68ae4097ae7f16a7fb488d4`


## gboom/mod.rs 完整阅读（727行）

GboomState拥有Game/Renderer/FireSim、Title/Playing/Won/Dead阶段、计时与PNG缓存、鼠标区域；new按kitty_flags_pushed一次设置release-aware，不自行推送键盘协议，也不解析/gboom命令或检查图片协议。注释“不按生产标准维护”不是本审计的范围豁免。

tick取距last_tick的时间min0.1秒、更新时间与phase_time；Title/Won/Dead每tick fire.step一次，不按dt步进，Playing game.step(dt)后死亡优先，否则won且所有imp已Dead才Won；每tick sim_gen递增。因此phase_time是被clamp的累积时间，长暂停不会直接跨越结束宽限，火焰速度仍随tick次数。set_phase归零phase_time和鼠标基线。

handle_key只检查code、不检查kind/modifiers，上游应分发press/repeat与release。Esc/q/Q所有阶段Close且不自行清placement；Title其他任意key仅切Playing，不把该键同时执行运动/射击；Playing W/S或上下前后、A/D平移、左右转向，Space/Enter排队射击，直到tick才改变画面generation；无效键仍Changed。Won/Dead非退出键仅phase_time严格>0.8后Close。handle_release仅Playing且movement code时委派；release_all委派并清鼠标基线，focus事件绑定是上游责任。

鼠标仅Playing响应；区域半开且右/下饱和相加。区域内左键Down排队射击，Moved/任何button Drag按横列差*0.06改角度并递增generation，第一事件只记基线；绝对差>12只重置基线，出区域移动清基线；无垂直瞄准/按modifier限制/角度归一化。set_mouse_region不清旧基线，clear才清；hud只拷贝HP/kills/total/playing，any_movement_held只test或test-support公开。

frame_size_for_cells假定8×16px cell，初始各维至少64，等比缩到480×320内后各维再至少64，极端窄长比例会因第二次floor改变。frame_png本身只拒绝w/h<8，不执行最大480×320限制，任意大尺寸仍进入fb.resize及u32转换；上游需调用尺寸辅助约束资源。缓存键为(sim_gen,w,h)，命中直接返回借用png；否则resize、按phase画场景或火焰文字，PNG RGB8/Fast/Adaptive编码，失败清cached并None。标题/结束提示按phase_time*1.6整数偶数闪烁，结束还需宽限；没有终端写入，PNG发送由调用方完成。

15项测试定义（其中1项ignored人工导出）：标题/退出/宽限、PNG签名和尺寸解码、同generation字节相等/tick变化、尺寸常规上限、胜负转换、鼠标区域/基线/点击/清理。缓存测试只比较bytes，不证明没重编码；未测key kind误分发、极端尺寸或真实终端按键释放。ignored导出把7张图写固定temp子目录，未清理且注释命令仍-p pager；本轮未执行或生成图片。当前完整Rust62/67，game/engine/assets与wrapping/osc8仍待完整审阅，嵌入主题资源另待核查。

- `crates/codegen/pager-render/src/gboom/mod.rs` SHA256 `c57d2448a15c3ade4f56f5e698c4e9faaa8db98bd47437c04a06e0bb79bb3806`


## gboom/game.rs 完整阅读（916行）

固定22×22关卡，1–4墙纹理、P出生、I敌人，其他floor；行宽仅debug_assert，越界cell视为墙1。blocked以radius外接方形覆盖的网格逐格判墙，并非精确圆墙碰撞；player半径0.20、imp0.30。LOS用DDA墙遮挡，近零距离直接true，先跨格再判墙且到目标距离先true，不验证起点/终点所在格；无敌人遮挡。

Game初始HP100、敌人HP30、角0、固定RNG种子。won仅kills==imps.len，不核查死亡状态（外层另等尸体动画），dead为HP<=0；step自身不检查dead/won，也不验证dt有限/正值或上限，0.1限幅来自GboomState。step顺序累计time→player移动与冷却→消费最多一次bool射击→敌人。多次queue_fire合并一次，冷却中请求直接丢弃不延迟；冷却0.32，muzzle0.09，即使未命中也设置。

六方向hold在timer模式press刷新0.16秒，release-aware设Infinity；每step先减dt再读取，release和release_all在两种模式都直接清hold（注释“release-aware only”不作为限制）。切换mode不清既有hold，release_all不立即清速度/已排队射击。对向抵消、平移向量归一防斜走加速，目标3.3tile/s、转向2.2rad/s，指数平滑tau0.08/0.07，然后积分角度与位置；x/y分开碰撞滑墙，只检查终点不扫掠。bob按速度绝对值累计，即便墙阻挡也增长；旋转不归一，血闪每秒衰减1.8。

target_in_crosshair选alive（非Dying/Dead）、前向投影>0、垂距<=0.33、LOS无遮挡中沿射线最近者，同距保留先枚举者，无射程上限。命中扣11+int(rng*7)，致死进入Dying0.55并kills+1，否则Pain0.28；具体随机端点留待XorShift实现核查。

敌人Idle在距离<9且LOS时变Chasing；追踪后不再要求LOS/寻路，距离<0.95且冷却0启动0.38 windup，否则沿玩家方向加近邻分离力后1.55tile/s滑墙。Chasing动画在尝试移动时增加，即使实际被墙挡；邻居仅alive、距离平方(1e-6,0.49)内加0.6分离，顺序更新会影响后续敌人。攻击到时若距离<0.95*1.25就累计7+int(rng*5)伤害，不重新检查LOS，切追踪并设冷却0.95；冷却仅Chasing递减。Pain到期追踪、Dying到期Dead，Dead完全跳过；总伤害最后统一扣HP下限0并血闪1。可穿薄墙近战的条件边界记录为独立审计候选，不混入文档迁移实现。

13项测试覆盖出生点网格连通/地图边框、朝上墙运动、LOS两个样本、射击击杀/冷却、追踪咬人、kills胜利、惯性衰减/目标速度/双键释放/全部释放及重复频率速度近似。没有跨不同dt完整世界等价、碰撞圆角/穿越、LOS角点/端点、穿墙近战、模式切换Infinity和异常dt覆盖；不能用“plays identically at any frame rate”注释推导严格帧率无关。当前完整Rust63/67；未执行Cargo。

- `crates/codegen/pager-render/src/gboom/game.rs` SHA256 `96b2854356ecf912719e4773d879d8f214b69df61497d08a51f12cad61ed4f07`


## gboom/assets.rs 完整阅读（711行）

资源全部由源码计算或char-map常量生成，本模块不读文件/网络；开头“no copyrighted material”仅作者声明，源码审阅不证明版权状态。TEX_SIZE64、GBOOM_RED[235,40,32]、EYE_GLOW[255,216,0]共享给renderer/chrome。Texture row-major64×64，sample位与63循环取样，依赖pixels构造长度。build_textures每次新建brick/stone/tech/hellstone四张，另建floor/ceiling，无全局缓存。纹理由固定hash01、棋格/接缝/噪声和正弦生成；tech“blinking-looking”只是固定亮点，无时间动画；scale通道clamp后转u8，hellstone直接as u8。

XorShift64零seed强制1，三个xor移位12/25/27后wrapping乘常量，next_u32右移33位，故值最多2^31-1；next_f32再右移8除2^24，实际范围[0,0.5)，上限0.5-2^-24，与“Uniform [0,1)”注释不一致。由此game伤害实际pistol11..14、bite7..9（按该算法值域），不是常见由乘7/5预期的11..17/7..11；火焰的随机参数也受该范围影响，具体表现待engine审阅。hash01另为32位hash低16位除65536，确实小于1，与XorShift浮点路径不同。

Sprite::from_art按首行字节宽和行数分配，每个byte查palette，点与未知字符都透明；行宽仅debug_assert。七张imp精灵各16×20（WalkA/B、Attack、Pain、DieA/B、Corpse），枪Idle/Fire24×18，全部图案已逐行读取。sample将u/v乘尺寸转usize再min末索引，越界浮点趋于边缘、没有透明越界返回；空尺寸会减1/索引出错，但现有常量非空。palette共16个可见符号，眼睛颜色与renderer豁免常量一致。

glyph5x7将ASCII转大写，定义A/B/C/D/E/G/H/I/K/L/M/N/O/P/R/S/T/U/V/Y/!/-；其余（包括数字和未列字母）全空，不是完整英文字库。每字形7行低5bit。5项测试覆盖纹理数量尺寸、PRNG零seed非退化及1000次只检查[0,1)、palette颜色被至少一图使用、imp尺寸/角采样与gun宽、五个固定屏幕字符串字符有字形。PRNG测试无法发现分布只覆盖半区；没有统计/黄金序列、纹理像素一致性或sample非法尺寸测试。本轮未执行Cargo；当前完整Rust64/67，剩engine、wrapping、osc8和嵌入tmTheme资源。

- `crates/codegen/pager-render/src/gboom/assets.rs` SHA256 `4c92b01811b12cb32b50a08147e20538f46034c7388219ca36bb2202eb9524a7`


## gboom/engine.rs 完整阅读（728行）

FrameBuffer复用RGB8 pixels、每列墙depth/上下边界、精灵排序scratch和可分离暗角因子；resize同尺寸直接返回不清数据，变化时w*h*3普通乘法分配，无本地上限，put/darken直接索引依赖内部坐标。Renderer每实例一次构建六纹理及两组精灵。

render_game零维返回；顺序墙→地板天花板→敌人→世界暗角→枪/准星→全帧受伤红闪→低血红通道脉冲。muzzle正时世界亮度1.35，否则1；暗角各轴1-0.11*t²，枪不受该暗角但受后续血闪。HP<=25且未死时脉冲实际作用全帧红通道，不是空间vignette（注释有偏差）。

墙逐列DDA最多256步，越界map视墙，默认texture1，深度下限1e-4；按h/depth投影，朝向调整texture列，侧面乘0.72、距离雾1/(1+dist*0.16)。记录wall strip供后续跳过。floor/ceiling按h/2起始扫描、镜像天花板、地平距离分母至少1，世界坐标rem_euclid循环采样，逐列只画墙外部分。无需外部图片资产，不是完整3D深度缓冲。

敌人含尸体按距离平方远到近unstable排序，每帧复用Vec但容量不足仍分配；相机深度<=0.08跳过，世界高0.72投影，按动画选择精灵、行走轻微bob。墙zbuf逐列遮挡整个精灵列，精灵之间仅painter顺序，不写zbuffer。除Corpse外先画椭圆接触阴影，深度检查也是每列墙值；EYE_GLOW精确RGB免距离雾但仍受之后暗角/伤害效果，Pain像素向白混40%。不能说眼睛完全不变暗。

枪高约42%置底中央，按bob偏移，muzzle决定fire/idle；准星由game.target_in_crosshair共用命中规则决定灰或GBOOM_RED并加中心点。文字5×7每char占6*scale（末尾也算tracking），未知字形虽不画仍占宽；按像素边界裁剪，居中文字先画8向描边再正文，scale/尺寸没有泛化溢出校验。

FireSim固定160×84，底行heat36不被step改写，固定独立种子；每步从y1向上写上一行，随机低bit决定衰减0/1、(r>>2)%3左右漂移循环横坐标，目标可被后写覆盖或保留旧值。重要修正：火焰只调用next_u32，不使用上一节有半区问题的next_f32；因此不能把浮点范围错误直接解释为火焰“少一半强度”。draw按frac铺底部并最近邻取样，仅heat>1覆盖；色阶分段黑/红/橙/黄白，无dt参数。

4项测试：正常frame只断言至少一个非零字节及长度，不证明每像素被覆盖；极端尺寸仅1×1/2×2/16×8/639×401不panic；名为zbuffer_occludes_sprites_behind_walls的测试实际比较敌人在玩家背后与前方时像素不同，未放墙前后控制组，不能证明墙遮挡；火焰60步中行热量>0。无像素黄金图/真实终端、阴影/眼睛/低血/资源上限验证。本轮未执行Cargo；当前完整Rust65/67，余wrapping与osc8，嵌入tmTheme资源仍待审阅。

- `crates/codegen/pager-render/src/gboom/engine.rs` SHA256 `63f2bd3c7e4573d86967fa88dd5c1d856dee04521f5a763a414eb95988c7ccfb`


## render/wrapping.rs 完整阅读（1559行）

wrap_ranges_trim调用textwrap，Borrowed用unsafe offset_from原文获得范围，Owned直接panic；可配置算法不等于任意textwrap输出都受支持。RtOptions默认LF/FirstFit/HyphenSplitter/break_words=true、空初始后续indent，提供全部setter。单行flatten记录每span字节范围；先wrap整个flat取第一行，跳过边界ASCII空格，再以subsequent宽度重wrap剩余。两种可用宽都saturating_sub(indent.width).max(1)，所以总width0仍可能输出内容/indent超宽；break_words=false明确允许长词溢出。

切片共享单调span_cursor避免每行从头扫描，要求范围单调且UTF8边界合法；返回借用span，build以indent为Line基础再设original.style，各slice还patch original.style，原始alignment不直接传到最终Line而继承indent。无条件“保留所有样式”需受line级覆盖顺序限制。单行空输入产至少一个空/indent行。首行joiner None，其余Some边界原字符串；第一边界只主动跳ASCII空格，余下空range直接跳过，不可认定任意内嵌硬换行/尾空白可无损重建。多输入行每个首输出None作为硬断行，初始indent只给第一输入，其他输入初始用后续indent；输出全转owned，支持Line引用/可变引用/拥有值、String、str、Cow、Span和Vec<Span>。

表格检测纯字符启发式：首字符2500–257F除│/┃视表；首│只有越过前缀│和空格后再出现│才表；首ASCII |无条件表，+并不识别。表行直接fit_line_to_width单行裁剪/补空格，忽略indent等其他选项。引用前缀仅重复精确│空格序列且有正文，在caller后续indent显示宽0时自动取原prefix span，保留其局部style；>或┃不自动重复，含内部│的引用可能被视表。

wrap_byte_ranges_matching宽0/空text直接全范围，其他同样两阶段FirstFit/HyphenSplitter；注释“single pass”已过时。自动扣引用续行prefix宽但不插入前缀列，不处理table/no-wrap、caller indent或自定义options，不能称所有渲染breakpoint精确一致。byte_range_to_row_cols逐row求交并对row直接切片，调用者需合法范围；byte_offset_to_display_col逐char累计，落在多字节内部会计入整个char，超末尾得到总宽，不按grapheme/emoji整串宽。续行引用的col不含重注入prefix；结合已读paint_match_highlights只加外部prefix_w，引用偏移是否由调用方补偿待pager核查。

wrap_header_hanging保留首span当prefix，余内容统一按width-(extra+prefix)换行，首行只加prefix、不加extra；后续加完整空格indent。正常路径重建content_line丢header line级style/alignment；无span原样，零可用宽/单span回普通wrap。flush先所有行按width-indent换，再只给续行插空格，第一行也用了缩窄宽。indent任意usize可大分配，无上限。

测试覆盖普通/样式/缩进/空格/长词/连字符、joiner硬软断行、emoji、300span单调范围对照、400word顺序与颜色、匹配范围和表格/引用。真实Markdown单链接实际画buffer检查下划线仅Buildkite；多链接测试收集underlined但未最终比较，只按字符是否属于任意label检查，证据较弱。最后名为引用breakpoint匹配的测试只比较行数，不比较范围/显示列；没有零宽、内嵌硬换行、自定义Owned输出panic、line alignment、引用高亮列或header函数专测。当前完整Rust66/67，尚余osc8与主题资源；本轮未执行Cargo。

- `crates/codegen/pager-render/src/render/wrapping.rs` SHA256 `e1fe6b1625b34ac0d52d70afa3554ed3ddcf6650bdbcaa020acd8648d65b056d`


## render/osc8.rs 分段阅读（1–860/2000，未完成）

生产实现已读至扫描结束（约755行），测试只读开头。LinkTarget区分Url/File与Opaque/SelfResolvingPath；URL依Standard+is_safe_to_open后同时给osc8/open target；官方VSCode remote且SelfResolvingPath的File把两者都置None，委托终端自发现。其他File保留app目标，即使from_file_path不能造URL仍Some open target；不检查存在性。resolve_link_open_target默认Opaque，不采用传入presentation。LinkOverlay允许零长范围、倒序debug panic/release丢弃；不自动去重/裁剪，resolved_spans按终端上下文过滤URL并可抑制id，overlaps使用同row严格半开交集。

path_to_file_target实际委派tool_path_file_target(None cwd)，普通相对路径也能得到File，和“relative fail”注释不符；仅生成file URL时可能失败。file_link_presentation要求File、完整绝对/盘符prefix/tilde或有分隔且提供cwd的相对拼写，再与resolve目标Path相等才SelfResolving，无canonicalize/is_file；basename及截断显示通常Opaque。

local_link_to_file_target trim后拒空/#/独立…或...组件、mailto/tel及非file的://；file URL解析转Path后is_file。其他路径先resolve（cwd可None，此时is_file会依进程cwd），anchored直接目标最后查is_file；相对若目标不存在才按media_paths的Path.ends_with组件找唯一候选，重复相同路径也算歧义，最后仍is_file。显式Markdown版本允许anchored/fileURL目标不存在，相对仍复用保守路径。这些文件检查是同步元数据I/O，无快照一致性或缓存；不能称纯字符串解析。

scanner URL→quoted absolute/home→unquoted absolute/home→relative media顺序。URL来自markdown plain_url_ranges且safe，先记范围即使emit拒绝也禁止后续路径重复；path只在emit接受后登记范围。绝对regex要求至少两段/路径，未引号ASCII字符为主，只有末段可内空格且需扩展名；引号允许各段空格且要同闭引号，不解析转义，字符集合也可纳入控制字符。绝对扫描不走local_link存在性门（path_to_file_target无is_file），所以测试“plain-text discovery existence gated”不可推广到整个scanner。相对regex需/与文件扩展名，仅media非空才启用，但resolver先检查进程cwd相对文件，可能优先于media唯一后缀。省略组件/邻接省略与前导ASCII字符检查阻止部分伪片段，不是任意平台路径解析器。

scan_lines适配器按joiner重新拼完整logical text并记录row字节段；不能恢复已截断/修饰前缀，第一row的Some joiner忽略。push_link_segments先全部投影验证，任意段与旧overlay重叠或u16 checked_add失败就整体不插，joiner字节不属任何row。显示宽用UnicodeWidthStr分别测前缀和片段；无终端尺寸裁剪。生成的各段id None，不由此直接写OSC8，交给frame diff。

该文件测试尚未读完，不登记完整hash或增加完成计数；pager-render仍66/67完整Rust。下一段从osc8.rs第861行继续。


## render/osc8.rs 阅读完成（861–2000，合计完整2000行）

剩余测试完整读完：覆盖local relative项目/生成media回退、fileURL空格解码、显式缺失目标、同名后缀歧义、tool path父段与Unix非UTF8 cwd字节保留；官方VSCode remote使用构造TerminalContext矩阵，验证self-resolving委托与opaque本地activation差异，不是真实Remote SSH集成。相对File无法编码URL仍保留open target有明确单测。

扫描测试覆盖多个URL、styled span拼接、hard/soft行joiner、分段长session媒体路径与%二次编码、空格文件名首尾片段、句尾标点、行号冒号、单组件路径排除、URL/path重叠、~边界和已有overlay重叠。u16溢出只测试to_overlay_col辅助，不测试多row任意段失败时整体不插的实际扫描。没有resolved_spans emit_id/extend_from倒序/零宽range、控制字符引号路径、Windows native路径扫描、真实终端点击或输出序列集成测试。tilde测试无可用home可直接return；多数绝对path不存在也可生成链接，明确支持此前记录的scanner与local_link存在性语义区别。

名为local_link_relative_rejects_ambiguous_and_traversal的测试，只证明给定干净media后缀不能匹配../images/1.jpg；实现并未禁ParentDir，若cwd解析后文件存在就接受，不能声称禁止目录遍历或形成路径沙箱。file target是用户操作定位语义，不等于授权边界。

当前所有67/67个Rust文件均完成逐文件阅读，但该crate仍pending：嵌入主题资源、功能delta映射、运行验证与债务汇总尚未完成，不将“读完源码”当整包验收。

- `crates/codegen/pager-render/src/render/osc8.rs` SHA256 `23f56c5935da1ebaa927b24322fa8741f5330005b33c8eff9dcf5783af2a5ee3`


## 三份嵌入语法主题资源核查完成

使用 plistlib 解析 grow-day、grow-night、tokyo-night 的完整 XML，核对全部有序 settings；每份115项（1项全局默认、114项scope规则），三份的名称、scope及顺序完全一致，差异在配色与顶层元数据。规则涵盖注释/文档注释、数值/布尔/null、字符串、变量/参数/属性、关键字/运算符、函数/类型、模板/HTML/Vue、CSS/SCSS、正则、diff、JSON键深度0–8、Markdown、日志token、Apache、预处理器和ENV。scope存在不保证内置syntax_set一定产生对应token，不能把主题选择器清单直接当语言解析支持清单。

全局foreground依次为Day #444444、Night #b2b2b2、Tokyo #a9b1d6；background为#f6f6f6、#0e0e0e、#1a1b26。Night和Tokyo共享大量彩色token，但变量/类名/普通嵌入文本等分别用#c8c8c8与#c0caf5；Day单独使用适合浅底的色值。全局另有caret、invisibles、lineHighlight、带alpha的selection。uuid均为空；Day/Night的license字段为空，Tokyo为Apache-2.0，仅记录文件元数据，不推导项目授权结论。

字体规则包括评论等italic、特定YAML/Python规则清空fontStyle、spread加粗、字符串与继承类清空、Markdown H1–H6加粗、bold/italic/bold italic/underline与引用italic。实际syntect_to_ratatui_fg保留BOLD、ITALIC、UNDERLINE三个modifier，背景不传入ratatui；“foreground-only”不能理解成丢弃字体样式。全局caret/selection等也不是此转换函数的输出。主题资源里的Markdown规则用于该语法高亮链路，不能替代独立md_style/Theme对普通Markdown的样式契约。

重新核对get_syntect：Night/Rose/Oscura/Auto共用Night实例；Tokyo、Day各用独立OnceLock和嵌入资源，不读取运行时主题文件。终端原生锁下current_kind名义上Night，RGB经polarity_safe_syntax_fg映射，chroma<40为Reset，否则6种基础ANSI色；此分支直接返回，未调用常规quantize，不能笼统声称所有语法颜色都经过NO_COLOR量化。该发现仍需与调用方和颜色策略联审，不在文档审计中顺带改实现。

本次完整解析及有序选择器比较通过；不是syntect实际加载或真实终端显示测试。全部67份Rust与3份主题资源已阅读；功能映射、运行验证、债务汇总仍待完成，crate保持pending。

- `crates/codegen/pager-render/assets/grow-day.tmTheme` SHA256 `3523e5971413e29cf0539af1a123236fe17f73e5f63711531961ae05b602583a`
- `crates/codegen/pager-render/assets/grow-night.tmTheme` SHA256 `186c49ec72f42cadd0a93885bc227f2d145f01a9a7a55dedd207c534e45b0144`
- `crates/codegen/pager-render/assets/tokyo-night.tmTheme` SHA256 `6713e3ab9dab57033b806be71122a640e745e5c66fb1445b20909374f7e48c7f`

## 功能契约整理第一批

已建立 `../pager-render-feature-draft.json`，包含12项需求草稿与WHEN/THEN场景，覆盖syntax、host、link opener、glyph及feature入口；逐项核对来源文件和符号存在。草稿尚未覆盖完整crate，不加入feature-map、不生成完整delta、不勾选整包完成。后续继续依据本记录补齐clipboard、prompt_images、terminal、appearance、theme和render等模块，并进行运行验证。当前工作树无target目录，df显示可用64GiB；未启动Cargo构建。

## 功能契约整理：剪贴板

对照既有完整阅读记录并重新检查route、trust、resolve_delivery和attachment gate/route实现，增加14项剪贴板需求，草稿累计26项。明确预期与实际投递、Unverified布尔结果、远程native信任、tmux退出结果、备用文件失败组合、typed读取错误、来源文本比对、附件路由与snapshot baseline、文件URL优先、预热线程和hook作用域。全部新增来源符号存在；未运行真实系统剪贴板操作或Cargo测试。草稿尚不代表整包覆盖，保持pending。

## 功能契约整理：图片附件

根据完整prompt_images阅读记录增加17项需求及场景，草稿累计43项；覆盖延迟viewer、一次完成preview、清理与计数、批次解析、持久化提交、发送限制、ACP顺序、孤立图片预算、占位符剥路径和scrollback引用提取。全部新增来源符号已对照当前文件；未将静态边界描述冒充运行测试。其余terminal、appearance、theme、render等模块仍待映射，整包保持pending。

## 功能契约整理：render第一批

增加17项渲染需求，累计60项草稿，覆盖writer提交与drain、scrollbar布局、VTE网格、颜色与字素、换行特殊路径、链接目标/扫描/跨行投影。依据此前逐文件完整阅读，重新核对来源符号。仍待补齐其他模块及覆盖缺口；不以60项数量认定完整，不勾选整包验收。

## 功能契约整理：外观与主题第一批

增加10项需求，草稿累计70项。重新读取ConfigWatcher完整实现，确认仅静态加载，与旧热加载注释不符；依据既有完整源码审阅记录整理cache、权限光标、主题选择及cursor输出等场景，并核对所有新增来源符号。尚未将这些静态核验当作执行测试，剩余覆盖及整包验收继续进行。

## 功能契约整理：terminal第一批

补入10项终端需求，累计80项草稿。重新核对PostFlush/write_to及alt-screen策略实际实现，结合已完成逐文件阅读整理上下文、scheme、owner提交、图片格式识别、传输和尺寸边界；新增来源符号均存在。尚缺探测/键盘等完整映射和整包运行验证，不宣称整包完成。

## 首批需求进入正式change delta（不是整包验收）

已将80项草稿写入feature-map及client-surfaces/spec.md，仍属于进行中的change，未写主规范。整包status保持pending，源路径映射目前25项（含manifest），另有43份Rust尚未作为直接source登记，其中包括测试及聚合模块；直接路径覆盖只是缺口筛选，不能替代逐功能核对。后续登记须按crate+requirement增量去重或替换pager-render行，不再使用“整包此前不存在”的生成器断言。草稿与已登记80项当前一致；后续新增草稿必须再同步delta。

## 显示辅助模块映射补齐第一批

重新阅读modal_window_state、tool_paths目标/显示/缩短实现及safe_buf完整实现，增加5项需求并同步草稿、feature-map和client-surfaces delta；pager-render累计85项。SafeBuf只核对y及右界，明确不把函数命名推导为全部坐标安全保证。整包仍pending，尚待其他缺口与运行验证。

## 外观值类型与tab宽度映射

完整复核render_mermaid、scroll_mode、text_selection、appearance/mod四份文件，新增4项契约并同步delta；pager-render累计89项，feature-map总计1506项。明确枚举解析与实际输入/渲染消费者分工，未将模块旧热加载注释写为事实。来源及需求名称去重检查通过；整包仍pending。

## 终端版本与启动探测映射

根据完整阅读记录并复核版本来源及DA2/XTVERSION生产入口，新增5项契约，pager-render累计94项、全表1511项。明确环境品牌佐证、DA2优先级、两个探测互斥品牌门禁、外部过滤器回复和空闲pending边界。增量已同步，未执行真实TTY查询，整包仍pending。

## 键盘能力与KKP状态映射

复核keyboard完整文件与kitty_keyboard生产实现，新增5项契约并同步delta。pager-render累计99项、总表1516项。明确macOS分类表、Dropped rescue、2401/2402协商边界、flags记录与真实释放事件区别、原子take防重复消费。未发送真实协议命令，整包仍pending。

## GBOOM功能映射第一批

根据四模块完整阅读记录补入8项契约，覆盖阶段/tick、输入、PNG缓存与尺寸、射击合并、hold、程序资源和渲染顺序。随机浮点半区作为当前实现事实保留，不复制旧注释。pager-render累计107项、总表1524项；已同步delta，仍待覆盖审查与运行验证。

## 当前证据复核与剩余直接映射清单

本次重新核对审计记录中可提取的71项SHA256，均与当前文件一致；107项草稿与feature-map中的pager-render条目完全一致，全部来源符号仍存在。此检查证明证据未漂移，不证明功能穷举或场景测试通过。

下列Rust文件尚未在需求sources中直接登记，继续逐项判定独立契约、聚合导出或测试证据；不能为提高文件覆盖率虚构需求：

- `crates/codegen/pager-render/src/appearance/config.rs`
- `crates/codegen/pager-render/src/lib.rs`
- `crates/codegen/pager-render/src/render/gboom_overlay.rs`
- `crates/codegen/pager-render/src/render/highlight.rs`
- `crates/codegen/pager-render/src/render/image_overlay/content.rs`
- `crates/codegen/pager-render/src/render/image_overlay/geometry.rs`
- `crates/codegen/pager-render/src/render/image_overlay/tests.rs`
- `crates/codegen/pager-render/src/render/image_overlay.rs`
- `crates/codegen/pager-render/src/render/mod.rs`
- `crates/codegen/pager-render/src/render/preview_overlay.rs`
- `crates/codegen/pager-render/src/render/renderable.rs`
- `crates/codegen/pager-render/src/terminal/embedded_editor.rs`
- `crates/codegen/pager-render/src/terminal/image/tests.rs`
- `crates/codegen/pager-render/src/terminal/probe.rs`
- `crates/codegen/pager-render/src/terminal/test.rs`
- `crates/codegen/pager-render/src/terminal/tmux_probe.rs`
- `crates/codegen/pager-render/src/theme/cache.rs`
- `crates/codegen/pager-render/src/theme/color_support.rs`
- `crates/codegen/pager-render/src/theme/growday.rs`
- `crates/codegen/pager-render/src/theme/grownight.rs`
- `crates/codegen/pager-render/src/theme/md_style.rs`
- `crates/codegen/pager-render/src/theme/osc11.rs`
- `crates/codegen/pager-render/src/theme/oscura.rs`
- `crates/codegen/pager-render/src/theme/rosepine.rs`
- `crates/codegen/pager-render/src/theme/terminal_default.rs`
- `crates/codegen/pager-render/src/theme/tokyonight.rs`
- `crates/codegen/pager-render/src/util.rs`

## Renderable与搜索高亮映射

复核标准高度/包装委托及highlight完整实现，增加4项契约，pager-render累计111项、总表1528项。明确单行高度不随width变化、REVERSED插入不切换、single_row缺少wrapped纵向裁剪。已同步规范增量，整包仍未验收。

## 嵌入编辑器映射

完整复核embedded_editor文件，补入环境识别优先级及空值场景，明确无法凭环境判断实际嵌套方向。累计112项pager-render契约、总表1529项；已同步delta。该模块不独立产生日志，诊断字段由TerminalContext消费者提供，未虚构独立诊断输出契约。

## 探测I/O与tmux结果映射

复核probe生产入口与tmux等待/管道/清理实现，增加4项契约，pager-render累计116项、总表1533项。保留局部deadline与整体耗时/内存上限的区别，不把查询函数名bounded当完整资源保证。未启动真实TTY或tmux查询，整包仍pending。

## util契约映射第一批

依完整源码审阅记录补入5项util需求，来源符号复核通过；明确词法路径边界、固定月年、时钟投影、有限实体替换和间隔解析限制。pager-render累计121项、总表1538项，已同步delta，完整功能覆盖与运行验证仍待完成。

## 颜色探测与主题解析映射

复核raw颜色探测与主题初始/运行时解析，新增4项契约，累计125项pager-render需求、总表1542项。明确NO_COLOR首次检测、原生锁cap、独立诊断证据、Auto解析不等于应用；显式set抢先占用OnceLock的边界仍须纳入后续覆盖核对。已同步delta，未运行真实终端探测。

## 颜色显式初始化与量化补齐

复核set/get/quantize_color，新增2项契约明确OnceLock抢先设置可绕过后续环境探测，及RGB/Indexed/命名色转换矩阵。pager-render累计127项，总表1544项；已同步delta，未以静态核验冒充运行测试。

## 图片弹窗计划与布局映射

复核geometry完整实现与render入口/元数据分支，新增3项契约并同步delta，pager-render累计130项、总表1547项。明确prepared门禁、source路径、28列及6/8行最小尺寸，以及None只表示没有像素序列。整包仍pending。

## 图片弹窗内容格式映射

完整复核content.rs，补入元数据格式及字符/显示列两阶段路径裁剪2项契约；pager-render累计132项，总表1549项，已同步delta。无新增运行时代码或构建缓存，整包仍pending。

## 文本preview映射与构建观察

补齐preview默认布局、首尾截取及高度裁剪契约，累计133项pager-render需求、总表1550项。session33350重新轮询确认仍运行，构建日志推进至textwrap依赖；独立target约1.6GiB，磁盘可用60GiB，不重复启动或清理活跃构建。

## Markdown主题桥接契约

依已复核md_style生产实现补入颜色/效果/标题转换契约，累计134项pager-render需求、全表1551项。运行测试明细已持久化到change，后续继续未映射模块及契约覆盖复验。

## 内置palette共享契约

依六份已完整阅读palette文件及本次构造入口/标题字段复核，将固定palette与标题差异合并为一项契约，避免为每份配色文件重复造能力。累计135项pager-render需求，总表1552项；六个来源已登记，不据来源文件覆盖推导所有语义字段皆已逐项映射。

## OSC11行为映射

复核查询、RGB解析、亮度和termios实现，新增3项契约，累计138项pager-render需求、总表1555项。明确fff取高字节15、信封未完全验证和恢复错误忽略；不执行真实TTY探测，整包仍pending。

## 外观配置转换与持久化第一批

复核动画限幅及manual fold写入完整生产路径，补入3项契约。累计141项pager-render需求、总表1558项；明确解析错误、table形状、进程内锁与rename边界。其他配置字段/颜色序列化覆盖仍需补齐，不因config.rs已有source就视作整文件功能映射完成。

## 当前映射一致性复验

141项草稿与feature-map逐对象一致；每项在client-surfaces delta恰有一个需求标题、契约和场景文本均存在，来源符号均在当前源码。持久测试明细1036项，其中1034 passed核对一致。该检查不替代行为覆盖审查。

尚无直接source映射的文件：

- `crates/codegen/pager-render/src/lib.rs`
- `crates/codegen/pager-render/src/render/gboom_overlay.rs`
- `crates/codegen/pager-render/src/render/image_overlay/tests.rs`
- `crates/codegen/pager-render/src/render/mod.rs`
- `crates/codegen/pager-render/src/terminal/image/tests.rs`
- `crates/codegen/pager-render/src/terminal/test.rs`

除上述清单外，appearance/config颜色解析/序列化及完整配置字段、gboom剩余世界行为、主题样式辅助及其他已注明部分映射还需补齐。测试/聚合文件应登记证据关系而非单独虚构功能需求。

## GBOOM chrome与聚合入口收口

复核gboom_overlay生产实现，补入尺寸/HUD/像素发送分工契约，累计142项pager-render需求、全表1559项。lib.rs和render/mod.rs全文复核均为模块声明/重导出，运行行为由已登记模块承担，不额外生成重复需求。后续以证据登记关联这两份聚合文件。三份独立tests文件作为测试证据，不为其创建运行时功能。仍需处理此前明确的内部功能缺口后验收。

## OptionalColor解析与序列化映射

复核OptionalColor完整实现及固定命名表，新增3项契约并同步delta，累计145项pager-render需求、总表1562项。保留BLACK等命名ANSI颜色序列化unknown的实际不对称，Indexed转RGB身份丢失与None/Some Reset区别，未顺带改运行时代码。

## 外观缓存种子与prime补齐

复核scroll speed/mode/prime实现，新增3项契约，累计148项pager-render需求、总表1565项。明确u8解析先于clamp、环境mode无效回配置以及prime两组不同更新语义，避免旧注释简化为全部默认或一次读盘。

## 权限光标回退补齐

复核环境seed和resolve_initial_cursor，新增2项契约，累计150项pager-render需求、总表1567项。明确默认sentinel环境过滤及sticky未匹配时不再尝试配置；空列表返回0不是存在有效选项的保证。已同步delta，整包保持pending。

## 系统外观监听生命周期补齐

复核watcher完整生产实现，新增同步探测/异步轮询/变化发送/Drop契约，累计151项pager-render需求、总表1568项。明确start参数不持续跟踪auto mode及abort不打断同步探测；已同步delta，未扩大既有测试结论。

## 配置字段转换补齐

复核Scroll/Execute/Edit/Thinking转换，增加4项契约，pager-render累计155项、总表1572项。明确局部限幅、主题fallback、可选字段默认及固定false字段，未把raw字段存在等同实际运行启用。已同步delta，整包仍pending。

## GBOOM命中与胜负补齐

复核target_in_crosshair/try_fire/won/dead，新增3项契约，累计158项pager-render需求、总表1575项。明确投影距离选择、同距顺序、即时kills与外层死亡动画完成的区别。已同步delta，未改变游戏实现。

## GBOOM敌人状态映射

复核step_imps及chase_step，补齐敌人六状态转换与聚合伤害契约，累计159项pager-render需求、总表1576项。近战LOS不复查继续作为实际行为及独立债务保留，未实施修复。

## 帧绘制顺序与错误边界

复核draw_frame完整生产实现，新增帧顺序/idle discard/错误忽略契约，累计160项pager-render需求、总表1577项；同步包清单requirements。现有idle测试验证无变化时零字节，不据此推导flush失败恢复。

## 配置转换补核：布局、终端与显示默认值

直接复读 appearance/config.rs 的 LayoutConfig、RawTerminalConfig 与 RawAppearanceConfig 转换，补入 Appearance layout padding and compact projection、Appearance terminal defaults and runtime initialization、Appearance scrollback display optional conversion。区分转换时默认与后续加载/渲染行为；显式零值未校验不等于所有消费方接受零。未修改运行时代码，未新增编译产物。

## 剪贴板写入和 payload 探测补核

直接核对 clipboard/mod.rs 的写入三路径、备份路径及权限实现、bracketed payload 条件。新增 Clipboard write legs execute without success short circuit、Clipboard backup path and write boundary、Bracketed paste attachment probe size and line gates。备份截断与符号链接行为按当前源码记录，不宣称具备原子保存；其他任务报告的修复不作为本工作树已实现证据。本轮只改文档，无构建产物。

## 拖放路径与会话媒体目录补核

复读 prompt_images.rs token_to_path、read_image_at_path、try_read_dropped_path、persist_to_session 和两个 session 目录 helper。新增 Dropped path token decoding boundary、Dropped image and nonimage path distinction、Session media directory identity prerequisite。注意 anchor 接受 ~/ 不代表 tilde 展开；图片保留输入路径而 NonImage 尽量 canonicalize；目录 helper 不执行创建。已有持久化契约继续适用，失败分支会尽力删除 tmp 文件。未执行编译。

## 文件证据去向核对

新增 [文件覆盖表](pager-render-coverage.md)，登记全部 71 个已阅读文件的行为来源、聚合模块、测试或资源角色。本次全清单 738 个已登记文件 SHA-256 与工作树一致。该表是覆盖核对辅助，不替代内部行为验收，pager-render 保持 pending。

## 主题辅助样式补核

复读 tokyonight.rs 中 Theme 的样式 helper 和 theme/mod.rs 的 diff、ghost、polarity helper。补入两个契约并更新文件覆盖表，当前 pager-render 171 条要求。区分 Reset 转 DIM 的 muted/dim 与显式 Reset 加 ITALIC 的 ghost；命名 White 在 is_dark 中仍走默认 true，不把函数名解释成完整颜色分类器。无运行时改动。

## GBOOM 地图与玩家积分补核

复读 game.rs Map 与 step/step_player，补入地图碰撞/视线以及模拟顺序/移动积分两个契约。方框碰撞与仅检查终点按真实实现记录，既有独立债务不在本变更修复；不把注释 circle 理解为精确圆形相交。同步覆盖表，pager-render 当前 173 条要求。
Luna/high子代理完整读取`pager-render` crate 的 Cargo.toml、67个Rust文件和3个.tmTheme资源，共71个文件、37561行Rust；主代理复核现有173条client-surfaces delta与198条精确来源符号，并确认全部已登记文件SHA-256匹配。8个无独立delta的文件仍作为完整审计边界登记，不把模块导出、测试harness或资源XML虚构成额外契约。未运行Cargo、终端、剪贴板、平台探测或外部进程。跨域crate职责、全局OnceLock/环境快照和分散错误语义保留为后续债务。
