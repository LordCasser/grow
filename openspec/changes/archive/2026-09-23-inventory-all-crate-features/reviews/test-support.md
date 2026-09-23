# test-support 逐包核查

已完整读取 Cargo.toml 及下列7份Rust文件，共1588行；其余7份文件尚未完整读取，包保持pending，不计入完成数。此阶段仅静态审阅，未运行动态测试，未产生构建缓存。

## 包和环境入口

library导出ACP typed/raw、headless、mock、进程和sandbox、资源采样；leader与UDS proxy仅unix编译。default与default-bazel均为空feature，本包manifest不据此切换实现。scaled读取GROW_TEST_TIMEOUT_SCALE为正u32，非法/零/缺失退1，Duration普通乘法不保证极值不溢出。

env_parse缺失/非Unicode取默认，parse失败向stderr打印原值并默认。EnvGuard保存OsString原值，set/unset后Drop恢复；使用unsafe全局环境修改，没有内部互斥，调用方串行标注并不自动证明进程其它线程无环境访问。

grow_binary依次使用存在的GROW_BINARY（转绝对路径）、存在的CARGO_BIN_EXE_grow（原路径）、CARGO_TARGET_DIR或workspace/target下debug/grow。fallback不存在则同步cargo build -p cli --bin grow，无--locked，使用pager环境并detach；仅验证路径存在，不校验可执行性/新鲜度。git_workdir委托git sandbox，实际隔离待sandbox模块核对。

## 资源观测

ResourceSnapshot逐项采样本进程，非原子一致快照。Linux RSS读/proc/self/status VmRSS并乘1024，macOS执行ps并解析stdout（不检查exit status），其它平台None。threads/fds仅Linux以目录迭代count计算，其它None；fd计数包含探测本身瞬时FD。capture_rss跳过其它探测。growth_from每项saturating_sub，任一端None传播None；现有1测试验证下降饱和及缺失传播，不证明平台采样正确。

## 命名推理预期与脚本优先级

InferenceEndpoint覆盖chat/completions、responses、messages。分类先非空trim后x-grow-turn-idx为foreground，再非空x-grow-req-id为auxiliary，否则tools数组至少2项才foreground。请求指纹包含endpoint、kind、trim request-id及JSON序列化body；无request-id不建立复用条目。

response_override优先claim expectation，再路径FIFO脚本，最后required-token鉴权。预期从pending中取首个endpoint+kind匹配项，可跳过不匹配项；FIFO只按路径，不区分前台/辅助，辅助调用仍可消费兼容脚本。脚本/预期覆盖认证；fallback只认精确Bearer或bearer前缀及匹配token，不认x-api-key，虽然401文案提及它。

重复指纹且active>0复用response并增加claim，角色Replay；primary被移出pending，Received。注册校验status/headers，名称仅对当前pending/inflight检查唯一，不能推断生命周期全局唯一。watch状态Pending/Received/Blocked/Satisfied；wait_received也接受后续状态，wait_blocked也接受Satisfied。handle Drop只release，不自动assert_satisfied；wait没有内置timeout。

block_before_terminal为true时等待release（可提前release）；仅primary写Blocked。跨terminal的primary才能使预期满足；有指纹时还需所有active lease结束，无指纹primary跨边界直接Satisfied。Replay跨terminal不能替代primary成功；取消会Drop lease、减active，不自动重新排队预期。Satisfied表示本地pipeline越过屏障，不证明客户端接收或消费HTTP/SSE。global hold只在相应terminal wait检查，不保证所有响应类型一律受控：命名预期用自身barrier，FIFO仅foreground SSE接global，fallback接入待mock_server核对。

CompletionGate用AtomicBool和Notify循环，后续需结合并发测试核对注册/唤醒时序；不从注释推导无丢唤醒。

## 脚本响应

ScriptedBody为JSON、SSE事件列表、Raw(String)，Raw不是任意非UTF8字节。构造器不校验，enqueue/register处validate只校验status/header，未提前校验SSE事件内容。自定义header通过insert依次覆盖同名以及响应默认header。

JSON/Raw有terminal wait时在返回响应前等待；SSE逐事件先delay，最后一个事件前wait，不按协议事件类型识别terminal；空SSE有wait则插入无事件占位执行barrier。SSE启用默认keep-alive；流未被poll/取消不保证到terminal。JSON/Raw不使用逐事件delay。事件内容委托axum编码，不作为原始SSE线格式透传。

## 连接计数server

绑定127.0.0.1:0返回/v1 base及共享accept计数/header日志。每连接循环读CRLFCRLF，再按第一个可解析Content-Length消费body，保留pipeline剩余数据，每请求固定200 JSON {}。不实现chunked、通用HTTP解析、body/header大小或读取超时上限；无显式shutdown handle，listener及连接任务交给runtime生命周期。日志只保存header，body被丢弃。

## headless执行

run_headless按给定args直接调用grow，不额外注入-p；创建带mock_url sandbox并采用调用者cwd。自定义命令支持owned/borrowed sandbox及显式env覆盖；实际env清理由TestProcess负责，现有测试断言命令级ambient被清除且override获胜，仍须读取实现。

TestProcess配置stdout/stderr为Piped，两个异步read_to_end收集完整Vec，headless输出并非总量有界。进程wait期限60秒乘scale，到期kill并timed_out=true，错误panic含diagnostic。进程结束后分别限2秒drain；读错误/task失败/超时返回传入时刻的tail快照，超时abort并await读任务。elapsed含spawn/wait/drain但不含外层grow_binary解析和sandbox构建；两个drain顺序执行。

stderr_tail以len.saturating_sub(max_chars)字节偏移slice，不是字符截取，可能切进UTF8导致panic。assert_headless_success仅检查非timeout和exit success；assert_no_crashes只做六种关键词大小写无关子串检查，不能证明无崩溃。3个unix异步测试覆盖borrowed artifact保留、drain超时返回partial、显式env覆盖；未执行。

## 本批读取证据

- `crates/codegen/test-support/src/lib.rs` SHA256 `dfb7bcd7247590f30822b5b64c0fcd8dd6830db18e6360452d638ea54ea66a2d`
- `crates/codegen/test-support/src/env.rs` SHA256 `2d91c00920efa0694e771d378f584bf1982c29c252857305f05f4a16a152d132`
- `crates/codegen/test-support/src/resources.rs` SHA256 `849c39f58fb2821e2cb25df9258c2e890a1483ea6b0fd350779cab77817391b2`
- `crates/codegen/test-support/src/inference_override.rs` SHA256 `bf40662435f6318e5a3316acfc5d7a2d73b8875176ee816b9c90c0e50cc9359a`
- `crates/codegen/test-support/src/scripted.rs` SHA256 `91a12da836798e80636859b9b1a4d77a5f1c160ce811e7efd66c3f3111851566`
- `crates/codegen/test-support/src/counting_server.rs` SHA256 `c08a388229a0fcecb694d1fde827b74c14d2d2aa96b2e44569896b39ec5eb137`
- `crates/codegen/test-support/src/headless.rs` SHA256 `7298a5e7dadaf0b5bb6f2b1dc8a5a919653c0f62244b029d98c10541dc1a0e0b`
- `crates/codegen/test-support/Cargo.toml` SHA256 `8812475cb4e41eb7f963675a5e980597d7bbdd14a3e08ceea8a4f131cf6e8562`

## process.rs 完整读取

1254行含全部测试已读。默认stdin Null，stdout/stderr Capture；tail64KiB、grace500ms、kill wait5s，tail_bytes最少1。两种输出都使用pipe；Capture后台8KiB块读取，Piped只有调用方读取时更新tail，未读取可能阻塞子进程。tail字节有界、计数饱和、UTF8 lossy展示；单次append长度恰等capacity也置truncated，即便尚未丢字节。read_error保留文本，poisoned mutex恢复内部值。

spawn先sandbox.env_clear，再pager_env，再config.env；原command环境被清除。stdin/stdout/stderr及kill_on_drop由本层重设，detach委托tty_utils。创建process group后spawn并attach；Unix attach失败直接kill并最多250ms同步try_wait后报错，Windows失败保存diagnostic继续best-effort。Windows post-spawn enrollment不能保证极短命后代未逃逸。TestProcessTree attach宽松返回不可用对象，try_attach传播错误；不负责concrete child reaping，release丢弃group句柄，Drop对仍持有group尝试kill。

state/status仅反映缓存，不主动查询系统；is_running调用try_wait。Unix先waitid(WEXITED|WNOHANG|WNOWAIT)观测直接子进程，趁leader僵尸保留PGID时kill group，随后Tokio try_wait收割。PID零或超过i32上限拒绝；ECHILD原样返回，不当已退出成功。Windows先reap再group清理。清理错误保存diagnostic仍可record_reaped并release；不保证所有后代均死亡。missing process错误仅ESRCH/ECHILD（Windows1168）忽略。

wait_with_deadline每10ms以内poll，超时返回None并保留所有权，不隐式kill；捕获任务收尾每路额外最多1秒，故总时长不是严格deadline上界。start_terminate有group且unix时发TERM不等待，否则hard kill；close先查询退出，否则TERM、grace、hard升级；kill立即group kill加direct start_kill双路径，wait超时返回TimedOut并保留owner。termination_reason记录发起原因，不等于OS退出信号；自然退出填NaturalExit。

Drop未缓存退出时先group/direct kill，最多250ms同步yield循环观测/收割，失败或超时不保证已reap；随后abort内部capture任务不await。外部take出的reader任务不由本对象保存。finish_capture_task只对timeout abort，已完成task的JoinError未记录。退出正常但残余后代持有输出pipe时也可走drain超时。

diagnostic_summary仅输出label/tree/status/tail/lifecycle errors，不列cmd参数或env；公开tail本身是原始文本。redactions在spawn从sandbox抓取，不含config.env新增的值；长度>=4的已知值逐项replace，再按authorization/api_key/apikey/password/passwd/secret/bearer/token/cookie/credential等关键词整行隐藏。不是任意输出的完整秘密识别器，label、read_error和lifecycle_errors未经过同一sanitize。相关债务须单独评估，不升级为安全保证。

测试完整读取：Unix PID非法、WNOWAIT保留status、自然退出tail、超时后杀孙进程、TERM正常/忽略升级、有界tail脱敏、Drop回收；Windows Job测试attach失败会自行cleanup并提前返回，不能把其通过视为已验证Job后代回收。本轮未执行测试。

## sandbox.rs 完整读取

908行含全部测试已读。TempDir拥有home、home/.grow、workspace、tmp，构造不修改进程全局env；set/extend/remove只修改BTreeMap，env返回有序副本。std/Tokio apply先env_clear；PTY CommandBuilder只合并，必须由调用方先clear。不是OS文件/网络隔离沙箱。

baseline仅保留平台allowlist：非Windows PATH/LANG/LC_ALL/DYLD_LIBRARY_PATH/LD_LIBRARY_PATH/GIT_BIN_PATH/SHELL，Windows PATH/PATHEXT/SystemRoot/WINDIR/ComSpec/NUMBER_OF_PROCESSORS/GIT_BIN_PATH。Unix无SHELL补/bin/sh；HOME/USERPROFILE/GROW_HOME/TMPDIR/TMP/TEMP指向隔离目录。设置反馈/诊断/自更新/提示建议开关、loopback NO_PROXY、Git禁全局系统配置及交互、LFS smudge禁用、pager。环境开关依赖被测程序遵守，不保证无网络请求；明确未设置web fetch禁用。

apply_mock_url只写CHAT_PROXY/INFERENCE/MODELS/FEEDBACK/CONVERSATIONS五个base URL及测试key，不校验loopback或URL，不额外配置trace/web端点；builder注释范围大于实现，契约按实际五项。set_mock_url可后改。

GIT_BIN_PATH相对路径按构造时parent cwd绝对化，目录前置PATH并作为GIT_EXEC_PATH，join_paths失败退仅parent。git_command用选定binary、清空env、detach、stdin null、pager与auth suppression、--no-optional-locks；构建git fixture依次init/config identity/write README/add/commit no-gpg-sign，每条检查成功但无期限。默认对象Drop依赖TempDir清理，不延长借用它的TestProcess的文件所有权。

sandbox diagnostics打印root及所有路径；secret key按非alnum分段/若干后缀启发式遮罩；endpoint key含URL/ENDPOINT/PROXY，仅合法http(s)/ws(s) loopback保留，删userinfo/query/fragment但保留path，其它整体遮罩。不能宣称路径中的秘密也被去除。diagnostic_redactions另含home/temp/gitconfig等value供进程输出过滤；未知key敏感值不自动识别。

全部测试覆盖独立路径、git clean fixture、相对Git解析/无Git环境、无global mutation、baseline与shell、env覆盖删除、跨平台home/temp、Windowsallowlist、key启发式和loopback URL脱敏；只静态读取，尚未执行。

## 本批追加读取证据

- `crates/codegen/test-support/src/process.rs` SHA256 `1e436410fc0e4cfa7a5ac1ba4938d0a9138533329ffa94b1ba3e434341264654`
- `crates/codegen/test-support/src/sandbox.rs` SHA256 `f3d193bd579adde1eee152b488af5f0bf830cfc90845f063b0fd0029729bc8c9`

累计9/14份Rust文件完整读取（3750/8844行）；尚余acp_client、leader、mock_server、sse、uds_proxy，包仍pending。

## acp_client.rs 完整读取

577行完整读取，无本文件内单测。spawn_agent_process先改sandbox mock_url，再extra_env覆盖，leading_args在agent stdio之前，cwd取调用者值。stdin/stdout Piped，stderr默认Capture；持有sandbox延长目录生命。typed client用LineBufferedRead和connect_client_v1，两处spawn_local要求调用方LocalSet，本对象不持有IO任务join handle。

initialize不是spawn自动执行；明确请求V1、默认filesystem capability、terminal=false，startupHints nonInteractive/skipGitStatus/skipProjectLayout及test客户端标记。仅接受auth method provider.api_key，缺失panic；authenticate携带headless meta。create_session默认无MCP，model版本用meta.modelId；set_model用稳定config option key=model及value_id。prompt仅发送一个text content；ext_method传JSON raw params，无本层timeout。没有独立cancel辅助入口，不从协议类型推导本wrapper提供取消流程。

permission callback优先AllowOnce，否则选首项（可能并非允许），空options返回Cancelled，不能笼统宣称总批准。session notification无session过滤，所有通知累加u32，仅非空AgentMessageChunk Text加入全局chunks，captured_text全量join无清空/容量限制。扩展通知仅grow/session_notification和grow/session/update解析JSON保留，其它忽略；grow_notifications clone全部历史。

默认initialize/create/set_model本身无timeout；对应wrapper20秒乘scale，prompt30秒，load60秒；超时panic并丢弃future，不在此发送cancel或立即close子进程。initialize/auth/create/load错误panic，prompt/set_model/ext传播ACP Result。close/start terminate/kill委托TestProcess；take_sandbox仅一次，取出后由调用者负责继续保有，后续sandbox()再调用panic。stderr返回原始有界tail，timeout诊断未走process sanitizer。

RawStdioClient按原始UTF8字符串加LF并flush，无JSON预校验/写期限。response_for_id只接受无method键且id为相等字符串的JSON对象，不验证jsonrpc/result/error；method:null仍视为请求一类。有method且存在id就回-32601同id拒绝（包括null），通知及其它response被丢弃，不为未来id缓存。无效JSON也跳过。固定deadline只包stdout read_line，拒绝请求时send_line可超出期限；完整line无大小上限。诊断累计skipped并保留最后3条各200 Unicode字符，read EOF/error与timeout panic。相关stderr_tail仍有字节边界风险。

## uds_proxy.rs 完整读取

512行含6测试完整读取，unix-only。spawn先best-effort remove proxy_path再bind，未确认旧路径类型/所有者，调用方需提供独占测试路径。listener任务每次accept后顺序await upstream connect，失败丢client继续；shutdown无join等待。Drop还remove路径；shutdown本身不unlink。

4字节大端u32长度，超过64MiB拒绝，等于上限和零长度接受，先分配/读完整body再施加fault。每连接每方向从1计数，FaultPlan只作用一个方向，0永不匹配。优先级drop（直接continue）→sever（写前2字节prefix、flush、cancel）→delay→duplicate；同一帧drop抑制其它fault，duplicate两份都增加forwarded。另一方向仍经过完整帧解析及64MiB上限，非任意字节透传。

FaultHandle.sever_now取消现token并换新token，只覆盖已取得旧scope的连接；后续连接不受影响。两个pump共享conn token；read和delay用select监听取消，write_frame及mid-prefix write/flush未包取消/期限，写阻塞时不能承诺立即关闭。普通EOF/read/write错误仅结束当前pump，不自动cancel另一方向；错误未暴露给调用者。shutdown先sever再accept cancel，与尚在upstream connect后建立scope存在窗口，不能将无join接口描述成已确认全部任务终止。

forwarded为跨连接独立Relaxed计数，只在prefix/body/flush成功后加一，不证明对端应用消费；drop、half-prefix不计入。测试覆盖透传、第二帧drop、第一帧duplicate、half-prefix关闭、delay及sever_now，未覆盖重连scope、超大长度、拥塞写取消或shutdown竞态；测试本身多数无timeout。尚未执行动态测试。

## 本批追加读取证据

- `crates/codegen/test-support/src/acp_client.rs` SHA256 `261011052a6e8e0fcfb9da05433917af85aa4c1de7c2631389896eaf4ed96bb8`
- `crates/codegen/test-support/src/uds_proxy.rs` SHA256 `d7d18192280b024801d010886e4e8e3a1d03e4c842c8b4ffb590e5dac6a56288`

累计11/14份Rust文件完整读取（4839/8844行）；尚余leader、mock_server、sse，包保持pending。

## leader.rs 完整读取

969行含4测试完整读取。unix-only LeaderFixture仅拥有自己spawn的初代std Child及TestProcessTree；锁文件PID只观测，replacement不被adopt或signal。state记录binary/socket/lock、active_clients及可选初代owner；leader_pid返回仍持有的PID，不检查实时存活。

启动命令agent leader --no-exit-on-disconnect --relay-on-demand --no-auto-update；清空sandbox env后覆盖四个base URL、test key、GROW_LEADER_SOCKET和RUST_LOG=shell=debug（无额外conversation覆盖）。stdout/stdin null，stderr写grow_home/leader.log（create截断，失败null），detach后try_attach，失败kill并最多1秒wait。fixture只借sandbox，不保有目录；调用者须延长其生命。start_with_base_url接受任意URL供离线测试，未做网络隔离。

wait_ready默认30秒（不乘scale），每50ms检查socket.exists且pid_alive；不连接socket、不验证路径为socket、也不证明ACP可用。启动失败调用close，清理失败附加错误。初代退出而未reap的zombie也可能通过kill0存活观测，不能将exists+pid视为强readiness。

spawn_client可换binary/base URL，固定fixture socket；注册计数先加，registration用Weak，失败或Drop自动减（saturating）。client close/kill_and_close仅成功后移除registration；fixture.close发现active!=0报错，要求调用者先close/drop，而非代关客户端。fixture Drop不做该计数门禁，直接尝试关初代。

close/reap用spawn_blocking且持state mutex；初代已退出先WNOWAIT观测后reap，正常TERM等2秒，再group+child KILL等2秒，后续reap再wait最多2秒。reap尝试group kill但忽略其错误，release树句柄后child.wait；不宣称所有后代死亡已确认。Drop同步执行shutdown且忽略错误，可能阻塞；contain_failed_cleanup_for_unwind取走owner、发kill、mem::forget避免阻塞Drop，明确会泄漏owner且不保证reap。client同名方法只start_kill并保留所有权。

kill_current_concrete_leader仅对初代tree及child操作，无reap；reap_exited_concrete_leaders最多2秒等待退出后清空owner。wait_for_new_leader读lock，与old不同且pid_alive就返回，不验证命令身份、generation或socket服务状态，不signal观察PID。

LeaderStdioClient公开conn，可由调用方直接发协议请求；内部spawn_local仍要求LocalSet。permission优先AllowOnce否则首项/空Cancelled；文本chunks包含空文本，无session过滤或容量限制。扩展事件仅按grow/leader_reconnected、models/update、settings/update方法计数，不验证payload。所有计数累计AtomicU32。

initialize的60秒仅包initialize请求，后续authenticate无timeout；固定provider.api_key和headless metadata。create_session/model meta与普通client同样无MCP，30秒；prompt单text30秒。以上期限均不乘GROW_TEST_TIMEOUT_SCALE。超时panic不主动发协议cancel，stderr为原始tail。没有客户端sandbox所有权。

leader_lock_path/read_pid/log使用home/.grow，区别于fixture实际grow_home，显式覆盖时调用者要选正确入口。read PID trim parse u32，pid_alive未先拒绝0或>i32::MAX，cast后kill(signal0)只做观测但可能不再指单一PID；EPERM当存活。wait_for_live_leader每100ms检查，timeout零不检查。wait_for_replay_notifications只需reconnected_count>0或notification_count超过baseline，旧重连计数/无关session通知即可满足，不能证明某次完整replay完成。leader_log读失败返回空且无大小上限。

4测试静态读取：关闭初代、active注册阻止close、lock replacement不被杀、忽略TERM的后代被清理；本轮未执行。

读取证据：`crates/codegen/test-support/src/leader.rs` SHA256 `ffeac9beb1ec678e00fce69107d754b93121b537701a27281b1c5ee3e5249c48`。

累计12/14份Rust文件完整读取（5808/8844行）；尚余mock_server和sse，包仍pending。

## sse.rs 完整读取

1150行全部读取，首次输出中间截断后补读510–770，涵盖9个shape/encoder测试。生成的是内存Vec，无网络发送/节奏控制，不执行模型或验证真实provider行为；ID、时间和token usage多为固定测试值。

Messages生成6个data-only事件：message_start(input10/output0/cache0)、text block start、单个完整text delta、block stop、message_delta传入stop_reason及output5/input10、message_stop；没有event名或[DONE]，stop_reason不校验。

Chat普通版split_whitespace，首delta无前空格，其余加ASCII空格，折叠原空白；exact版按单空格split再重加前缀，保留原始字节（包括重复/首尾空格、换行、tab）。首chunk加assistant role，最后内容chunk设finish_reason=stop，然后独立空choices usage（prompt10/completion delta数量）及[DONE]。普通空白输入无内容chunk，exact空字符串产生一个空内容chunk，不能写成相同空输入事件结构。

Responses普通版每个split_whitespace word后加空格，流式拼接可能带尾空格；completed仍放原text，两者有意不一致。exact用split_inclusive单空格保留字节，空text无delta。共同created→output_text delta→completed→[DONE]，seq递增，completed固定usage10/5/15、message正文原text。数据事件转axum时保留可选event名。

reasoning_only按split_whitespace流reasoning_summary_text delta后加空格，completed仅reasoning item存原reasoning，无message/text/tool；reasoning_and_text先reasoning后answer delta(output_index1)，completed顺序reasoning+message存原文，固定usage10/10/20 reasoning5。空输入不必生成delta，但仍有对应completed item；不能保证“至少一个delta”适用于所有输入。

doom_loop_check_events基于reasoning-only，在completed前插入每个triggers累计前缀的命名response.doom_loop_check事件，terminal response.doom_loop_check带全部triggers；不去重/解析trigger。插入序号按插入index，但原completed序号不重排，不能承诺单调连续序号。空triggers无中间帧但terminal含空数组。terminal-only基于reasoning+text仅补completed字段；with_doom_loop_frame在created后插入调用者原始data，允许非法JSON/结构，自身不补terminal字段或修正sequence。terminal补丁只改首个type=completed项（any短路），找不到panic。

Responses reasoning_then_tool_call先reasoning，再output_item.added声明function(id=item_<call_id>,call_id独立,arguments空,status in_progress)，再单个完整arguments delta，completed含reasoning+function，无message，固定usage10/20/30 reasoning5。arguments不解析JSON、不执行工具。Chat对应先reasoning_content，再一条assistant/content null/tool_calls完整参数delta，再空delta finish_reason=tool_calls及固定usage，最后[DONE]；均单工具固定index0（Responses output_index1）。

9测试静态读取：多行/空白exact恢复、reasoning-only、reasoning+text、两协议reasoning+tool、累计doom帧及terminal、terminal-only、原data splice。多数是JSON结构检查，未直接依赖真实sampling parser，本轮未执行，注释中的集成保证留待调用方审阅。

读取证据：`crates/codegen/test-support/src/sse.rs` SHA256 `a956aaf9b8f40e162c9aa6ebac4facc11838d426a8dbe8eef314fd13742f9eba`。

累计13/14份Rust文件完整读取（6958/8844行）；尚余mock_server，包仍pending。

## mock_server.rs 完整读取

1886行含全部测试已逐段读取。绑定127.0.0.1:0，默认单test-model，运行axum后台任务，5秒TCP connect探测ready。Drop仅发送graceful shutdown，不await任务或终止inflight；held流/长sleep可能延迟退出。默认Echo，set_response替换为Fixed且无公开恢复Echo入口；set_agent_turns替换跨三推理端点共享前台FIFO，只有覆盖脚本和鉴权均未返回时才消费。

三POST先Json<Value> extractor成功才记录，因此非法JSON、body-limit拒绝、未匹配路由不是日志中的“全部请求”。推理固定返回SSE，不根据stream字段切非流式；model缺失/非string用test-model，不校验目录存在。Chat取最后role=user的string content，否则hello；Responses仅input数组中最后user的string或首个input_text块，顶层input string不支持；Messages取最后user的string或首个text块，不拼接多个text。三端点Echo加Echo:前缀，但Messages单delta保留空白，只有Chat/Responses走折叠encoder。

命名预期→按path脚本→required auth→前台turn队列→全局Echo/Fixed。chunk_delay在handler读取，fallback另读一次，不改变已生成stream节奏。paced_events每个事件先delay，最后事件前global wait；Chat/Responses最后是[DONE]，completed/finish_reason此前可已发送，Messages则message_stop前等待。该gate不等于业务完成事件前屏障。

GET models返回object=list/data，set_models替换目录；MockModelEntry可选_meta.agentType与顶层apiBackend，不校验字符串。GET settings默认404，set_settings任意可序列化JSON，preset空对象；settings还消费path FIFO，models不消费脚本。set_hang只作用models/settings：在处理时读bool，为true睡3600秒，不是永久挂起；false不会唤醒已sleep请求，sleep后再读最新model/settings或脚本。GET user固定mock用户，日志保留原query；这些GET不保存auth/header也不执行required auth。

router DefaultBodyLimit为256MiB；只对相应extractor生效，不是总请求/日志内存上限。RequestLog count为AtomicU32递增可能回绕，keep=false停止新entry留存但不清旧记录；requests/bodies均clone，messages_request_count与has helpers只基于保留日志，不能替代总计数。record并发时count增加早于mutex append，条目顺序是锁获取顺序，不承诺网络到达顺序。headers来自HeaderMap迭代，不保证原始wire顺序，非UTF8值lossy；authorization有效UTF8才额外保留。header查询大小写归一后首项。日志可含测试凭据，summary仅打印method/path（user query也在path）。

last_system_prompt只找最近chat/responses，不含Messages；取messages首项string content未验证role=system，失败再instructions，不继续往更早请求找有效system。这是测试便利提取，不是通用prompt还原。

测试已完整静态读取：分类header优先、辅助不偷前台预期、并发独占claim、blocked/Drop释放、同指纹并发重放及顺序新claim、body变化、primary/replay取消、名称冲突及未满足diagnostic；三端点四种body模式屏障；echo/exact、settings、FIFO/auth优先、header/raw/SSE格式、global gate覆盖及JSON/raw例外、request日志及有效token。测试内部ctor安装TLS provider；多数HTTP等待无整体timeout，不能把仅源码断言当动态通过。本轮尚未执行包测试。

读取证据：`crates/codegen/test-support/src/mock_server.rs` SHA256 `4d537e8c4720af640dcdfc260d347acf7c09b1daf80243e3406013e66594290d`。

全部14/14份Rust文件（8844行）及manifest完成静态读取。正式契约映射和测试尚未完成，包保持pending。

## test-support 包测试

cargo test --locked -p test-support --all-features 使用本任务独立target，关闭incremental与dev/test debug，退出0：71 passed、0 failed、0 ignored；doctest0。日志/tmp/grow-test-support-tests.log。测试后立即cargo clean，删除2615文件695.2MiB，可用磁盘约72GiB。该结果只覆盖本平台包内测试，不证明Windows、真实grow/ACP端到端、极端输入和故障竞态均正确。16项基础契约暂存draft，仍需补齐其余模块映射，不提前标reviewed。

## 正式登记

全部14份Rust文件和manifest已审阅，50项契约登记至test-harness-runtime；71项本平台测试通过并清理target。以上保留阶段记录，以本节为最终包状态。
