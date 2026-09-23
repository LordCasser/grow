## ADDED Requirements

### Requirement: Test harness timeout scaling
scaled SHALL 读取正整数 GROW_TEST_TIMEOUT_SCALE 并乘基础 Duration；缺失、非法或零取1。

#### Scenario: 实现边界
- **WHEN** 使用极大期限或未调用scaled的入口
- **THEN** 不保证乘法溢出安全；leader固定期限不自动缩放。

证据：`crates/codegen/test-support/src/lib.rs` — `pub fn scaled`。

### Requirement: Test harness environment restoration
EnvGuard SHALL 保存原始OsString环境值，设置或移除后在Drop恢复原值。

#### Scenario: 实现边界
- **WHEN** 调用方并发访问进程环境
- **THEN** guard没有内部串行机制，调用方须满足unsafe环境修改前提。

证据：`crates/codegen/test-support/src/env.rs` — `impl Drop for EnvGuard`。

### Requirement: Test harness binary discovery
grow_binary SHALL 依次选择存在的GROW_BINARY、CARGO_BIN_EXE_grow或target/debug/grow，fallback缺失时构建cli。

#### Scenario: 实现边界
- **WHEN** 已有路径存在
- **THEN** 不检查二进制新鲜度或可执行性；fallback build未加--locked。

证据：`crates/codegen/test-support/src/env.rs` — `pub fn grow_binary`。

### Requirement: Test harness resource growth
ResourceSnapshot SHALL 提供本进程RSS及可用线程/FD采样，growth_from逐字段饱和相减并传播None。

#### Scenario: 实现边界
- **WHEN** 平台无法提供指标
- **THEN** 返回None；多个指标不是原子快照，macOS只提供ps读取的RSS。

证据：`crates/codegen/test-support/src/resources.rs` — `pub fn growth_from`。

### Requirement: Test harness sandbox filesystem ownership
TestSandbox SHALL 用TempDir拥有home、grow_home、workspace和tmp目录，构造不修改全局环境。

#### Scenario: 实现边界
- **WHEN** 将sandbox借给运行中的进程
- **THEN** 调用方负责延长目录生命；这不是OS访问控制沙箱。

证据：`crates/codegen/test-support/src/sandbox.rs` — `pub fn build`。

### Requirement: Test harness sandbox environment precedence
sandbox应用到std或Tokio命令 SHALL 先env_clear再写入有序基线，后续显式覆盖获胜。

#### Scenario: 实现边界
- **WHEN** 应用到portable PTY CommandBuilder
- **THEN** 仅合并环境，调用方须先env_clear。

证据：`crates/codegen/test-support/src/sandbox.rs` — `pub fn apply_to_command_builder`。

### Requirement: Test harness mock endpoint wiring
set_mock_url SHALL 配置chat proxy、inference、models、feedback、conversations五项base URL及测试API key。

#### Scenario: 实现边界
- **WHEN** 传入非loopback或非法URL
- **THEN** 本层不验证或提供网络隔离，不推导trace/web端点已覆盖。

证据：`crates/codegen/test-support/src/sandbox.rs` — `fn apply_mock_url`。

### Requirement: Test harness git sandbox fixture
git sandbox SHALL 建立含已提交README的仓库，使用隔离Git环境、禁交互及签名的提交。

#### Scenario: 实现边界
- **WHEN** Git命令失败
- **THEN** 断言失败并带诊断；命令没有内置执行期限。

证据：`crates/codegen/test-support/src/sandbox.rs` — `fn init_git_workspace`。

### Requirement: Test harness diagnostic sanitization
sandbox diagnostics SHALL 按key启发式隐藏凭据值，只展示移除userinfo/query/fragment后的合法loopback URL。

#### Scenario: 实现边界
- **WHEN** 秘密位于URL path或未知key值
- **THEN** 不保证被隐藏；进程tail公开接口仍返回原始文本。

证据：`crates/codegen/test-support/src/sandbox.rs` — `fn sanitize_endpoint`。

### Requirement: Test harness process output policy
TestProcess SHALL 默认使用Null stdin、后台Capture输出和64KiB诊断tail，Piped交由调用方读取时更新tail。

#### Scenario: 实现边界
- **WHEN** Piped输出无人读取
- **THEN** 不自动排空，子进程可能被pipe背压阻塞；tail用UTF8 lossy展示。

证据：`crates/codegen/test-support/src/process.rs` — `pub enum TestOutput`。

### Requirement: Test harness process environment overrides
TestProcess spawn SHALL 在sandbox清空环境后应用pager环境及config显式覆盖，并重新设置stdio及detach。

#### Scenario: 实现边界
- **WHEN** 命令预先设置环境
- **THEN** 被sandbox清除；最终config覆盖优先。

证据：`crates/codegen/test-support/src/process.rs` — `pub fn spawn`。

### Requirement: Test harness nonreaping child observation
Unix进程收割 SHALL 先WNOWAIT观测退出，清理process group后再消费直接子进程状态。

#### Scenario: 实现边界
- **WHEN** waitid返回ECHILD或非法PID
- **THEN** 传播错误，不将他人收割当作成功；后代清理错误可记录后继续收割。

证据：`crates/codegen/test-support/src/process.rs` — `pub fn process_has_exited_without_reap`。

### Requirement: Test harness process close escalation
close SHALL 尝试优雅终止并在grace期后升级hard kill，kill同时尝试group及直接子进程。

#### Scenario: 实现边界
- **WHEN** wait_with_deadline超时
- **THEN** 返回None并保留所有权，不隐式kill；输出drain可额外增加总耗时。

证据：`crates/codegen/test-support/src/process.rs` — `pub async fn close`。

### Requirement: Test harness process drop cleanup
TestProcess Drop SHALL 尝试kill并最多250ms同步收割，随后abort内部capture任务。

#### Scenario: 实现边界
- **WHEN** 超时或清理失败
- **THEN** 不保证已收割全部进程；外部reader任务不在内部capture所有权中。

证据：`crates/codegen/test-support/src/process.rs` — `impl Drop for TestProcess`。

### Requirement: Test harness headless output collection
headless runner SHALL 用60秒乘scale的进程期限，超时kill并标记timed_out，stdout/stderr分别完整收集。

#### Scenario: 实现边界
- **WHEN** 输出drain失败或超过2秒
- **THEN** 返回预取tail快照；完整read_to_end本身无总量上限。

证据：`crates/codegen/test-support/src/headless.rs` — `async fn finish_output_drain`。

### Requirement: Test harness headless assertion limits
assert_headless_success SHALL 检查非timeout及成功exit，assert_no_crashes执行关键词匹配。

#### Scenario: 实现边界
- **WHEN** 使用stderr_tail截取多字节文本
- **THEN** 按字节偏移切片可能panic，不能作为字符安全截断保证。

证据：`crates/codegen/test-support/src/headless.rs` — `pub fn stderr_tail`。

### Requirement: Test harness request classification
推理请求 SHALL 优先按非空turn-idx标为foreground，其次非空req-id标为auxiliary，否则按tools数组至少2项判断foreground。

#### Scenario: 实现边界
- **WHEN** 显式req-id与多工具同时存在
- **THEN** 无turn-idx时仍为auxiliary，不让工具数覆盖显式分类。

证据：`crates/codegen/test-support/src/inference_override.rs` — `fn classify`。

### Requirement: Test harness response override precedence
响应选择 SHALL 依次尝试命名预期、路径FIFO脚本、required-token鉴权，再进入fallback。

#### Scenario: 实现边界
- **WHEN** 脚本存在但token无效
- **THEN** 脚本仍可返回；FIFO仅区分路径，不区分前台辅助。

证据：`crates/codegen/test-support/src/inference_override.rs` — `pub(crate) async fn response_override`。

### Requirement: Test harness expectation matching
命名预期 SHALL 原子领取首个endpoint和kind匹配的pending项，保留其它项。

#### Scenario: 实现边界
- **WHEN** 重复注册当前pending/inflight名称
- **THEN** panic；不保证名称在整个历史中永远不可复用。

证据：`crates/codegen/test-support/src/inference_override.rs` — `fn claim_expectation`。

### Requirement: Test harness overlapping request replay
预期 SHALL 对endpoint、kind、req-id及序列化body相同且仍active的请求复用响应及屏障。

#### Scenario: 实现边界
- **WHEN** 请求无req-id、body改变或先前active已清空
- **THEN** 不使用该复用条目，可以领取下一项；不是持久幂等缓存。

证据：`crates/codegen/test-support/src/inference_override.rs` — `struct ModelCallFingerprint`。

### Requirement: Test harness expectation terminal ownership
Satisfied SHALL 要求primary跨越本地terminal屏障，有指纹时还需全部active lease结束。

#### Scenario: 实现边界
- **WHEN** 仅Replay跨屏障或primary取消
- **THEN** 不能代替primary满足；不保证客户端已经消费响应。

证据：`crates/codegen/test-support/src/inference_override.rs` — `fn update_shared_state`。

### Requirement: Test harness expectation release lifecycle
预期handle SHALL 提供received/blocked/satisfied等待、release及显式assert_satisfied，Drop仅释放屏障。

#### Scenario: 实现边界
- **WHEN** 等待已越过的阶段
- **THEN** received接受后续阶段，blocked接受Satisfied；等待没有内置timeout。

证据：`crates/codegen/test-support/src/inference_override.rs` — `impl InferenceExpectation`。

### Requirement: Test harness script body pacing
ScriptedResponse SHALL 支持JSON、UTF8 Raw及SSE列表，SSE每事件先delay并在最后列表项前等待terminal barrier。

#### Scenario: 实现边界
- **WHEN** SSE为空或JSON/Raw有barrier
- **THEN** 空SSE执行占位等待，JSON/Raw返回前等待；不是按协议事件语义识别terminal。

证据：`crates/codegen/test-support/src/scripted.rs` — `pub(crate) async fn into_response_paced`。

### Requirement: Test harness script validation
脚本登记 SHALL 校验status及header，响应header依序insert覆盖同名字段。

#### Scenario: 实现边界
- **WHEN** 事件内容不合法
- **THEN** 登记校验不覆盖SSE data/event内容；Raw是String而非任意非UTF8字节。

证据：`crates/codegen/test-support/src/scripted.rs` — `pub(crate) fn validate`。

### Requirement: Test harness mock server lifecycle
MockInferenceServer SHALL 绑定loopback随机端口，以TCP连接探测启动，并在Drop发送graceful shutdown。

#### Scenario: 实现边界
- **WHEN** 仍有被hold或长sleep的请求
- **THEN** Drop不等待后台任务退出，不保证立即关闭全部连接。

证据：`crates/codegen/test-support/src/mock_server.rs` — `async fn start_inner`。

### Requirement: Test harness mock response modes
推理fallback SHALL 优先消费共享前台turn队列，否则使用Echo或Fixed文本模式，均返回SSE。

#### Scenario: 实现边界
- **WHEN** 请求stream=false或model不在目录
- **THEN** 不据此切换非流式或拒绝model；缺失model用test-model。

证据：`crates/codegen/test-support/src/mock_server.rs` — `fn pop_agent_turn`。

### Requirement: Test harness mock user text extraction
mock SHALL 取最后user消息；Chat只接受string，Responses另接受首个input_text块，Messages另接受首个text块。

#### Scenario: 实现边界
- **WHEN** 缺少支持的文本形态
- **THEN** 使用hello，不合并多个块；Messages Echo保留原空白。

证据：`crates/codegen/test-support/src/mock_server.rs` — `fn build_router`。

### Requirement: Test harness mock catalog and settings
mock SHALL 支持替换模型目录与任意JSON settings；settings默认404并优先消费settings路径FIFO。

#### Scenario: 实现边界
- **WHEN** 对models登记FIFO脚本
- **THEN** models路由不消费脚本；可选agentType在_meta，apiBackend在顶层。

证据：`crates/codegen/test-support/src/mock_server.rs` — `pub fn set_settings`。

### Requirement: Test harness mock hang semantics
set_hang SHALL 使models/settings在处理时发现true后睡3600秒，再继续响应。

#### Scenario: 实现边界
- **WHEN** 稍后将hang设false
- **THEN** 不会唤醒已经睡眠的请求，不是永久挂起。

证据：`crates/codegen/test-support/src/mock_server.rs` — `pub fn set_hang`。

### Requirement: Test harness mock request logging
mock SHALL 记录已进入路由handler的请求，总count与可选保留entry分别维护。

#### Scenario: 实现边界
- **WHEN** 关闭keep_requests或请求被extractor拒绝
- **THEN** 关闭不清旧entry；非法JSON等提前拒绝不记入该日志，计数u32可回绕。

证据：`crates/codegen/test-support/src/mock_server.rs` — `fn record`。

### Requirement: Test harness mock log query limits
last_system_prompt SHALL 从最近chat/responses取首消息string content或instructions。

#### Scenario: 实现边界
- **WHEN** 首消息不是system或最近请求无可取内容
- **THEN** 不校验role、不回溯更早请求，也不覆盖Messages。

证据：`crates/codegen/test-support/src/mock_server.rs` — `pub fn last_system_prompt`。

### Requirement: Test harness compatibility completion hold
global completion hold SHALL 作用foreground fallback及FIFO SSE的最后一个事件。

#### Scenario: 实现边界
- **WHEN** Chat或Responses发送完成字段后
- **THEN** 仍可能在最后DONE前等待；FIFO JSON/Raw不受该global gate约束。

证据：`crates/codegen/test-support/src/mock_server.rs` — `fn paced_events`。

### Requirement: Test harness chat exact text fixtures
Chat exact生成器 SHALL 保留文本字节，普通生成器折叠空白；末内容chunk带stop，随后usage及DONE。

#### Scenario: 实现边界
- **WHEN** 输入空字符串
- **THEN** exact仍有空内容delta，普通空白输入无内容delta。

证据：`crates/codegen/test-support/src/sse.rs` — `fn chat_completion_script_from_deltas`。

### Requirement: Test harness responses exact text fixtures
Responses exact生成器 SHALL 按包含单空格的片段保留文本字节，输出created、delta、completed及DONE。

#### Scenario: 实现边界
- **WHEN** 使用普通Echo编码
- **THEN** 流式word追加空格，而completed保存原text，两者可能不同。

证据：`crates/codegen/test-support/src/sse.rs` — `fn responses_api_script_from_deltas`。

### Requirement: Test harness messages text fixtures
Messages生成器 SHALL 发message_start、text block开始/单delta/结束、带stop_reason的message_delta及message_stop。

#### Scenario: 实现边界
- **WHEN** 传入任意stop_reason
- **THEN** 原样写入，不验证协议枚举；不追加DONE。

证据：`crates/codegen/test-support/src/sse.rs` — `pub fn messages_api_events`。

### Requirement: Test harness reasoning fixtures
Responses reasoning生成器 SHALL 支持仅reasoning及reasoning后text两种输出，completed保留相应原文item。

#### Scenario: 实现边界
- **WHEN** reasoning为空
- **THEN** 可以没有reasoning delta但仍有reasoning item，usage为固定测试值。

证据：`crates/codegen/test-support/src/sse.rs` — `pub fn responses_api_reasoning_and_text_events`。

### Requirement: Test harness tool call fixtures
两协议reasoning-then-tool生成器 SHALL 在reasoning后生成单工具完整参数，Responses先声明item再发参数delta。

#### Scenario: 实现边界
- **WHEN** arguments不是合法JSON
- **THEN** 仍原样生成字符串，不执行工具或验证参数。

证据：`crates/codegen/test-support/src/sse.rs` — `pub fn responses_api_reasoning_then_tool_call_events`。

### Requirement: Test harness doom loop fixtures
doom-loop生成器 SHALL 支持累计命名帧加terminal字段、仅terminal字段及任意原始data插入三种形态。

#### Scenario: 实现边界
- **WHEN** 插入累计帧
- **THEN** 不重排原completed sequence，不保证序号连续；不解析去重trigger。

证据：`crates/codegen/test-support/src/sse.rs` — `pub fn responses_api_doom_loop_check_events`。

### Requirement: Test harness typed ACP initialization
typed ACP client SHALL spawn agent stdio并提供显式V1初始化、provider.api_key认证和无MCP会话创建。

#### Scenario: 实现边界
- **WHEN** 仅调用spawn
- **THEN** 不自动initialize；spawn_local要求调用方LocalSet。

证据：`crates/codegen/test-support/src/acp_client.rs` — `pub async fn initialize`。

### Requirement: Test harness ACP permission selection
测试ACP callback SHALL 优先选择AllowOnce，否则首项，无选项返回Cancelled。

#### Scenario: 实现边界
- **WHEN** 首项不是允许选项
- **THEN** 仍按首项选择，不保证总是批准。

证据：`crates/codegen/test-support/src/acp_client.rs` — `async fn request_permission`。

### Requirement: Test harness ACP capture and timeouts
typed client SHALL 累计全局通知和非空文本，提供20秒初始化/会话/模型、30秒prompt、60秒load缩放期限包装。

#### Scenario: 实现边界
- **WHEN** 包装超时
- **THEN** panic而不主动发cancel；底层无timeout入口及扩展调用仍需调用方约束。

证据：`crates/codegen/test-support/src/acp_client.rs` — `pub async fn prompt_with_timeout`。

### Requirement: Test harness raw ACP matching
RawStdioClient SHALL 原样发送UTF8行加LF，以无method键且相等string id匹配响应。

#### Scenario: 实现边界
- **WHEN** 遇到其它请求
- **THEN** 回复-32601，其它消息丢弃；写操作不被read deadline覆盖。

证据：`crates/codegen/test-support/src/acp_client.rs` — `pub async fn response_for_id`。

### Requirement: Test harness leader concrete ownership
LeaderFixture SHALL 仅拥有其spawn的初代leader，锁文件replacement PID仅供观测。

#### Scenario: 实现边界
- **WHEN** 等待新leader或fixture关闭
- **THEN** 不adopt或signal replacement；PID存活不是服务身份验证。

证据：`crates/codegen/test-support/src/leader.rs` — `pub async fn wait_for_new_leader`。

### Requirement: Test harness leader readiness and registration
leader ready SHALL 以socket路径存在及PID存活判断，fixture close要求active client注册数为零。

#### Scenario: 实现边界
- **WHEN** 仍有client或socket路径陈旧
- **THEN** close报错；路径存在不证明ACP就绪，fixture Drop不执行注册门禁。

证据：`crates/codegen/test-support/src/leader.rs` — `async fn wait_ready`。

### Requirement: Test harness leader cleanup containment
leader cleanup SHALL 尝试TERM及hard kill并收割直接owner；失败unwind containment可kill后forget owner。

#### Scenario: 实现边界
- **WHEN** 使用containment
- **THEN** 不保证reap，明确泄漏owner以避免阻塞Drop。

证据：`crates/codegen/test-support/src/leader.rs` — `pub fn contain_failed_cleanup_for_unwind`。

### Requirement: Test harness leader ACP deadlines
leader client SHALL 对initialize请求限60秒、new/prompt限30秒，捕获重连/models/settings通知计数。

#### Scenario: 实现边界
- **WHEN** initialize后authenticate或等待replay
- **THEN** authenticate无内置期限；计数变化不证明指定replay完整完成，期限不乘scale。

证据：`crates/codegen/test-support/src/leader.rs` — `pub async fn initialize`。

### Requirement: Test harness UDS frame faults
UDS代理 SHALL 按每连接每方向1起始帧号施加drop、half-prefix sever、delay、duplicate，优先级依此。

#### Scenario: 实现边界
- **WHEN** 多个fault指向同帧
- **THEN** drop抑制其余；duplicate每成功副本都计forwarded。

证据：`crates/codegen/test-support/src/uds_proxy.rs` — `async fn pump_frames`。

### Requirement: Test harness UDS frame bounds
UDS代理 SHALL 读取4字节大端长度及完整body，拒绝超过64MiB的帧。

#### Scenario: 实现边界
- **WHEN** 未选fault的方向或零长度帧
- **THEN** 仍执行帧解析和上限；零长度接受，不是任意字节透传。

证据：`crates/codegen/test-support/src/uds_proxy.rs` — `async fn read_frame`。

### Requirement: Test harness UDS cancellation boundary
sever_now SHALL 取消现有连接scope并更换token，shutdown同时通知listener取消。

#### Scenario: 实现边界
- **WHEN** pump阻塞于write或upstream connect尚未完成
- **THEN** 无join确认，不能保证立即关闭；普通EOF不自动取消反向pump。

证据：`crates/codegen/test-support/src/uds_proxy.rs` — `pub fn shutdown`。

### Requirement: Test harness counting HTTP server
counting server SHALL 统计TCP accept并按Content-Length消费请求，记录header后固定回复200 JSON。

#### Scenario: 实现边界
- **WHEN** chunked或无界header输入
- **THEN** 不提供完整HTTP解析或大小/超时保护，无显式shutdown句柄。

证据：`crates/codegen/test-support/src/counting_server.rs` — `pub async fn spawn_counting_server`。


### Requirement: PTY harness binary resolution and implicit build
pager_binary SHALL 优先读取 Unicode PAGER_BINARY；该路径不存在即报错，不继续回退，存在时转绝对路径。其次采用存在的 CARGO_BIN_EXE_grow，原样返回其路径。否则选择 CARGO_TARGET_DIR 或工作区 target 下 debug/grow 加平台后缀，仅在路径不存在时运行 cargo build -p cli --bin grow；已有路径不校验新鲜度、文件类型或可执行权限。自动构建失败或构建后路径仍缺失均报错。

#### Scenario: Invalid explicit override
- **WHEN** PAGER_BINARY 指向不存在的路径
- **THEN** 立即报错，即使 CARGO_BIN_EXE_grow 有效也不回退。

#### Scenario: Existing local artifact
- **WHEN** 未解析到两个环境覆盖且本地 debug/grow 路径存在
- **THEN** 复用该路径，不构建也不检查是否对应当前源码。

#### Scenario: Missing local artifact
- **WHEN** 本地路径不存在
- **THEN** 在编译期 manifest 推导的工作区根运行 CARGO 指定的程序或 cargo，构建 cli 的 grow；成功后再检查路径存在。

证据：`crates/codegen/pager-pty-harness/src/env.rs` — `pub fn pager_binary`；`crates/codegen/pager-pty-harness/src/env.rs` — `fn ensure_local_pager_binary`；`crates/codegen/pager-pty-harness/src/env.rs` — `fn target_dir`。

### Requirement: PTY frame timing parser measurement boundary
FrameTimingParser SHALL 在持续 VTE 流中识别 intermediates 恰为 ? 且首个参数首值为 2026 的 h/l；h 重置起始 Instant 和字符计数，l 仅在存在起点时追加记录。duration 测量 harness 解析两个标记之间的墙钟时间，不是子进程渲染耗时或屏幕呈现延迟；chars 统计 VTE print 回调次数，不是 UTF-8 字节数或屏幕列宽。reset 清空结果和当前帧状态，但保留 VTE parser 的未完成序列状态。

#### Scenario: Repeated begin marker
- **WHEN** 一帧尚未结束又收到有效 2026 h
- **THEN** 用新起点和零字符计数替换旧帧，不产生旧帧记录。

#### Scenario: End without begin
- **WHEN** 无当前帧起点时收到有效 2026 l
- **THEN** 不增加 frame_count。

#### Scenario: Additional mode parameters
- **WHEN** 2026 不在首个参数首值，或位于首值且还有其他参数
- **THEN** 前者不识别为帧标记，后者仍按 h/l 处理。

#### Scenario: Reset in partial escape sequence
- **WHEN** feed 收到部分 CSI 后调用 reset 再继续 feed
- **THEN** 已记录帧被清空，但解析器可继续补全之前的序列。

证据：`crates/codegen/pager-pty-harness/src/timing.rs` — `impl FrameTimingParser`；`crates/codegen/pager-pty-harness/src/timing.rs` — `impl vte::Perform for FrameTimingHandler`。

### Requirement: PTY benchmark aggregation and percentile semantics
BenchResults.from_timings SHALL 对空输入输出全零数值；非空输入按 duration 毫秒排序，percentile 使用 round(pct/100*(n-1)) 对应元素而非插值。avg_fps 使用调用方 wall_time，零时长得到 0；jank 仅计严格超过两倍 p50 的帧，jank_rate 为其占比，chars_per_frame_avg 为字符计数均值。percentile 的排序与百分比范围前置条件仅由 debug_assert 检查。

#### Scenario: Two samples median
- **WHEN** 样本时长为 10 和 20 ms
- **THEN** p50 为 20 ms，不取两者平均。

#### Scenario: Exact jank threshold
- **WHEN** 某帧时长恰为两倍 p50
- **THEN** 该帧不计入 jank。

证据：`crates/codegen/pager-pty-harness/src/results.rs` — `pub fn from_timings`；`crates/codegen/pager-pty-harness/src/results.rs` — `pub fn percentile`。

### Requirement: PTY baseline persistence and regression scope
基线 SHALL 以 scenario 名为 HashMap 键序列化为 JSON；write_baseline 创建父目录并直接覆盖文件，同名结果后者覆盖前者。compare_baseline 按当前结果顺序仅比较 p99 相对增长，缺失基线或基线 p99<=0 跳过，增长严格大于传入 threshold 才报告；默认阈值常量为 0.15，不在比较函数内自动应用。

#### Scenario: Missing baseline scenario
- **WHEN** 当前结果无对应基线
- **THEN** 跳过，不将首次运行判为退化。

#### Scenario: Boundary threshold
- **WHEN** p99 相对增长等于给定 threshold
- **THEN** 不报告退化。

#### Scenario: Other metrics regress
- **WHEN** 仅 avg_fps 或 jank_rate 变差而 p99 未超过阈值
- **THEN** compare_baseline 不据这些指标报告退化。

证据：`crates/codegen/pager-pty-harness/src/results.rs` — `pub fn write_baseline`；`crates/codegen/pager-pty-harness/src/results.rs` — `pub fn load_baseline`；`crates/codegen/pager-pty-harness/src/results.rs` — `pub fn compare_baseline`。

### Requirement: PTY screen visible and historical query separation
ScreenTracker SHALL 通过 ptyctl Terminal 解析字节；contents 将默认 screen_content 行以换行连接，contains 只查该文本。scrollback_text 读取当前全部已保留历史，full_text 在历史非空时以换行拼接历史与当前屏幕，full_contains 查询该组合。styled/html 使用默认 ScreenOpts；resize 将行列参数换序传给 Terminal，cursor_position 将底层 1-based 坐标转为 u16 后饱和减一。

#### Scenario: Text scrolled off visible screen
- **WHEN** 文本已进入 emulator scrollback
- **THEN** contains 可为 false 而 full_contains 为 true；后者不证明屏幕上仍可见。

#### Scenario: No scrollback
- **WHEN** 历史文本为空
- **THEN** full_text 直接返回屏幕文本，不添加前置换行。

证据：`crates/codegen/pager-pty-harness/src/screen.rs` — `impl ScreenTracker`。

### Requirement: PTY emulator reply draining
ScreenTracker SHALL 为 emulator 回复建立无界通道；drain_responses 非阻塞取走当前排队回复并按顺序连接字节，已取走的回复不在下次重复返回。该方法只返回字节，不自行向子进程写入；实际探测回复依赖 harness 调用者转发。

#### Scenario: Drain twice
- **WHEN** 一次查询已产生回复且其间无新回复
- **THEN** 第一次 drain 返回排队字节，第二次为空。

#### Scenario: Reply not forwarded
- **WHEN** 调用者只 feed 查询而未向 PTY 写回 drain 结果
- **THEN** 不能从 emulator 生成回复推导子进程已收到回复。

证据：`crates/codegen/pager-pty-harness/src/screen.rs` — `pub fn drain_responses`；`crates/codegen/pager-pty-harness/src/screen.rs` — `pub fn new`。

### Requirement: PTY flow label waiting and submit retries
wait_for_labels_absent SHALL 等待所有 label 在可见屏幕中消失，但丢弃 wait_until 的错误且无结果返回。submit_turn 先注入 prompt 加 CR，初次注入失败 panic；以最多 10 秒的单次等待寻找 sentinel，失败且总 deadline 未到时重发 CR并忽略重发错误，总超时 panic。成功仅代表屏幕出现 sentinel，helper 不验证请求身份或 exactly-once。

#### Scenario: Labels timeout
- **WHEN** 指定标签在超时后仍存在
- **THEN** helper 返回 unit，不向调用方传播等待错误。

#### Scenario: Preexisting sentinel
- **WHEN** sentinel 已在屏幕上
- **THEN** 提交后可立即满足文本等待，不能据此证明本次 turn 已开始。

证据：`crates/codegen/pager-pty-harness/src/flows.rs` — `pub fn wait_for_labels_absent`；`crates/codegen/pager-pty-harness/src/flows.rs` — `pub fn submit_turn`。

### Requirement: PTY inference count path matching
inference_request_count SHALL 对记录的请求按 path 包含 /chat/completions、/responses 或 /messages 任一子串计数；不按 HTTP method、响应状态或严格完整 endpoint 匹配。

#### Scenario: Incidental model discovery
- **WHEN** 请求路径仅为 /v1/models
- **THEN** 不计入 inference count。

#### Scenario: Substring endpoint
- **WHEN** 请求 path 含 /responses 子串但不等于标准 endpoint
- **THEN** 仍计入，不由该 helper 验证请求性质。

证据：`crates/codegen/pager-pty-harness/src/flows.rs` — `pub fn inference_request_count`。

### Requirement: PTY model waiting creates new sessions
wait_for_model_via_new_sessions SHALL 每轮先检查可见屏幕是否包含 model，存在返回 true；否则 deadline 已到返回 false，未到则注入 /new 加 CR并忽略注入错误，update 固定 3 秒后重试。该等待会创建会话，单次 update 不缩短到剩余 deadline。

#### Scenario: Model already visible at deadline
- **WHEN** 调用时 model 已可见且 timeout 为零
- **THEN** 先匹配文本并返回 true。

#### Scenario: Short remaining deadline
- **WHEN** model 不可见且剩余时间少于 3 秒
- **THEN** 仍注入 /new 并调用完整 3 秒 update，返回时间可能超过 deadline。

证据：`crates/codegen/pager-pty-harness/src/flows.rs` — `pub fn wait_for_model_via_new_sessions`。

### Requirement: Host clipboard text commands and restoration scope
宿主剪贴板文本 helper SHALL 在 Windows 通过 PowerShell Set/Get-Clipboard，在非 Windows 通过 pbcopy/pbpaste 操作机器全局剪贴板；写入文本走 stdin，成功要求进程成功退出，读取失败返回 None，stdout 使用 UTF-8 lossy 解码。HostClipboardTextGuard.save 保存文本表示，Drop 仅在 prior 为 Some 时尝试恢复并忽略错误，不恢复图片或其他格式，也不提供并发互斥。

#### Scenario: Unavailable original text
- **WHEN** save 时读取返回 None
- **THEN** Drop 不清空或恢复剪贴板。

#### Scenario: Original image clipboard
- **WHEN** 原剪贴板包含图片
- **THEN** guard 不能保证恢复图片，只保存读取到的文本表示。

#### Scenario: Non Windows unsupported host
- **WHEN** 非 Windows 系统没有 pbcopy/pbpaste
- **THEN** 仍尝试这些程序，按启动失败返回错误/None，不自动切换 Linux 后端。

证据：`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `pub fn pbcopy`；`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `pub fn pbpaste`；`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `impl Drop for HostClipboardTextGuard`。

### Requirement: Host clipboard roundtrip mutates global state
clipboard_roundtrip_works SHALL 写入 HOSTCLIPROUNDTRIP 加当前 PID，再读取并 trim_end 后与该值比较；写入失败返回 false。函数自身不保存或恢复此前剪贴板，true 只确认该次文本往返，nonce 不是每次调用随机生成。

#### Scenario: Successful roundtrip
- **WHEN** 写入和读取均成功且读取值仅多出末尾空白
- **THEN** 返回 true，剪贴板仍含探测文本，恢复由外部 guard 负责。

证据：`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `pub fn clipboard_roundtrip_works`。

### Requirement: Host clipboard PNG fixtures and platform encoding
write_fixture_png SHALL 在给定目录写入固定名 host_clipboard_fixture.png 的 64×64 RGBA(200,40,120,255) 图片，不创建父目录。set_clipboard_png 在非 Windows 以 osascript 读取 PNGf，路径直接插入双引号脚本文本；Windows 将单引号转义后构造 WinForms SetImage 脚本，以 UTF-16LE Base64 EncodedCommand 和 STA 调用 PowerShell。成功以退出状态判定，不执行图片回读验证。

#### Scenario: Missing fixture directory
- **WHEN** 给定目录不存在
- **THEN** 保存失败并返回错误，不先创建目录。

#### Scenario: Windows image command
- **WHEN** Windows 调用 set_clipboard_png
- **THEN** 以 STA PowerShell 执行图片加载、SetImage 和 Dispose，非零退出报错。

证据：`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `pub fn write_fixture_png`；`crates/codegen/pager-pty-harness/src/host_clipboard.rs` — `pub fn set_clipboard_png`。

### Requirement: PTY leader cluster shared socket launch
LeaderCluster.start SHALL 启动一个 ContentController，创建其 home/.grow 并使用 leader-e2e.sock 路径，解析 pager binary，保存终端尺寸；start 本身不启动 pager 客户端。spawn_leader 和 attach 共享 content，均传 --leader --leader-socket，attach 另外传 --resume，调用方 extra_args 追加在后。客户端 PtyHarness 由调用方持有，cluster 不保存客户端集合。

#### Scenario: Cluster allocation
- **WHEN** 仅调用 LeaderCluster.start
- **THEN** 建立共享控制器和 socket 路径配置，不据此认为 leader 进程已启动。

#### Scenario: Attach client
- **WHEN** 调用 attach
- **THEN** 与首客户端使用同一配置 socket，并追加 --resume 后再追加调用方参数。

证据：`crates/codegen/pager-pty-harness/src/leader.rs` — `pub async fn start`；`crates/codegen/pager-pty-harness/src/leader.rs` — `fn spawn_client`。

### Requirement: PTY leader persisted update scan tolerance
session_updates SHALL 递归扫描 home/.grow/sessions 下所有 updates.jsonl，目录读取及条目类型失败跳过，不递归符号链接目录；匹配文件名的非目录路径会尝试读取。文件读取或 UTF-8 解码失败跳过整个文件；有效文本逐行 trim，忽略空行、JSON 错误及缺失 params.update 的记录，保留其他 update 值。扫描不排序、不按 session ID 或 method 过滤。

#### Scenario: Torn UTF8 file tail
- **WHEN** 文件尾使 read_to_string 失败
- **THEN** 本次扫描跳过整个文件，下一次重新扫描。

#### Scenario: Torn JSON final line
- **WHEN** 文件 UTF-8 有效而最后一行 JSON 不完整
- **THEN** 保留之前可解析 payload，跳过错误行。

证据：`crates/codegen/pager-pty-harness/src/leader.rs` — `pub fn session_updates`；`crates/codegen/pager-pty-harness/src/leader.rs` — `fn parse_update_payloads`；`crates/codegen/pager-pty-harness/src/leader.rs` — `fn collect_updates_files`。

### Requirement: PTY leader completion wait identity scope
wait_for_turn_completed SHALL 每轮重读所有 session updates，返回首个 sessionUpdate=turn_completed 的 payload；不要求新记录、指定 prompt/session 或成功 stop_reason。无匹配且 deadline 已到时报错，包含目录、记录数和已见 tag 集合；否则固定睡眠 150ms 后再读，未裁剪到剩余时间。

#### Scenario: Prior completion exists
- **WHEN** 其他会话或旧 turn 已有 turn_completed
- **THEN** 可立即返回旧记录，调用者须自行验证目标身份。

#### Scenario: Cancelled completion
- **WHEN** 记录 tag 为 turn_completed 且 stop_reason=cancelled
- **THEN** 仍满足等待条件。

证据：`crates/codegen/pager-pty-harness/src/leader.rs` — `pub fn wait_for_turn_completed`；`crates/codegen/pager-pty-harness/src/leader.rs` — `fn is_turn_completed`。

### Requirement: PTY child environment layering
apply_child_env SHALL 在提供 TestSandbox 时先清空继承并应用 sandbox；无 sandbox 时保留其他继承环境。两条路径都设置 TERM=xterm-256color，删除 NO_COLOR/CLICOLOR/CLICOLOR_FORCE、四个 SSH 变量、两个 OSC52 sink 变量及 HOST_TERMINAL_ENV_VARS 所列品牌/复用器/编辑器变量，最后按顺序执行调用方 EnvOp Set/Remove。EnvOp 支持 OsStr，后续操作可覆盖或删除前面设置。

#### Scenario: Explicit terminal override
- **WHEN** 调用方最后设置 TERM_PROGRAM 或 NO_COLOR
- **THEN** 覆盖清理阶段的删除结果，允许测试显式模拟该环境。

#### Scenario: Inherited environment launch
- **WHEN** 使用 spawn_inherited_env
- **THEN** 仍执行颜色、SSH、sink 与终端身份清理，不是完整原样继承。

证据：`crates/codegen/pager-pty-harness/src/pty.rs` — `fn apply_child_env`；`crates/codegen/pager-pty-harness/src/pty.rs` — `pub enum EnvOp`。

### Requirement: PTY reader channel and drain boundary
spawn_reader SHALL 在独立 pty-reader 线程中以最多 8192 字节单次读取并通过无界 mpsc 传递拥有的字节块；EOF、任意读取错误或接收端关闭结束线程，线程创建失败 panic。drain_output 在给定总预算内持续收集块，超时或通道断开返回已有块；零预算不先尝试取出已排队块。inject_keys 只 write_all，不额外 flush。

#### Scenario: Read error
- **WHEN** PTY reader 返回错误
- **THEN** 停止读取并关闭发送端，不通过通道传递错误原因。

#### Scenario: Zero drain budget
- **WHEN** 通道已有数据但 timeout 为零
- **THEN** 返回空块列表，保留排队数据供后续读取。

证据：`crates/codegen/pager-pty-harness/src/pty.rs` — `fn spawn_reader`；`crates/codegen/pager-pty-harness/src/pty.rs` — `pub fn drain_output`；`crates/codegen/pager-pty-harness/src/pty.rs` — `pub fn inject_keys`。

### Requirement: PTY child exit observation and cached status
PtyController SHALL 区分 Running、PendingStatus 和 Exited(code)，查询错误传播；is_running 仅对 Running 为 true，wait_exit_code 在 PendingStatus 或 Exited 时立即返回，Running 等待到 deadline。Unix 先无 reap 观察退出并清理后代，再取 portable-pty 状态；ECHILD 仅在已观察退出或 kill 可能已 reap 时进入缓存恢复，无缓存则报错。取得状态后缓存一次并清除 PID，后续查询使用缓存。child_pid 在已观察退出时即隐藏 PID，send_signal 仅使用可用 PID。

#### Scenario: Exit observed without status
- **WHEN** 已观察子进程退出但 try_wait 尚无状态
- **THEN** poll 返回 PendingStatus，is_running=false，child_pid=None，不伪造退出码。

#### Scenario: Unknown ECHILD
- **WHEN** 未观察退出且未调用可能 reap 的 kill 时观察返回 ECHILD
- **THEN** 传播错误，不把它推断为正常退出。

证据：`crates/codegen/pager-pty-harness/src/pty.rs` — `pub fn poll_exit_code`；`crates/codegen/pager-pty-harness/src/pty.rs` — `pub fn wait_exit_code`；`crates/codegen/pager-pty-harness/src/pty.rs` — `fn poll_exit_status`；`crates/codegen/pager-pty-harness/src/pty.rs` — `fn observe_exit_before_reap`。

### Requirement: PTY quit and drop bounded cleanup
quit SHALL 尽力发送 q，轮询最多 5 秒且 PendingStatus 也视为完成；仍 Running 时清理后代、kill direct child，再以 1 秒预算等待实际状态。Drop 在未缓存退出状态时尽力清理后代、kill 并以 250ms 预算回收，忽略错误，最后释放 process tree。回收轮询间隔 10ms；预算不构成 kill/底层系统调用的硬超时保证。

#### Scenario: Pending status on quit
- **WHEN** quit 轮询得到 PendingStatus
- **THEN** 返回成功，不等待可用退出码。

#### Scenario: Drop cannot reap
- **WHEN** 250ms 回收预算耗尽或底层查询报错
- **THEN** Drop 忽略错误，不 panic，也不声称已经获得退出码。

证据：`crates/codegen/pager-pty-harness/src/pty.rs` — `pub fn quit`；`crates/codegen/pager-pty-harness/src/pty.rs` — `fn wait_child_bounded`；`crates/codegen/pager-pty-harness/src/pty.rs` — `impl Drop for PtyController`。

### Requirement: PTY harness output fanout and reply forwarding
PtyHarness SHALL 初始化独立 ScreenTracker、FrameTimingParser、空 raw_output/cast_events 与关闭的 respond_to_queries。每个 PTY chunk 先追加原始输出并记录相对创建时刻和结束偏移，再 feed screen，随后 feed timing；启用回复转发时排空 emulator 回复并尝试写回 PTY，忽略写回错误。关闭回复转发不排空队列。update 在总预算耗尽、读取超时或通道关闭时停止；feed_screen 仅修改 emulator，不写子进程、不记 raw/cast 或 timing。resize 先调整真实 PTY，成功后才改 emulator。

#### Scenario: Queries while forwarding disabled
- **WHEN** 关闭回复转发时解析产生查询回复
- **THEN** 回复保留队列，后续启用且处理新 chunk 时可能连同新回复一起发送。

#### Scenario: Screen-only repaint
- **WHEN** 调用 feed_screen 模拟外部重绘
- **THEN** 屏幕状态改变，但原始录制和帧计时不包含这些直接注入字节。

#### Scenario: PTY resize failure
- **WHEN** 底层 PTY resize 返回错误
- **THEN** 传播错误且不更新 emulator 尺寸。

证据：`crates/codegen/pager-pty-harness/src/lib.rs` — `fn from_pty`；`crates/codegen/pager-pty-harness/src/lib.rs` — `fn pump_one`；`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn feed_screen`；`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn resize`。

### Requirement: PTY condition stability and idle observation
wait_until SHALL 在首次读取前先判断 condition，之后以最多 50ms 的单次读取推进到满足或总超时；wait_until_stable 要求 condition 连续成立达到 hold，任一观察为 false 重置计时，达到与保持共用总 timeout。通道关闭报错；关闭诊断文字写 process running:false，但该分支未查询实际进程状态。wait_for_turn_idle 使用 250ms 稳定窗口，仅判断可见屏幕不含 Ctrl+c:cancel、Waiting for response、Responding，不验证 durable completion。

#### Scenario: Initially satisfied condition
- **WHEN** condition 初始为 true 且 timeout=0
- **THEN** 普通 wait_until 直接成功，不先 pump 或检查 deadline。

#### Scenario: Transient idle observation
- **WHEN** 三个状态文本曾消失但 250ms 内再次出现
- **THEN** 重置稳定窗口继续等待。

证据：`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn wait_until`；`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn wait_until_stable`；`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn wait_for_turn_idle`。

### Requirement: PTY cast capture geometry and byte decoding
write_cast SHALL 创建父目录并直接覆盖 asciinema v2 JSONL 文件，header 使用创建时 cols/rows，仅输出收到的 PTY chunk 对应 o 事件，不输出 resize/input 事件。每个事件以已记录时间和 raw_output 偏移切片，对跨 chunk 的 UTF-8 continuation 边界向前退避并把未完成字符留给后续事件，最终残缺字节 lossy 解码；无数据的切片跳过。录制保存的是 harness 已接收的输出，不包含 feed_screen 直接注入。

#### Scenario: Resized terminal
- **WHEN** 录制中调用 resize
- **THEN** cast header 仍为初始尺寸且没有 r 事件，不能精确重现改变后的几何。

#### Scenario: Character split between reads
- **WHEN** 一个多字节字符被两个 PTY chunk 分开
- **THEN** 前一输出事件边界回退，完整字符由后续事件携带。

证据：`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn write_cast`。

### Requirement: PTY exit drain preserves known code with separate budgets
wait_for_exit_and_drain SHALL 先轮询直到取得 Exited(code)，期间以最多 50ms update 推进输出；PendingStatus 不视为成功，退出预算耗尽时分别报告状态不可用或仍运行。取得 code 后启用独立 drain_timeout，继续 pump 到通道关闭、连续 200ms 无新输出或 drain 预算耗尽，均返回已知 code。排空阶段超时不报错，也不保证收到了 EOF 或全部后续输出。reset_timing 只重置 timing，不清除屏幕、原始输出或 cast 事件。

#### Scenario: Pending status reaches exit deadline
- **WHEN** 退出已观察到但始终没有退出码
- **THEN** 返回状态不可用错误，不进入成功 drain 阶段。

#### Scenario: No drain budget
- **WHEN** 已取得退出码且 drain_timeout=0
- **THEN** 立即返回该码，不先收取排队尾部输出。

#### Scenario: Quiet before EOF
- **WHEN** drain 中连续 200ms 无输出且发送端尚未关闭
- **THEN** 返回已知退出码，不能据此断言 EOF 已发生。

证据：`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn wait_for_exit_and_drain`；`crates/codegen/pager-pty-harness/src/lib.rs` — `pub fn reset_timing`。

### Requirement: PTY content controller defaults and explicit config seed
ContentController.start SHALL 使用 test-model 创建 mock server；start_with_models 将 settings 设为 empty preset、响应设为 default_response_text，并建立指向 server URL 的 TestSandbox，设置 GROW_PROMPT_SUGGESTIONS=false。seed_llm_config 是独立显式操作，创建 home/.grow 并直接覆盖 config.toml，配置默认 mock/mock、chat_completions backend、当前 server base_url 和 128000 context_window；启动 controller 不自行调用该方法。

#### Scenario: Controller startup only
- **WHEN** 只调用 start 或 start_with_models
- **THEN** 建立 server 与 sandbox 默认环境，不由该入口调用 seed_llm_config。

#### Scenario: Existing config
- **WHEN** 调用 seed_llm_config 时 config.toml 已存在
- **THEN** 直接覆盖成 mock provider 配置，不合并用户字段。

证据：`crates/codegen/pager-pty-harness/src/content.rs` — `pub async fn start_with_models`；`crates/codegen/pager-pty-harness/src/content.rs` — `pub fn seed_llm_config`。

### Requirement: PTY logical turn expectation endpoint alternatives
AgentTurnExpectation SHALL 维护 Responses 与 Chat Completions 两个 expectation；wait_received、wait_blocked、wait_satisfied 使用 select 等待任一对应事件，release 释放两者。is_satisfied 为任一完成，assert_satisfied 在两者均未完成时 panic；diagnostic 汇总两者，unsatisfied_diagnostics 返回所有尚未 satisfied 的项，不限于从未被 claim 的项。该包装层没有自行设置 timeout。

#### Scenario: Only one endpoint completes
- **WHEN** Responses 完成而 Chat Completions 未完成
- **THEN** 逻辑 turn satisfied，未完成 endpoint 仍可出现在 unsatisfied_diagnostics。

#### Scenario: Release logical barrier
- **WHEN** 调用 release
- **THEN** 对两种 endpoint expectation 都执行 release，不只操作先收到请求者。

证据：`crates/codegen/pager-pty-harness/src/content.rs` — `impl AgentTurnExpectation`。

### Requirement: PTY foreground turn registration matching scope
expect_agent_turn SHALL 为 Responses 和 ChatCompletions 分别注册 foreground matcher，名称追加 endpoint 后缀，使用 test-model 的 exact SSE script 输出调用方 text；text 是响应内容，不是输入匹配条件。blocked 版本为两种注册选择 expect_response_blocked；with_responses 允许分别提供脚本。包装层不额外绑定 prompt 内容、session ID 或具体 request ID；需要更窄匹配时调用 expect_response/expect_response_blocked 传入自定义 matcher。has_chat_completion 对任一 ChatCompletions 或 Responses 请求均可返回 true。

#### Scenario: Response text differs from prompt
- **WHEN** 注册 text=MATCHED_TURN 而请求用户内容为 hello
- **THEN** 该包装层不因输入与 text 不同而拒绝，匹配由 foreground matcher 决定。

#### Scenario: Endpoint custom scripts
- **WHEN** 调用 expect_agent_turn_with_responses
- **THEN** 两个 endpoint 分别使用传入脚本，不强制相同响应内容。

证据：`crates/codegen/pager-pty-harness/src/content.rs` — `fn expect_agent_turn_with_responses_inner`；`crates/codegen/pager-pty-harness/src/content.rs` — `pub fn expect_agent_turn`；`crates/codegen/pager-pty-harness/src/content.rs` — `pub fn has_chat_completion`。

### Requirement: PTY benchmark scenario registry
Scenario SHALL 依次枚举并分发 ScrollStress、StreamingRender、ResizeStorm、LargeCodeblock、IdleCost、MixedInteraction，as_str 和 serde 使用对应 snake_case 名称；empty_enter_send_now 与 plan_approval_resume 虽作为模块公开，但不在 Scenario::ALL 中。wait_for_welcome 仅等待可见文本 Quit 最多 15 秒，不验证具体欢迎页面身份。

#### Scenario: Benchmark enumeration
- **WHEN** 遍历 Scenario::ALL
- **THEN** 得到六项基准，不能据此认为两个独立回归场景也被执行。

证据：`crates/codegen/pager-pty-harness/src/scenarios/mod.rs` — `pub enum Scenario`；`crates/codegen/pager-pty-harness/src/scenarios/mod.rs` — `pub const ALL`；`crates/codegen/pager-pty-harness/src/scenarios/mod.rs` — `pub(crate) async fn wait_for_welcome`。

### Requirement: PTY idle cost workload measures welcome frames
idle_cost.run SHALL 等待 Quit 文本，再 update 1 秒并 reset_timing，在最多约 3 秒窗口每次 update 100ms，若子进程非运行中则提前结束，最后返回 idle_cost 的帧统计。该场景不提交 prompt、不使用 ContentController、不采样 CPU，也不自行断言零帧或进程存活。

#### Scenario: Idle child exits
- **WHEN** 测量窗口内 is_running 返回 false
- **THEN** 提前结束并返回已收集统计，不将退出自动判为场景失败。

#### Scenario: Frames observed during idle
- **WHEN** 计时窗口收到了同步帧
- **THEN** 计入结果，不在该函数中以帧数非零返回错误。

证据：`crates/codegen/pager-pty-harness/src/scenarios/idle_cost.rs` — `pub async fn run`。

### Requirement: PTY scroll stress workload and readiness proxy
scroll_stress SHALL 配置标题加 500 行 Lorem markdown，等待欢迎页后注入 go 加 CR，最多 30 秒等待 Lorem 可见，再 update 500ms 并重置计时。测量最多 200 次 j 输入，每次 update 16ms，非运行中提前结束但不据此报错；末尾 update 250ms 计入 wall_time。该场景不核验全文完成、输入焦点、实际滚动距离或最终文本正确性。

#### Scenario: First response token visible
- **WHEN** 仅 Lorem 已显示而后续响应仍在流式输出
- **THEN** 500ms 等待后即进入测量，没有 durable completion 屏障。

#### Scenario: Child stops during loop
- **WHEN** is_running 返回 false
- **THEN** 退出循环后收集尾部并返回统计，不主动生成退出失败。

证据：`crates/codegen/pager-pty-harness/src/scenarios/scroll_stress.rs` — `pub async fn run`。

### Requirement: PTY resize storm workload and crash checks
resize_storm SHALL 在等待 Quit 后重置计时，执行 25 次 resize，偶数迭代为 35×100、奇数为 55×160（rows×cols），每次 update 40ms 并检查存活；非运行中报错。最后 update 500ms，若当前可见屏幕包含 panicked 则报错，否则返回统计。该场景不提交内容，也不在最终 500ms 后再次查询存活或验证布局尺寸/历史内容。

#### Scenario: Resize iteration child exit
- **WHEN** 某次 resize 后 is_running=false
- **THEN** 返回包含迭代编号的错误。

#### Scenario: Final settling window
- **WHEN** 最后 500ms 后可见屏幕无 panicked
- **THEN** 按当前统计返回，不额外确认进程仍运行。

证据：`crates/codegen/pager-pty-harness/src/scenarios/resize_storm.rs` — `pub async fn run`。

### Requirement: PTY streaming render workload window
streaming_render SHALL 设置 stream-bench 前缀加 word0 至 word399 的响应，等待欢迎页并提交 stream 加 CR，最多 20 秒等待前缀可见，再重置计时，以每次 100ms update 收集约 4 秒帧。非运行中提前退出循环并返回统计；该函数不设置 SSE chunk delay、不检查测量期仍在 streaming，也不要求最小帧数。

#### Scenario: Measurement coverage boundary
- **WHEN** 响应在窗口开始前已传完
- **THEN** 仍收集剩余窗口的帧，不能由场景名称推导测量期间持续流式输出。

证据：`crates/codegen/pager-pty-harness/src/scenarios/streaming_render.rs` — `pub async fn run`。

### Requirement: PTY mixed interaction workload overlap scope
mixed_interaction SHALL 设置 mixed-bench 前缀加 tok0 至 tok399 的响应，等待欢迎页后提交 go 加 CR，最多 20 秒等待前缀，重置计时并最多发送 80 次 j，每次 update 40ms；非运行中停止循环，末尾 update 250ms 计入统计。函数不设置流式延迟或屏障，不验证 j 导致滚动，也不验证输入与 streaming 实际重叠。

#### Scenario: Measurement coverage boundary
- **WHEN** 响应很快结束
- **THEN** 仍发送 j 并报告统计，不因缺少流式/滚动重叠而失败。

证据：`crates/codegen/pager-pty-harness/src/scenarios/mixed_interaction.rs` — `pub async fn run`。

### Requirement: PTY large codeblock workload content and timing
large_codeblock SHALL 设置含 400 个 code_sample_N 单行 Rust 函数的 fenced codeblock，等待欢迎页并提交 code 加 CR，最多 30 秒等待 fn code_sample，update 500ms 后重置计时。最多发送 120 次 j，每次 update 20ms，非运行中停止，末尾 update 250ms 计入结果；不核验全部代码已渲染、语法颜色、wrap 结果或滚动位移。

#### Scenario: Measurement coverage boundary
- **WHEN** 只观察到首个 code_sample 文本
- **THEN** 额外等待 500ms 后开始测量，不使用整块完成断言。

证据：`crates/codegen/pager-pty-harness/src/scenarios/large_codeblock.rs` — `pub async fn run`。

### Requirement: PTY benchmark CLI orchestration and failure output
pty-bench SHALL 默认运行全部六个 Scenario，--scenario 与 --all 互斥；默认终端为 50 rows×120 cols，--binary 直接用于启动，否则调用 pager_binary。每个场景新建 ContentController、显式 seed_llm_config 并启动独立 harness，不额外设置 chunk delay；场景返回后尽力 quit。场景错误被记录成该项全零统计并继续其他场景，JSON 数组输出后返回 1，且不写入或比较基线。控制器/配置/启动等编排错误直接结束 run，由 main 返回 2。全部场景成功后才处理互斥的 write-baseline 或 baseline，退化返回 1，否则 0；隐藏 --bench 仅兼容接收并忽略。

#### Scenario: Scenario fails after spawn
- **WHEN** 一个 scenario.run 返回错误
- **THEN** 尽力 quit、加入零值结果并继续，最终输出 JSON 后以 1 退出，不覆盖基线。

#### Scenario: Spawn preparation fails
- **WHEN** ContentController 启动或配置 seed 失败
- **THEN** 立即传播错误，由 main 返回 2，不保证输出部分结果 JSON。

#### Scenario: All runs successful with baseline write
- **WHEN** 全部 scenario 返回成功且指定 --write-baseline
- **THEN** 先输出 JSON，再保存基线；保存错误仍导致退出 2。

证据：`crates/codegen/pager-pty-harness/benches/pty_bench.rs` — `struct Cli`；`crates/codegen/pager-pty-harness/benches/pty_bench.rs` — `async fn run`；`crates/codegen/pager-pty-harness/benches/pty_bench.rs` — `async fn main`。

### Requirement: PTY queued send now regression assertions
assert_empty_enter_force_sends_top_queued SHALL 创建被终止事件屏障阻塞的 TURNONE 与下一轮 TURNTWO expectation，以 50×120 PTY 启动；提交 go 并等待 TURNONE 和 blocked，再提交 follow-up 加 CR、等待文本可见、注入空 CR 后释放第一屏障。验证带 ❯ 前缀的 follow-up、TURNTWO 与第二 expectation 完成；当前可见屏幕不得含取消标记或 panicked。从记录请求的 messages 或 input 中收集 role=user 内容，选择首个包含 follow-up 的 blob，要求含 <user_query> 且无 interjection preamble。该 helper 未调用 seed_llm_config，也未断言请求数量、队列清空或跨历史全文不存在取消标记。

#### Scenario: Wire prompt shape
- **WHEN** 找到首个包含 follow-up 的 user blob
- **THEN** 缺少 <user_query> 或包含 interjection preamble 均报错。

#### Scenario: Historical cancellation marker
- **WHEN** 取消标记已滚出当前可见屏幕
- **THEN** 本场景的 contains_text 检查不能排除历史中存在该标记。

证据：`crates/codegen/pager-pty-harness/src/scenarios/empty_enter_send_now.rs` — `pub async fn assert_empty_enter_force_sends_top_queued`；`crates/codegen/pager-pty-harness/src/scenarios/empty_enter_send_now.rs` — `fn all_user_message_blobs`。

### Requirement: PTY plan approval resume fixture lifecycle
计划审批恢复场景 SHALL 显式 seed mock LLM 配置，在含 .git 目录的临时 cwd 启动 50×120 pager，完成 setup expectation 后发送两次 Ctrl-Q 并调用 quit，再修改磁盘 fixture；随后同 cwd、同 content 以 --continue 启动，要求 request changes、quit plan、approve 及计划正文或 setup sentinel 可见，按 a 后等待 implementation sentinel 和 expectation 完成。此用例人工写入 AwaitingApproval 状态，不验证实际规划过程生成该状态。

#### Scenario: Restart fixture
- **WHEN** 首次会话退出后调用 seed_parked_approval
- **THEN** 为沙箱下所有两层会话目录写计划 artifact 与控制事件，然后恢复测试。

证据：`crates/codegen/pager-pty-harness/src/scenarios/plan_approval_resume.rs` — `pub async fn assert_plan_approval_restored_after_resume`；`crates/codegen/pager-pty-harness/src/scenarios/plan_approval_resume.rs` — `fn seed_parked_approval`。

### Requirement: PTY plan control fixture ledger validation
append_awaiting_plan_control SHALL 逐行解析非空 Timeline JSON，要求统一整数 version、从零连续 seq，以及最新 control 含整数 revision 和对象 snapshot；保留 snapshot 其他字段，递增 revision（饱和加一且至少一），设置 Plan AwaitingApproval、approval_pending=true、artifact revision=1、正文 BLAKE3 hash 和 last_plan_handoff=null。追加 control 事件包含 behavior/plan_phase synthetic user reminder 和 retired goal_definition；必要时补换行并 sync_all。该工具不验证旧 control revision 单调性，不提供多会话写入事务。

#### Scenario: Malformed timeline
- **WHEN** 存在无效 JSON、非连续 seq 或混合 version
- **THEN** 返回错误，不按容错扫描忽略损坏行。

#### Scenario: Existing unrelated snapshot fields
- **WHEN** snapshot 含 Goal 或其他未修改字段
- **THEN** 克隆保留这些字段，仅覆盖所列控制与行为字段。

证据：`crates/codegen/pager-pty-harness/src/scenarios/plan_approval_resume.rs` — `fn append_awaiting_plan_control`；`crates/codegen/pager-pty-harness/src/scenarios/plan_approval_resume.rs` — `fn system_reminder_json`。

### Requirement: PTY prompt durability quit regression scope
prompt_history_durable_quit 测试 SHALL 默认 ignored，以独立 mock/home 和含 .git 的临时 cwd 启动，提交 canary、等待 ACK 后固定 update 1 秒。双 Ctrl-C（间隔 250ms）与 Unix 真实 SIGINT 分别要求 Exited(0)、退出前记录偏移之后出现 show-cursor 序列，并在任一 timeline 找到 type=turn、state=started、prompt_text 精确匹配、input_kind=prompt、identity.origin=user 的事件。双 Ctrl-C 另以 --continue 恢复，要求 canary 可见及 Up 后仍可见、无 panicked；SIGINT 分支不做重启。测试未显式 seed_llm_config，未检验事件唯一性、完整终端模式恢复或 ACK 后真实 idle。

#### Scenario: Durable record match
- **WHEN** 任一会话 Timeline 含匹配的 typed user started turn
- **THEN** 持久化断言成功，不要求唯一记录或指定会话身份。

#### Scenario: Terminal restoration marker
- **WHEN** 退出后 raw suffix 含 ESC[?25h
- **THEN** 标记检查成功，但不验证 raw mode、alt screen 或其他终端状态。

证据：`crates/codegen/pager-pty-harness/tests/prompt_history_durable_quit.rs` — `async fn run`；`crates/codegen/pager-pty-harness/tests/prompt_history_durable_quit.rs` — `async fn run_sigint`；`crates/codegen/pager-pty-harness/tests/prompt_history_durable_quit.rs` — `fn assert_prompt_durable`；`crates/codegen/pager-pty-harness/tests/prompt_history_durable_quit.rs` — `fn terminal_restored`。

### Requirement: PTY curated matrix test selection and serialization
scroll_matrix_curated SHALL 为八个固定 curated cell 提供非 ignored tokio 测试，全部要求 CellStatus::Pass，包括名称仍含 xfail 的 c1_auto_g4_jerk_xfail。执行前查找 cell（缺失 panic）并解析 binary，然后获取进程内 Tokio mutex 再运行 cell；该锁不覆盖 binary 解析或不同测试进程。产物目录采用 TEST_TMPDIR 或系统 temp 下 scroll-matrix-curated，保留诊断。额外测试要求 curated ID 列表与硬编码八项列表按顺序完全相同。

#### Scenario: Historical xfail identifier
- **WHEN** 运行 c1_auto_g4_jerk_xfail
- **THEN** 要求 Pass，不接受 Xfail 作为该测试通过结果。

#### Scenario: Concurrent test processes
- **WHEN** 多个 cargo 测试进程没有独立 TEST_TMPDIR
- **THEN** 进程内 mutex 不隔离共享产物目录，不保证跨进程无冲突。

证据：`crates/codegen/pager-pty-harness/tests/scroll_matrix_curated.rs` — `async fn run_curated_cell`；`crates/codegen/pager-pty-harness/tests/scroll_matrix_curated.rs` — `async fn assert_cell_passes`；`crates/codegen/pager-pty-harness/tests/scroll_matrix_curated.rs` — `fn artifacts_dir`；`crates/codegen/pager-pty-harness/tests/scroll_matrix_curated.rs` — `fn curated_cells_all_have_a_test`。

### Requirement: Scroll matrix report encoding and exit policy
矩阵报告 SHALL 将状态序列化为 pass/fail/x_fail/x_pass，表格显示 PASS/FAIL/XFAIL/XPASS；exit_code 仅检查 cell.status，任一 Fail 或 XPass 返回 1，否则含空列表均返回 0，不重新计算 invariant 结果。write_report_json 创建目录并直接覆盖 report.json 数组，None note/detail 省略。表格 detail 优先 note，否则第一个非 Pass invariant，CR/LF 换为空格并按 UTF-8 安全边界截到最多 72 字节（含 ...）；完整文本保留 JSON。该函数不清除任意 ANSI 控制字符，列宽按字节长度计算，不保证 Unicode 显示列对齐。

#### Scenario: Empty report collection
- **WHEN** 没有 cell report
- **THEN** exit_code 返回 0，不证明执行过任何测试。

#### Scenario: Expected failure versus unexpected pass
- **WHEN** 所有 cell 为 Pass/XFail 或其中出现 XPass
- **THEN** 前者退出 0，后者退出 1。

#### Scenario: Cell status contradicts invariant row
- **WHEN** 调用方传入 Pass cell 但 invariant 包含 Fail
- **THEN** exit_code 仍按 cell.status 判断，不验证一致性。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/report.rs` — `pub fn exit_code`；`crates/codegen/pager-pty-harness/src/scroll_matrix/report.rs` — `pub fn write_report_json`；`crates/codegen/pager-pty-harness/src/scroll_matrix/report.rs` — `pub fn summary_table`；`crates/codegen/pager-pty-harness/src/scroll_matrix/report.rs` — `fn detail_for`。

### Requirement: Scroll matrix JSONL schema and stream grouping
parse_jsonl_str SHALL 对每个 lines() 行执行完整 ScrollLogLine 反序列化，不跳过空白行；缺失必填字段或错误 JSON 导致整个解析失败并包含 1-based 行号，未知字段允许。group_streams 仅接受 stream_start/flush/finalize，拒绝无 start 的 flush/finalize、未结束时再次 start 和未知 evt，允许最后一个未 finalize 的 stream；不验证 ts 单调性。intra_stream_flush_spacings_ms 跳过首个 flush-bearing record，再收集存在的 spacing。

#### Scenario: Blank interior line
- **WHEN** JSONL 中间存在空白行
- **THEN** 解析失败而非跳过该行。

#### Scenario: Unfinished final stream
- **WHEN** 最后一组已有 start 但无 finalize
- **THEN** 保留 finalize=None 的尾组。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/log.rs` — `pub fn parse_jsonl_str`；`crates/codegen/pager-pty-harness/src/scroll_matrix/log.rs` — `pub fn group_streams`；`crates/codegen/pager-pty-harness/src/scroll_matrix/log.rs` — `pub fn intra_stream_flush_spacings_ms`。

### Requirement: Scroll matrix finalize wait raw marker scope
wait_for_finalize_count SHALL 每 10ms 左右重新读文件，以精确子串 "evt":"finalize" 出现次数达到 n 为成功，先计数再检查 deadline；文件不存在视为零，其他读取或 UTF-8 错误传播。该检查不解析 JSON、不要求完整行或正确流结构；达到计数不保证后续尾部已经静止。

#### Scenario: Whitespace formatted event
- **WHEN** 有效 JSON 使用 evt 键与值之间的额外空格
- **THEN** 精确子串不匹配，该 finalize 不计数。

#### Scenario: Partial final record
- **WHEN** 不完整 JSON 尾部已包含完整标记子串
- **THEN** 该子串仍计数，成功不代表整文件可解析。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/log.rs` — `pub fn wait_for_finalize_count`；`crates/codegen/pager-pty-harness/src/scroll_matrix/log.rs` — `fn count_finalize_lines`。

### Requirement: Scroll matrix blocking cell timeout and failure reports
run_cell SHALL 创建产物目录后以 spawn_blocking 执行 cell body，通过 runtime Handle.block_on 驱动，并用 60 秒 timeout 等待。目录、任务、body 错误与超时生成 Fail、空 invariants、streams=0 和 note；超时不取消 blocking task。函数返回 CellReport，不自行写 report.json；同步目录操作和任务最终清理不受此等待预算保证。

#### Scenario: Cell timeout
- **WHEN** body 超过 60 秒
- **THEN** 返回失败报告，但后台任务可能继续运行，不能据此认定资源已释放。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/runner.rs` — `pub async fn run_cell`。

### Requirement: Scroll matrix quiet window and teardown before verdict
矩阵 cell SHALL 在 finalize 计数满足后 update 300ms、reset_timing，再 update 500ms 记录 quiet_frames 与屏幕 marker。I-QUIET 仅在帧数大于 2 时失败。若有 streaming expectation，先 release，以最多约 30 秒等待可见 Responding 消失，再以独立 30 秒等待 expectation satisfied；之后 quit、drop content，再解析日志并按 cell 声明的 invariants 顺序判断。任一前置或 teardown 错误阻止 invariant 评估。

#### Scenario: Quiet threshold
- **WHEN** 500ms 窗口记录恰好两帧
- **THEN** I-QUIET 通过，零帧不是必须条件。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/runner.rs` — `async fn run_cell_inner`。

### Requirement: Scroll matrix screen travel simulation boundary
I-SCREEN SHALL 拒绝 Streaming session 或缺失末尾可见 marker；其他情况取每个 stream 最后 flush-bearing record 的 applied_total（无记录为零）作为位移，从零开始逐流累加且每步 min(0)，最后才以 -baseline 做顶部限制，要求 baseline 加该位移精确等于末尾 marker。该模拟按 stream 总量而非逐 flush，顶部限制不在每步执行。

#### Scenario: Top clamp applied at end
- **WHEN** baseline=30，两个 stream 位移为 -500、+10
- **THEN** 模拟最终仍返回 -30；不是每步顶部限制后的 -20。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/runner.rs` — `fn check_screen`。

### Requirement: Scroll matrix expected failure classification precedence
classify SHALL 按 outcomes 原序生成行，Pass 且不在 xfail 为 Pass，Pass 且在 xfail 为 XPass，Violated 分别为 Fail 或 XFail；cell 状态优先级为 Fail、XPass、XFail、Pass。空 outcomes 返回 Pass，xfail 中不存在于 outcomes 的 ID 不产生独立行或错误。

#### Scenario: Unevaluated expected failure
- **WHEN** xfail 含某 ID 但 outcomes 无该 ID
- **THEN** 不自动产生 XPass 或缺失评估错误，最终状态由现有行决定。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/runner.rs` — `fn classify`。

### Requirement: Scroll matrix gesture dispatch and declared stream counts
GestureId SHALL 提供 12 种手势；G1/G2/G8/G11 仅按 ept>=2 选择固定三事件每刻度表，否则选择单事件表，不按 ept 数值动态生成。G8 复用 G2 表，流式差别由 session 提供；其余手势使用固定表。expected_streams 对 G6/G7/G11 返回 2，其余返回 1，是预设计数而非运行观测。direction_counts 将 button=SGR_SCROLL_UP 计为 up，其他所有值计为 down，不验证 button 合法性。

#### Scenario: Nonstandard ept
- **WHEN** 对 G1 传入 ept=2 或 ept=5
- **THEN** 均返回三事件表；ept=0 返回单事件表。

#### Scenario: Unexpected button code
- **WHEN** direction_counts 输入含非上下滚动按钮
- **THEN** 非 up 项仍计入 down。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/gestures.rs` — `pub fn steps`；`crates/codegen/pager-pty-harness/src/scroll_matrix/gestures.rs` — `pub fn expected_streams`；`crates/codegen/pager-pty-harness/src/scroll_matrix/gestures.rs` — `pub fn direction_counts`。

### Requirement: Scroll matrix representative cell profiles and tiers

CELLS SHALL 定义 25 个代表性组合，其中 8 个 Curated、17 个 Full；curated() 仅按 tier 过滤并保留表顺序，不构造终端、配置和手势的笛卡尔积。默认 C1 的 ept/wheel_lpt/trackpad_lpt 为 3/3/3，iTerm C2 为 1/1/3，zed C3 为 1/3/3，vscode C4 为 1/3/15，TMUX C5 为 1/1/3；默认 auto、不反转、speed=1。speed100 行预期 multiplier=6；lines1 同时覆盖两种 lpt。TMUX 环境标记只模拟配置选择，不启动真实 multiplexer。

#### Scenario: Curated subset
- **WHEN** 调用 curated()
- **THEN** 返回表中八个 Curated 行，保留声明顺序；Full 行不在结果中。

#### Scenario: Multiplexer representative
- **WHEN** 使用 c5_tmux_g9b 行
- **THEN** 传入 TMUX 标记并回放 G9b，预期保守配置；该行不证明真实 tmux 转发行为。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `pub const CELLS`；`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `pub fn curated`。

### Requirement: Scroll matrix declared invariant and fixture constraints

当前 cell 构造 SHALL 将所有行的 xfail 初始化为空；名称含 jerk_xfail 的历史行仍要求 SmoothCoast 与 NoDrop 普通通过。表内 forced wheel 使用 ConsW，其他模式使用 ConsA；仅 G8 使用 Streaming，仅 G7 使用 BottomPinned，其他使用 Settled。单元测试检查唯一 ID、指定 curated 顺序、xfail 子集、预期配置的本地重推导、invariant 去重及模式/mux/session 对应关系；本地重推导复制规则，不直接调用生产配置实现，因此不能单独证明生产配置同步。

#### Scenario: Historical xfail name
- **WHEN** 执行 c1_auto_g4_jerk_xfail
- **THEN** 其 xfail 为空，SmoothCoast 和 NoDrop 违反时仍属于普通失败。

#### Scenario: Forced trackpad consistency
- **WHEN** 选用 c1_trackpad_g3_flood
- **THEN** 使用 AUTO_QUIET，其中包含 ConsA 而非 ConsW。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `const fn cell`；`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `fn invariant_lists_are_coherent`；`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `fn expected_profiles_agree_with_env`；`crates/codegen/pager-pty-harness/src/scroll_matrix/cells.rs` — `fn sessions_and_gestures_pair_correctly`。

### Requirement: Scroll matrix marker parsing boundary

marker_line SHALL 将序号格式化为至少四位十进制，marker_response 将 count 个 marker 放入带 MOCKRESPONSE 的代码围栏。屏幕行定位用子串匹配；topmost_marker_in 按屏幕从上到下取每行首个 MARKER- 后恰好四字节解析，跳过失败行，不验证后续字符或序号上界。

#### Scenario: Five digit marker
- **WHEN** 屏幕包含 MARKER-10000
- **THEN** topmost_marker_in 解析四位前缀为 1000，不返回 10000。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `pub fn marker_line`；`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `fn topmost_marker_in`。

### Requirement: Scroll matrix settled session preparation

Settled 与 BottomPinned SHALL 使用同一 preamble：启动 content、设置 marker 响应、显式 seed_llm_config，建立 50×120 sandbox PTY，等待 Quit 最多20秒后提交 go。等待最后 marker 最多60秒再更新500ms；要求 marker0 不可见且另一个 marker 可见，取 baseline 后 Tab、更新500ms、尽力等待 Space:prompt 最多5秒，再更新300ms并重置 timing。该检查不直接读取真实滚动偏移或持久化完成状态；marker_count 必须适合 marker_count-1 及可滚动前置，不提供零值错误返回。

#### Scenario: Equivalent session kinds
- **WHEN** 请求 Settled 或 BottomPinned
- **THEN** 执行相同构造，返回 content、baseline 和无 blocked_turn；调用者需保持 content 存活。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `pub async fn spawn_marker_session`；`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `fn spawn_pager`；`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `pub async fn spawn_settled_marker_session`。

### Requirement: Scroll matrix streaming session preparation boundary

Streaming SHALL 设置分块延迟并注册 blocked turn，将 marker 围栏后连接 TAIL 序列和 STREAMDONE；默认240个 tail word、每块30ms。等待最后 marker 最多30秒并更新300ms后，要求屏幕不含 STREAMDONE 且存在可滚动 baseline，返回仍由调用者释放的 expectation。该前置不检查剩余块数或保证整个手势期间持续传输；默认时间窗口测试仅比较手势声明延迟与尾部预算。

#### Scenario: Held completion
- **WHEN** Streaming 构造返回
- **THEN** 携带 Some(expectation)，调用者负责释放；屏幕未见 STREAMDONE 只证明该次屏幕检查未匹配标记。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `pub async fn spawn_streaming_marker_session`；`crates/codegen/pager-pty-harness/src/scroll_matrix/session.rs` — `fn streaming_window_covers_every_gesture_table`。

### Requirement: Scroll log invariant routing and absent finalize scope

check_log_invariant SHALL 路由12个日志判定；Screen 与 Quiet 传入时 panic。记录顺序为各 group 的 start、flushes、finalize。需要 finalize 的 DropEq、ConsW、ConsA、MuxNoOver 和 NoDrop 跳过无 finalize 的 group；没有通用非空或完成性判定。ConsW/ConsA 仍先验证模式。NoDrop 将缺失 dropped 当0，而 DropEq 要求 dropped 存在且等于 backlog_after。

#### Scenario: Missing dropped field
- **WHEN** 带 finalize 的记录缺少 dropped
- **THEN** NoDrop 按0判定，DropEq 返回违反；调用方须组合使用判定。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `pub fn check_log_invariant`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_drop_eq`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_no_drop`。

### Requirement: Scroll log cadence and carry tolerances

日志判定 SHALL 允许相等时间戳，限制每条记录 abs(flushed)<=cap。Cadence 跳过每流首个 flush-bearing、promotion 和 finalize，只对存在 spacing 的其余记录要求至少15ms。Accel 范围容差为0.01，允许0.99至3.01，存在 avg 时要求至少5.99ms。Carry 要求全部记录 abs(carry)<1，并在前一流 wheel finalize 后要求下一 start 的 abs(carry)<=0.01；不单独要求 wheel finalize 自身 carry 为0。

#### Scenario: Missing spacing
- **WHEN** 非首个普通 flush-bearing 记录没有 spacing
- **THEN** Cadence 不因字段缺失失败；该通过不证明实际发送频率。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_cadence`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_accel`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_carry`。

### Requirement: Scroll log pricing bounds and coast measurement

ConsW SHALL 用 events*wheel_lpt/max(ept,1)*speed 截断，有事件时至少1行，无事件0行，与 abs(applied_total+dropped_or_zero) 比较允许差1。ConsA 使用 trackpad_lpt/3，forced trackpad 仅取该速率至3倍，auto 与 wheel 速率取包络；desired 绝对值需在事件定价范围±1内，delivered 仅检查上界。MuxNoOver 上界为 events*speed+1。SmoothCoast 汇总 events_since_flush=0 的 flush-bearing 绝对 flushed，与该流全部 flush-bearing 的最大 cap 比较，无此记录时 cap=i64::MAX；这些判定不直接验证屏幕移动。

#### Scenario: Changing cap
- **WHEN** 一流不同 flush-bearing 记录的 cap 不同
- **THEN** SmoothCoast 使用最大 cap，而非起始、最终或每条记录各自的 cap。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_cons_w`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_cons_a`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_mux_no_over`；`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_smooth_coast`。

### Requirement: Scroll log configuration echo validation

Cfg SHALL 在每个 stream_start 比较 mode/ept/wheel_lpt/trackpad_lpt/invert 的 Some 值完全相等，speed 必须存在且与预期误差不超过0.01。不验证 viewport_height，也不验证后续 flush/finalize 的配置 echo。

#### Scenario: Absent configuration
- **WHEN** stream_start 缺少任一被检查配置字段
- **THEN** 返回违反；空 groups 没有 start 可查而通过。

证据：`crates/codegen/pager-pty-harness/src/scroll_matrix/invariants.rs` — `fn check_cfg`。

### Requirement: Scroll matrix command selection and report lifecycle

scroll-matrix SHALL 默认 curated、jobs=1、artifacts=target/scroll-matrix；full 包含全部行，再按 ID 大小写敏感子串过滤，空结果报错。先解析/检查 binary 存在，再筛选；无显式 binary 时调用 pager_binary，可能触发其隐式构建。将 artifacts 相对路径转换为调用 cwd 下绝对路径；全部执行后先输出表，再写 report.json，最后按 cell 状态决定退出码。run 返回错误时 main 输出错误并返回2，包含报告写入失败；不把存在性检查当作可执行性验证。

#### Scenario: Empty filtered result
- **WHEN** filter 不匹配任何 cell
- **THEN** 可能已执行 binary 解析后才报错，退出2，不生成本次矩阵报告。

证据：`crates/codegen/pager-pty-harness/src/bin/scroll_matrix.rs` — `async fn run()`。

### Requirement: Scroll matrix command concurrency and ordering

scroll-matrix SHALL 将 jobs=0 归一为1；jobs=1 按表顺序逐项等待，普通 Fail 报告不会阻止后续行。jobs>1 为每行创建 async task，并用 semaphore 限制同时持有 permit 的 run_cell 数量，收齐后按原索引排序报告。外层 join 错误使用 expect panic，不转 CellReport；run_cell 超时不取消其 blocking body，因此 permit 数不构成超时后底层存活进程数量的硬上限。

#### Scenario: Parallel completion order
- **WHEN** 并发 cell 的完成顺序不同于表顺序
- **THEN** 报告仍按原始表索引排序。

证据：`crates/codegen/pager-pty-harness/src/bin/scroll_matrix.rs` — `async fn run_cells`。

### Requirement: Scripted PTY command report exit mapping

pty-scenario SHALL 要求 --scenario，默认 artifacts=target/pty-scenarios；先调用 ScriptedScenario::from_file，再选择显式 binary 或 pager_binary 并检查路径存在，将 binary 与 artifacts 交给 ScriptedRunConfig。runner 成功返回后向 stdout 输出格式化完整 JSON；Passed/Skipped 退出0，Failed/Running 退出1，加载、解析binary、runner或最终序列化错误由 main 输出并退出2。CLI 自身不将 artifacts 转绝对路径，路径解释由下层负责。

#### Scenario: Skipped run
- **WHEN** runner 返回 Skipped 报告
- **THEN** 仍输出最终 JSON 并成功退出。

证据：`crates/codegen/pager-pty-harness/src/bin/pty_scenario.rs` — `async fn run()`。

### Requirement: Scripted scenario decoding and validation boundary

ScriptedScenario SHALL 按小写 yaml/yml 扩展名选择 YAML，其他扩展名或无扩展名选择 JSON；from_file 只读取/反序列化，不调用 validate。run 才拒绝空白 name 或空 steps，未在此校验终端尺寸、名称唯一性或动作参数。terminal 默认50×120且 respond_to_queries=false；os/arch 过滤为空时不限，否则分别精确匹配当前 OS/ARCH 且两条件同时成立。结构未声明 deny_unknown_fields。

#### Scenario: Uppercase extension
- **WHEN** 通过 from_file 加载 .YAML 文件
- **THEN** 选择 JSON 解码，不按内容探测 YAML。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `pub fn from_file`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn validate`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn matches_current`。

### Requirement: Scripted run preparation and skip ordering

ScriptedScenarioRunner SHALL 先验证场景，再按 artifact_dir/safe_name(name)/epoch_ms 创建运行目录并准备图片 fixture，然后检查平台过滤。不匹配时写 Skipped 报告返回；fixture 失败可先于 skip 返回错误。匹配时启动 ContentController，设置默认响应并按顺序注册 mock.turns；仅当 config_toml 存在才写入隔离 home/.grow/config.toml，不调用 seed_llm_config、不合并默认模型配置。创建可选 workspace 后用声明尺寸/env/args 启动 sandbox PTY，再设置 query reply 开关。

#### Scenario: Filtered scenario fixture error
- **WHEN** 不匹配平台的场景在准备 fixture 时出错
- **THEN** 返回错误，而非 Skipped；平台过滤不是跳过全部本地准备工作的开关。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `pub async fn run(&self, scenario`。

### Requirement: Scripted run failure capture and completion scope

脚本执行 SHALL 逐项同步运行 step；首个 step 错误记录 Bug 和 Failed outcome，尽力写 raw_output.bin 与 failure 截图，再写报告和 bugs.md，忽略 quit 结果并返回 Failed 报告。完成所有 steps 后检查进程存活，仅状态仍 Running 时用共同10秒预算等待所有 mock.turns，未满足则 Failed；之后 quit 错误被忽略，仍 Running 则改 Passed。正常尾部 raw 写入也尽力而为，报告/bugs.md 写入错误传播。因此 Passed 不证明 quit 成功、raw 产物存在或退出码为0；setup 和部分 I/O 错误直接返回 Err，不保证 Failed 报告。

#### Scenario: Quit failure after successful steps
- **WHEN** steps 成功、进程存活且所有必需 turn 满足，但 quit 返回错误
- **THEN** 该错误被忽略，报告仍可标记 Passed。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `pub async fn run(&self, scenario`。

### Requirement: Scripted ephemeral workspace materialization

materialize_workspace SHALL 创建临时目录，按 BTreeMap 顺序写文本文件并创建父目录；拒绝绝对路径及 ParentDir/RootDir/Prefix 组件，包括含 a/../b 的路径。可选 git_init 在写文件后通过 sandbox.git_command 执行 git init -q，失败携带状态/stderr传播；没有创建提交，临时目录由调用者保持到运行结束。该检查针对测试作者的路径输入，不声明抵抗外部并发文件系统修改。

#### Scenario: Parent component
- **WHEN** workspace.files 包含 a/../b
- **THEN** 拒绝该路径，即使规范化结果仍可能位于临时目录中。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn materialize_workspace`。

### Requirement: Scripted input injection and observation windows

run_step SHALL 对 TypeText/Keys 注入字节后更新100ms，Paste 注入 bracketed paste 后更新250ms；Copy 解析指定keys后更新250ms。直接鼠标单击更新100ms，双/三击及滚动更新150ms，Drag/SelectText共享实现后更新100ms。这些动作成功仅表示注入及固定窗口完成，不自行验证文本已接纳、选择成功或实际滚动。Resize 调用 harness.resize，Wait 仅 drain 指定时长；AssertContains/NotContains 读取当前屏幕，不等待稳定。

#### Scenario: Input step succeeds
- **WHEN** TypeText 返回 Passed
- **THEN** 不隐含用户请求持久化或模型已收到该文本。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn run_step`。

### Requirement: Scripted image clipboard simulation and drop payloads

PasteClipboardImage SHALL 解析单个 fixture 的普通路径并调用 paste_prompt_text，与操作系统剪贴板无交互。PasteImagePaths/FileUrls/EscapedImagePaths 均定位 prompt、点击、粘贴并更新500ms；file URL 直接拼接 file:// 与路径，不百分号编码；Escaped 只为指定字符加反斜杠，不是通用 shell quoting。DropImagesPrompt 在 prompt drop point 点击后 bracketed paste 普通路径并更新500ms。未知 fixture 名报错，多路径用换行连接。

#### Scenario: Clipboard named action
- **WHEN** 执行 PasteClipboardImage
- **THEN** 向PTY粘贴fixture路径，不证明系统图像剪贴板读取路径工作。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn run_step`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn image_path_payload`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn shell_escape_path`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn paste_prompt_text`。

### Requirement: Scripted locator character indexing and selection range

文本定位 SHALL 按可见行从上到下搜索非重叠子串，occurrence从0起，空文本报错；列位置采用Unicode scalar数量而非终端显示宽度，不跨行匹配。prompt定位取自下而上首个含❯的行，该行首个marker前字符数加2；drop point按尾部trim后的字符数计算并限制到该行字符末位。SelectTextRange定位两端首次出现，终点为文本后一列、0改1，再saturating加偏移并限制到screen.cols-1。Point locator不检查坐标边界。

#### Scenario: Overlapping matches
- **WHEN** 在 aaa 中定位 aa 的 occurrence=1
- **THEN** 不匹配重叠的第二个 aa。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn locate_text_impl`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn locate_prompt`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn locate_prompt_drop_point`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn run_step`。

### Requirement: Scripted highlight assertions use dominant background

高亮断言 SHALL 将styled runs按字符展开，以该行数量最多的背景作为基线，同数时按背景键排序取较大者。AssertHighlightRun 从目标首次出现扩展连续非基线区域，要求完整未trim文本等于equals且背景统一；AssertTextNotHighlighted只检查目标字符范围内背景等于基线。因此整行背景统一会被视作未高亮，不能将背景非默认等同于选中。

#### Scenario: Uniform colored row
- **WHEN** 一行所有字符共享非默认背景
- **THEN** 该背景成为基线，不被AssertHighlightRun视为高亮。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn row_char_bgs`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn dominant_bg`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn run_step`。

### Requirement: Scripted screenshot serialization boundary

capture_artifacts SHALL 按两位最小宽度step编号和safe_name组成basename，顺序直接写txt、html、svg、styled JSON，任何一步失败传播且之前文件可已存在。SVG采用每字符8px、每行18px固定近似，画布列数取最长单个run字符数且至少80，并非每行run总长；支持fg/bg/bold/italic/underline并绘制cursor框，不读取光标可见性。截图不是操作系统像素捕获，也不保证宽字符或多run长行精确排版。

#### Scenario: Partially failed capture
- **WHEN** SVG写入失败发生在txt/html之后
- **THEN** 返回错误但已写txt/html保留，不提供原子产物组。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn capture_artifacts`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn render_svg`。

### Requirement: Scripted image fixture bytes and name handling

prepare_image_fixtures SHALL 创建fixtures目录，将name与去掉前导点的extension拼成路径，写入后canonicalize并按原name存入HashMap，重名后项覆盖映射。无论extension均写PNG：Standard为8×8 RGBA(128,64,32,255)，Tiny1x1为1×1(200,100,50,255)，CrcCorruptPng为64×64(10,20,30,255)并翻转首个找到的IDAT CRC首字节。不对name/extension应用workspace的相对路径验证，不创建fixture指定子目录。

#### Scenario: Misleading extension
- **WHEN** Standard fixture扩展名为jpg
- **THEN** 文件内容仍为PNG。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn prepare_image_fixtures`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn standard_png_bytes`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn tiny_1x1_png_bytes`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn crc_corrupt_png_bytes`。

### Requirement: Scripted request image discovery and dimension assertion scope

request_images SHALL 递归遍历全部捕获JSON，将type大小写不敏感等于image、包含小写image子串，或同时存在mime_type/data键的对象计入，继续遍历其子对象，不去重、不限定endpoint/turn。AssertInlineImages要求总数至少min并对所有命中检查inline数据、MIME相等、非空standard base64及推断格式后的into_dimensions结果，尺寸可精确或包含端点的可选上下界。它不完整解码像素、不证明IDAT CRC有效，也不核对声明MIME与检测格式一致；min=0且无图可通过。

#### Scenario: Header-readable corrupt image
- **WHEN** PNG头可读但像素数据损坏
- **THEN** 尺寸检查本身不保证拒绝该数据，不能据通过声明完整图像解码成功。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn collect_images`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn inline_image_data`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn assert_inline_images`。

### Requirement: Scripted request text and temporary file assertion scope

AssertRequestContains SHALL 对捕获body的紧凑JSON序列化文本做子串匹配，未给index搜索全部，给index按最新为0倒序选择，空集合或越界报错；不限定某字段的原始字符串。AssertNoTempArtifacts递归content.home，仅检查普通文件扩展名大小写不敏感等于tmp，跳过symlink、不存在目录视作空，其他I/O错误传播；不证明其他后缀或home以外无临时文件。

#### Scenario: Escaped request text
- **WHEN** 请求字符串值含换行
- **THEN** 在序列化JSON上匹配转义后的文本，而非自动解码字段后匹配。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn assert_request_contains`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn collect_tmp_files`。

### Requirement: Scripted OSC52 raw history decoding

OSC52 断言 SHALL 扫描累计 raw_output，经lossy UTF-8转换按ESC]52;拆分，跳过无第二分号或空payload的片段；payload截止首个BEL或ESC，若无终止符则读取余段，使用standard base64和严格UTF-8解码，任意解码错误使断言报错。正断言要求任意历史payload含文本，负断言要求全部不含，空集合可使负断言通过；不验证剪贴板目标、完整ST或本次Copy因果。

#### Scenario: Unterminated payload
- **WHEN** 累计输出含有效base64 OSC52但尚无终止符
- **THEN** 仍可能解码并匹配，不证明完整终端协议发送成功。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn decode_osc52_payloads`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn run_step`。

### Requirement: Scripted Kitty graphics presence counting boundary

count_kitty_graphics SHALL 扫描累计字节中ESC_G，参数段截至分号或ESC或末尾；参数含a=d或a=q子串则排除，其余计数，不按参数token严格解析、不要求终止符、payload或终端确认。分块传输每个APC独立计数，不能作为图片数量或屏幕成功显示证明。AssertNoKittyGraphics仅要求该计数为0，允许delete/query控制序列。

#### Scenario: Incomplete introducer
- **WHEN** raw_output仅含ESC_G
- **THEN** 计数为1，即使没有图像数据或终止符。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `pub(crate) fn count_kitty_graphics`。

### Requirement: Scripted pointer protocol encoding

鼠标编码 SHALL 将0基坐标加1生成SGR col;row，click连续发送对应按钮M和m，重复点击无间隔连接左键序列；scroll连续发送count个64/65按钮M，不附release或间隔。drag发送左键press、整数中点drag32、终点drag32及release；使用u16加法计算坐标和中点，未做全域输入溢出检查。bracketed_paste直接包裹原文，不转义原文中的终止序列。

#### Scenario: Zero scroll count
- **WHEN** scroll count为0
- **THEN** 生成空字符串，动作不保证发生滚动。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn mouse_click_bytes`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn mouse_scroll_bytes`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn mouse_drag_bytes`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn sgr_mouse`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn bracketed_paste`。

### Requirement: Scripted artifact naming and assertion defaults

safe_name SHALL 保留ASCII字母数字及连字符下划线，其他字符压为连字符并去掉首尾连字符，可返回空串或与其他名称相同；run目录使用毫秒时间，不保证并发唯一。默认wait timeout为15000ms，scroll count和Kitty min为1，Copy keys为y，fixture extension为png，inline MIME为image/png但width/height默认精确2，不跟随8×8 Standard fixture自动调整。

#### Scenario: Default fixture with default dimensions
- **WHEN** 8×8 Standard图片以原尺寸提交且AssertInlineImages省略width/height
- **THEN** 默认2×2尺寸断言不匹配。

证据：`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn safe_name`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn epoch_ms`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn default_image_width`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn default_image_height`；`crates/codegen/pager-pty-harness/src/scripted.rs` — `fn default_copy_keys`。

### Requirement: Paste latency benchmark selection and global clipboard scope

paste_latency SHALL 在运行时拒绝非macOS；默认agent、text/image两模式、10次、50×120。all surfaces按agent后dashboard执行，dashboard image跳过，dashboard错误只警告并丢弃结果，agent错误传播。使用HostClipboardTextGuard尽力恢复原文本，不保存原图片。无结果退出1；有结果输出JSON且可直接写指定文件，不创建父目录；错误由main退出2。iterations=0仍构造cell并返回零统计，不作为空结果拒绝。

#### Scenario: Partial surface failure
- **WHEN** agent成功但dashboard失败
- **THEN** 保留agent结果且可退出0，结果不代表dashboard已验证。

证据：`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `async fn run()`；`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `async fn bench_cell`。

### Requirement: Paste latency measurement start and screen observation

每个cell SHALL 启动content并seed模型配置，复用一个session测量。文本在pbcopy完成后开始计时，注入Ctrl+V至sentinel首次可见，最多10秒；图片每cell预置一次PNG，计时后同次注入Ctrl+V与burst，先等burst最多10秒记录responsiveness，再等Image #最多30秒，以同一起点记录chip。屏幕轮询5ms；图片主p50/p95/max取chip时间，另列responsiveness p50，非终端真实显示或持久化完成测量。统计使用共享percentile，清理/重启时间不计入样本。

#### Scenario: Image chip appears before burst
- **WHEN** burst检查返回时Image #已在屏幕
- **THEN** 随后chip时间才被记录，不保证它等于图像实际首次出现时间。

证据：`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `async fn bench_cell`；`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `fn wait_visible`；`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `fn stats`。

### Requirement: Paste latency surface readiness and clear fallback

spawn_ready SHALL 等Quit最多20秒，go提交后等TURN_SENTINEL最多30秒并更新1秒；dashboard再CSI-u Ctrl-backslash，等+ New Agent最多10秒，更新300ms，出现Tab:input才Tab并等其消失3秒。这些都是屏幕代理条件。每次粘贴后先Ctrl+U等stale消失2秒，再64个Backspace等2秒，agent再Esc、250ms、Esc等2秒，否则忽略quit错误并以同content重启session。清理只检查指定stale子串不可见，不读取完整draft状态；最终quit错误忽略。

#### Scenario: Stale text offscreen
- **WHEN** 清理等待时所有指定stale子串已不可见
- **THEN** 视作清理成功，不证明输入缓冲区为空。

证据：`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `fn spawn_ready`；`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `fn clear_input_or_respawn`；`crates/codegen/pager-pty-harness/benches/paste_latency.rs` — `fn wait_absent`。

### Requirement: PTY scroll correctness mixed input regression scope

scroll_up_from_follow_bottom_then_back_down SHALL 启动配置mock模型的40×100 PTY，提交400行响应，等待底部marker30秒，再以不含Responding且含底部marker或Turn completed的屏幕条件等待45秒并更新400ms。向上连续30次PageUp每次更新35ms，再50个wheel-up；要求屏幕变化、底部marker消失且0..340内至少一marker可见。向下35次PageDown及50个wheel-down后要求不同于上滚截图并含底部marker、340..400 marker或Turn completed。该用例验证键盘与wheel组合的粗粒度位置，不独立证明wheel有效、精确滚动行数、完整内容顺序或性能上限；记录up_wall_ms但不设阈值，quit错误忽略。

#### Scenario: Mixed input evidence
- **WHEN** PageUp和wheel-up组合后断言通过
- **THEN** 只证明组合后的可见结果，不隔离两个输入各自的贡献。

证据：`crates/codegen/pager-pty-harness/tests/scroll_correctness_ptyctl.rs` — `async fn scroll_up_from_follow_bottom_then_back_down`。

### Requirement: Pager in process leader cluster harness lifecycle

PagerLeaderCluster SHALL 仅在Unix的ignored串行测试中，以current-thread Tokio LocalSet启动真实leader server、真实MvpAgent、生产leader bridge与完整AppView reducer/effect循环；mock inference、GROW_HOME、代理URL、API key和固定leader socket均由进程级env guard隔离，cluster持有LeaderLock防止reconnector启动外部leader。server与agent之间使用两个8MiB simplex及逐行JSON桥；socket绑定/释放各最多等待10秒，client connect、initialize、authenticate各有30秒上限，多client pump以10ms tick和60秒总预算推进。kill_leader先取消server并确认旧socket消失，再abort且await全部generation tasks，清除认证状态后才允许同路径respawn；Drop只能best-effort cancel/abort，不能await清理完成。client按terminal=false及nonInteractive/skipGitStatus/skipProjectLayout初始化，cluster每代只在首次需要时认证；reconnect使用unbounded policy。夹具忽略effect返回的quit/meta及task JoinError，不提供terminal/auth handle，因此只覆盖其显式建模的无终端leader路径。

#### Scenario: Old generation socket remains
- **WHEN** 取消leader后10秒内固定socket仍存在
- **THEN** 夹具在启动新代前失败，避免旧代清理删除新代socket。

#### Scenario: Reconnect capable client
- **WHEN** client以reconnect=true连接
- **THEN** bridge获得真实LeaderReconnector、固定socket和unbounded策略，flock迫使它等待并接入in-process server。

证据：`crates/codegen/pager/src/app/leader_cluster/mod.rs` — `PagerLeaderCluster::start / spawn_leader_generation / kill_leader / client / Drop`；`crates/codegen/pager/src/app/leader_cluster/mod.rs` — `ClusterClient::pump_once / process_effects / pump_clients_until`。

### Requirement: Pager leader multi client scenario assertion scope

leader_cluster scenarios SHALL 作为四个Unix-only、ignored、GROW_HOME串行测试存在。双客户端场景验证首轮完成后viewer attach只渲染一次历史，第二轮仍由原driver发送且driver/viewer各只见一次；N-client场景验证三个viewer各自单播replay一次、driver不收到附加replay，随后一个live turn在四端各出现一次。reattach场景轮询updates.jsonl出现turn_completed，断开原driver后要求新client replay一次、状态Idle且inference请求数不增加。leader重启场景取消整代、同socket重建，等待reconnector generation至少1，手工重新initialize/authenticate并按permissionMode=ask及可选cursor执行session/load，replay落地后finish reload，要求旧历史一次且新turn一次。测试以sentinel在AgentMessage文本中的出现次数为代理，不验证全部消息顺序、所有ACP帧、并发提交排序或跨平台传输；文件内四项测试默认不执行，本轮静态审计也未运行它们。

#### Scenario: Durable completed reattach
- **WHEN** 磁盘updates文件已含turn_completed且原driver断开
- **THEN** 新client加载同session后sentinel恰一次、会话Idle，mock inference计数不变。

#### Scenario: Leader generation replacement
- **WHEN** 真实reconnector观察到respawn后的generation至少为1并完成手工reload
- **THEN** 旧sentinel不重复，且新一轮仍可完成并只显示一次。

证据：`crates/codegen/pager/src/app/leader_cluster/scenarios.rs` — `two_clients_share_session_and_stream_both_ways / n_client_fan_out_without_replay_duplication`；`crates/codegen/pager/src/app/leader_cluster/scenarios.rs` — `reattach_completion_roundtrips_durable_log / leader_kill_reconnect_reloads_without_duplicating_history / find_session_updates_file`。

### Requirement: Pager isolated GROW_HOME path integration check
grow_home_paths integration test SHALL set GROW_HOME to a temporary directory before the process-wide OnceLock is read, then assert pager.toml resolution, $GROW_HOME display/abbreviation for memory and copy paths, and positive/negative containment. It does not restore the environment or test canonical/symlink identity.

#### Scenario: Custom root
- **WHEN** GROW_HOME is outside HOME
- **THEN** pager and copy display paths use $GROW_HOME.

#### Scenario: Containment
- **WHEN** memory/MEMORY.md and /tmp/other are checked
- **THEN** only the former is under the configured root.

证据：`crates/codegen/pager/tests/grow_home_paths.rs`。

### Requirement: Pager Mermaid real child ignored integration gates
mermaid_render_subprocess SHALL define four ignored real-binary tests: valid cyclic input requires a decodable nonzero PNG; oversized and invalid input require Err with no PNG; a 1ms timeout requires Err, no PNG and return under 10 seconds. pager_binary resolution is expect-based. Ordinary cargo test skips them and this audit did not opt in.

#### Scenario: Valid child
- **WHEN** the ignored login test runs with a built pager
- **THEN** output is a decodable positive-size PNG.

#### Scenario: Failed source
- **WHEN** source is oversized or invalid
- **THEN** result is Err and output absent.

#### Scenario: Tight timeout
- **WHEN** budget is 1ms
- **THEN** the error returns within the loose 10-second ceiling.

证据：`crates/codegen/pager/tests/mermaid_render_subprocess.rs`。

### Requirement: Pager PTY integration family entrypoint topology
Eight top-level PTY binaries SHALL include shared common support and responsibility-specific path modules: clipboard 11 plus one local test, config 14, minimal 2, persistence 7, queue 20, scroll/selection 28 plus scroll support, shell/tools 16 plus scroll support, and smoke 23. Their comments describe ignored/Bazel execution policy, but entrypoint module lists do not themselves prove child ignore attributes, assertions or results; child files remain separately auditable.

#### Scenario: Queue family
- **WHEN** pty_e2e_queue builds
- **THEN** common support and 20 child modules join that test binary.

#### Scenario: Shared scroll helper
- **WHEN** scroll-selection or shell-tools builds
- **THEN** both common.rs and scroll.rs are included.

#### Scenario: Entrypoint-only audit
- **WHEN** only these eight files are reviewed
- **THEN** child behavior is not inferred from filenames.

证据：`crates/codegen/pager/tests/pty_e2e_{clipboard,config_ui,minimal,persistence,queue,scroll_selection,shell_tools,smoke}.rs`。

### Requirement: Pager unknown SSH clipboard delivery PTY contract
The ignored unknown SSH clipboard test SHALL remove OSC52 sink variables, submit a real response, execute /copy 1 and require the sentinel in newly decoded OSC52 payloads. UI shows Copy sent but neither Copy failed nor Copied; /doctor shows clipboard.delivery-unverified and grow wrap ssh guidance without panic. It was not run in this audit.

#### Scenario: Unknown SSH copy
- **WHEN** no wrap sink is advertised
- **THEN** OSC52 carries the response and UI labels it sent rather than verified copied.

#### Scenario: Doctor follow-up
- **WHEN** /doctor runs
- **THEN** the named finding and wrap guidance appear.

证据：`crates/codegen/pager/tests/pty_e2e_clipboard.rs` — `unknown_ssh_clipboard_delivery_is_unverified`。

### Requirement: Pager selection model external literal API checks
selection_model_public_api SHALL compile an external exhaustive literal and a struct-update literal for ResolvedSelectionModel, proving ranges, visible_blocks and content_area are public and Default can fill omitted fields. Assertions cover only empty ranges and rectangle equality, not selection behavior or invariants.

#### Scenario: Exhaustive literal
- **WHEN** all three current fields are provided externally
- **THEN** construction compiles and ranges is empty.

#### Scenario: Struct update
- **WHEN** content_area is supplied with ..Default
- **THEN** it equals Rect(1,2,3,4).

证据：`crates/codegen/pager/tests/selection_model_public_api.rs`。

### Requirement: Pager XTVERSION ignored PTY validation matrix
pty_xtversion SHALL define nine ignored two-thread Tokio tests against the built pager at 120x50. Unknown and WezTerm brands must emit ESC[>0q and surface complete fictional DCS identity in /doctor without screen garbage; vscode and detected tmux must emit no query. Silent or unterminated replies still reach welcome, omit xtversion diagnostics and do not leak fragments. Complete late or split replies are swallowed and recorded; user bytes around a reply remain intact. Raw-query waits pump 20ms updates until a 20s deadline. Ordinary test runs skip all cases and this audit did not execute them.

#### Scenario: Unknown round trip
- **WHEN** a complete fictional reply follows the observed query
- **THEN** /doctor shows it and probe bytes do not render.

#### Scenario: Probe gate
- **WHEN** vscode is declared or TMUX is set
- **THEN** raw output contains no query.

#### Scenario: Interleaved input
- **WHEN** he, the reply, then y are injected
- **THEN** hey survives while reply bytes are swallowed.

#### Scenario: Malformed reply
- **WHEN** an unterminated DCS arrives
- **THEN** welcome completes and /doctor omits xtversion.

证据：`crates/codegen/pager/tests/pty_xtversion.rs`。

### Requirement: Pager scripted scenario wrapper execution contract
run_scenario SHALL load YAML below tests/scenarios, choose CARGO_TARGET_TMPDIR or OS temp plus scripted-scenarios, resolve the built pager, run ScriptedScenarioRunner, and require Passed, no bugs and a screenshot step with artifacts. Its 40 async wrappers are ignored two-thread Tokio tests. Wrapper comments are not substitutes for the separately auditable YAML steps.

#### Scenario: Passing wrapper
- **WHEN** report is Passed, bug-free and owns a screenshot artifact
- **THEN** the helper returns successfully.

#### Scenario: Missing screenshot
- **WHEN** status passes without an artifact-bearing screenshot step
- **THEN** the helper fails.

#### Scenario: Ordinary Cargo
- **WHEN** ignored tests are not selected
- **THEN** none of the 40 scripted runs executes.

证据：`crates/codegen/pager/tests/scripted_scenarios.rs` — `scenario_path / run_scenario`。

### Requirement: Pager scripted scenario wrapper catalog
The ignored catalog SHALL map 40 Rust tests to named YAMLs spanning startup, input/render/copy, pickers, paste/images/Mermaid, Goal/trust/dashboard, hints/compact UI, skill echo and inline edit/rewind. This file does not compare comments with YAML. Two wrappers perform additional raw artifact checks; the remainder use the shared report helper.

#### Scenario: Named mapping
- **WHEN** scripted_auto_compact_resize runs
- **THEN** it loads auto_compact_resize.yaml.

#### Scenario: Comment drift
- **WHEN** prose and YAML disagree
- **THEN** this wrapper alone does not detect it.

证据：`crates/codegen/pager/tests/scripted_scenarios.rs` — `scripted_*`。

### Requirement: Pager scripted raw OSC8 and Kitty artifact gates
The path-space wrapper SHALL require Passed and raw OSC8 containing Demo%20App.app, rejecting truncated Demo-only output unless the full marker also exists. The inline-image wrapper SHALL scan lossy raw output for Kitty a=T/a=t/a=p substrings, require at least one action, fewer than 30 uploads, placements at least uploads when present, and Passed. It does not correlate image IDs or redraws.

#### Scenario: Full spaced path
- **WHEN** raw OSC8 contains Demo%20App.app
- **THEN** the full-path marker assertion passes.

#### Scenario: Upload bound
- **WHEN** a=T plus a=t reaches 30
- **THEN** the wrapper fails.

#### Scenario: Place records
- **WHEN** a=p exists
- **THEN** its count is required to be at least uploads.

证据：`crates/codegen/pager/tests/scripted_scenarios.rs` — `scripted_path_space_hyperlink / count_kitty_actions / scripted_inline_image_memory`。

### Requirement: Pager scripted YAML ordinary parse gates
Three non-ignored tests SHALL parse 29 named YAML references and require nonempty steps: one list of 27 plus dedicated ANSI and Vim files. The directory currently has 45 YAML files, so ordinary parse gates are not exhaustive. Parsing does not execute actions or validate artifacts/workspace references.

#### Scenario: Listed malformed YAML
- **WHEN** one of 29 named files cannot parse
- **THEN** ordinary cargo test fails.

#### Scenario: Listed empty YAML
- **WHEN** parsed steps are empty
- **THEN** the test fails.

#### Scenario: Unlisted YAML
- **WHEN** another scenario file is malformed
- **THEN** these parse gates do not detect it.

证据：`crates/codegen/pager/tests/scripted_scenarios.rs` — `scenarios_parse / ansi_execute_output_scenario_parses / vim_modal_command_palette_scenario_parses`。

### Requirement: Pager early doctor and du startup isolation tests
Ignored real-binary tests SHALL launch with cleared environment, isolated HOME/GROW_HOME, null stdin and piped output. doctor --json emits clean schemaVersion 1 JSON, preserves hostile valid version state and creates no startup entries. du --json reports one synthetic SQLite-shaped chat.db and creates neither startup artifacts nor SQLite sidecars. Immediate entry names are compared; metadata and most existing content are outside the assertion.

#### Scenario: Doctor JSON
- **WHEN** version.json already exists
- **THEN** command succeeds without changing its bytes or directory entries.

#### Scenario: DU JSON
- **WHEN** chat.db contains a SQLite-shaped header
- **THEN** one entry is reported and no sidecars appear.

证据：`crates/codegen/pager/tests/doctor_early_dispatch.rs`。

### Requirement: Pager doctor fix listing and write boundary integration tests
Ignored Doctor tests SHALL require probe-based applicable fix listing without writes, explicit confirmation, remote SSH refusal, alias-conflict preservation, hostile relative HOME/BYOBU rejection, HOME-only exact managed blocks, and Unix existing-mode preservation. They inspect selected output and files, not full ordering, concurrent updates, symlink races or crash atomicity.

#### Scenario: SSH listing
- **WHEN** doctor fix runs with SSH_CONNECTION
- **THEN** local guidance appears and no shell rc is written.

#### Scenario: Conflict
- **WHEN** an ssh alias already exists
- **THEN** exit is 1 and the file stays byte-identical.

#### Scenario: Confirmed tmux fix
- **WHEN** applicable evidence and --yes exist
- **THEN** only HOME/.tmux.conf receives the exact block.

#### Scenario: Restrictive umask
- **WHEN** an existing rc is 0666 and child umask 077
- **THEN** successful replacement preserves 0666.

证据：`crates/codegen/pager/tests/doctor_early_dispatch.rs`。

### Requirement: Pager doctor tmux probe containment test definitions
Three ignored tests SHALL intend to bound tmux probes below 12 seconds, prevent writes on timeout, close background pipe holders, and on Unix remove a TERM-ignoring descendant within 2 seconds. However fake scripts call bare sleep while PATH contains only fake-bin; the verified shell returns command-not-found. Current fixtures therefore do not establish a blocking/live descendant and cannot prove their named containment behavior.

#### Scenario: Missing sleep
- **WHEN** fake-bin contains no sleep
- **THEN** fake tmux cannot establish the intended 30-second process.

#### Scenario: Fast return
- **WHEN** Doctor returns under 12 seconds
- **THEN** elapsed time alone does not prove timeout handling for this fixture.

#### Scenario: PID disappearance
- **WHEN** exec sleep fails after writing the shell PID
- **THEN** ESRCH may occur without Doctor killing a live descendant.

证据：`crates/codegen/pager/tests/doctor_early_dispatch.rs`；本轮隔离`/bin/sh`探针。

### Requirement: Pager non TTY wrap argv and exit integration check
The ignored non-TTY wrap test SHALL invoke wrap /bin/sh -c with positional argv, require stdout exactly argv-ok and preserve exit code 7. It does not assert stderr, signals, interactive proxying or broader environment behavior.

#### Scenario: Wrapped shell
- **WHEN** child prints $1 and exits 7
- **THEN** pager returns argv-ok and code 7.

#### Scenario: Stderr
- **WHEN** diagnostics are emitted
- **THEN** this test has no stderr expectation.

证据：`crates/codegen/pager/tests/doctor_early_dispatch.rs` — `wrap_non_tty_true_exec_preserves_argv_and_exit`。

### Requirement: Pager wrap direct and shell route ignored PTY checks
Unix-only ignored PTY tests SHALL require existing explicit paths to pass selected output and exit 0/7, one argv string to run through pinned /bin/sh, and a PATH-unresolved bare name to invoke a fake shell as -i -c with the first word bare and spaced tail quoted. Assertions cover selected substrings/status, not complete output or arbitrary quoting.

#### Scenario: Direct route
- **WHEN** /bin/echo succeeds or /bin/sh exits 7
- **THEN** output is visible and exact child status propagates.

#### Scenario: One string
- **WHEN** one argv contains a spaced command line
- **THEN** /bin/sh splits and executes it.

#### Scenario: Alias shape
- **WHEN** a bare name is absent from PATH
- **THEN** the fake shell receives -i, -c and a quoted-tail command.

证据：`crates/codegen/pager/tests/pty_e2e/wrap_{single_string_routes_via_shell,echo_passthrough_and_exit_code,not_found_alias_routes_via_shell_contract}.rs`。

### Requirement: Pager wrap explicit path failure ignored PTY check
The Unix-only ignored test SHALL invoke a nonexistent slash-containing path and require wrapped mode failed, failed to exec and status 1. It does not assert output channel, precise OS error or permission-denied cases.

#### Scenario: Missing explicit path
- **WHEN** /nonexistent-grow-wrap-e2e/prog is wrapped
- **THEN** both fallback notices appear and status is 1.

#### Scenario: Bare name
- **WHEN** first argv has no slash
- **THEN** this test provides no coverage.

证据：`crates/codegen/pager/tests/pty_e2e/wrap_explicit_path_not_found_fails_fast.rs`。

### Requirement: Pager wrap OSC52 sink advertisement ignored PTY check
The Unix-only ignored test SHALL inherit GROW_OSC52_SINK=0, route one string through /bin/sh, require sink=1, exclude sink=0 and exit 0. It does not cover LC_GROW_OSC52_SINK, nested SSH or environment cleanup.

#### Scenario: Inherited zero
- **WHEN** parent supplies sink=0
- **THEN** wrapped shell observes sink=1 only.

#### Scenario: Other sink variable
- **WHEN** LC_GROW_OSC52_SINK is relevant
- **THEN** this test defines no expectation.

证据：`crates/codegen/pager/tests/pty_e2e/wrap_osc52_sink_env_advertised_through_shell.rs`。

### Requirement: Pager wrap terminal mode restoration ignored PTY checks
Three Unix-only ignored tests SHALL cover balanced clean exit, child SIGKILL with five latched modes, and SIGTERM delivered to wrap after READY. Clean exit requires one occurrence of five tested resets and no Kitty pop. Dirty child requires resets after the final enable with alternate-screen leave last among them, but only some exit code. Wrap SIGTERM requires code 143 and four reset presences. No test asserts full byte equality, child reaping or reset uniqueness/order beyond those checks.

#### Scenario: Balanced child
- **WHEN** child balances five mode sequences and exits 0
- **THEN** each tested reset occurs once and no Kitty pop is added.

#### Scenario: Killed child
- **WHEN** SIGKILL leaves modes enabled
- **THEN** matching resets follow and alternate-screen leave is last among tested resets.

#### Scenario: Wrap SIGTERM
- **WHEN** wrap receives SIGTERM after READY
- **THEN** status is 143 and four restores are present.

证据：`crates/codegen/pager/tests/pty_e2e/wrap_{clean_exit_stays_byte_transparent,child_killed_with_latched_modes_restores_terminal,sigterm_restores_terminal_and_exit_code}.rs`。

### Requirement: Pager folder trust prompt decision ignored PTY checks
Three ignored PTY tests SHALL create a git repo with project MCP config. With trust enabled, the question precedes session; two Ctrl-N presses cannot bypass Pending or write a grant, y eventually persists trust and permits a mock response, and n stops pager within 10 seconds without trust. With the feature explicitly off, welcome appears without a question. Grant bytes, exact question ordering and second-process reload are not asserted.

#### Scenario: Pending bypass
- **WHEN** Ctrl-N is pressed twice before answering
- **THEN** the question remains and no grant exists.

#### Scenario: Accept
- **WHEN** y is pressed
- **THEN** trust is eventually visible and a prompt can receive the response.

#### Scenario: Decline
- **WHEN** n is pressed
- **THEN** pager stops and no grant is stored.

#### Scenario: Feature off
- **WHEN** trust is disabled
- **THEN** welcome renders without the question.

证据：`crates/codegen/pager/tests/pty_e2e/folder_trust_{question_renders_and_accept_persists_grant,decline_quits_without_grant,feature_off_shows_no_question}.rs`。

### Requirement: Pager folder trust HOME and subdirectory key ignored PTY checks
Two ignored tests SHALL make HOME a git repo. Launching exactly at HOME with local config reaches welcome without a question because HOME is unrecordable. Launching HOME/proj with its own local config prompts, stores trust for proj and leaves HOME untrusted; TrustStore is loaded directly to avoid the parent test process's HOME. Restart and symlink/canonical variants are not covered.

#### Scenario: CWD equals HOME
- **WHEN** HOME has repo-local config
- **THEN** welcome renders without a question.

#### Scenario: Subdirectory
- **WHEN** HOME/proj owns config and y is pressed
- **THEN** proj becomes trusted.

#### Scenario: No expansion
- **WHEN** proj is trusted
- **THEN** HOME remains untrusted.

证据：`crates/codegen/pager/tests/pty_e2e/folder_trust_{cwd_is_home_git_repo_no_prompt,home_git_repo_subdir_keys_on_subdir}.rs`。

### Requirement: Pager MCP menu project classification ignored PTY wrappers
Two ignored wrappers SHALL delegate to drive_mcp_menu_load with either content HOME for non-project deferred-session classification or a temp directory containing .git for eager project classification. They contain no local assertion; MCP/session/UI behavior remains dependent on later common.rs review.

#### Scenario: Non-project
- **WHEN** cwd equals mock HOME
- **THEN** the shared drive is invoked for the deferred path.

#### Scenario: Project
- **WHEN** a temp cwd contains .git
- **THEN** the shared drive is invoked for the eager path.

#### Scenario: Wrapper audit
- **WHEN** common.rs is not yet read
- **THEN** MCP menu success is not claimed.

证据：`crates/codegen/pager/tests/pty_e2e/mcp_menu_loads_servers_in_{non_project_dir,project_dir}.rs`。

### Requirement: Pager shared PTY corpus and trust fixtures
The shared PTY module SHALL provide deterministic welcome, prompt, long/tall response, contextual-hint and folder-trust fixtures. Trust helpers create an isolated git project with project MCP config and query TrustStore under the supplied HOME; they do not themselves prove cross-process persistence.

#### Scenario: Rendered corpus
- **WHEN** a PTY test requests long or tall output
- **THEN** fixed response text and row sentinels are returned.

#### Scenario: Trust project
- **WHEN** a trust test requests a project fixture
- **THEN** a temporary git repo and local MCP config are created.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `long_response`, `tall_response`, `git_project_with_local_config`, `trust_env`, `folder_is_trusted`。

### Requirement: Pager shared MCP menu PTY drive
drive_mcp_menu_load SHALL seed a platform-specific stdio MCP server named cat-mcp, launch pager in the supplied cwd, pass welcome, enter `/mcps`, require the menu heading and server name within bounded waits, reject the literal panic marker and quit. This is executed only when an ignored wrapper is selected.

#### Scenario: Menu appears
- **WHEN** either project-classification wrapper drives the helper
- **THEN** `MCP Servers` and `cat-mcp` must appear.

#### Scenario: Ordinary Cargo
- **WHEN** ignored tests are not selected
- **THEN** no runtime menu pass is claimed.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `seed_platform_mcp_server`, `drive_mcp_menu_load`。

### Requirement: Pager shared queue and screen interaction helpers
Shared PTY helpers SHALL expose exact queue/interjection key sequences, inject paced bytes, inspect composer/transcript lines by border glyph and substring, and drive a rendered turn into scrollback. The final footer wait in the scrollback drive is best effort and does not fail the helper.

#### Scenario: Paced keys
- **WHEN** bytes are injected through the paced helper
- **THEN** each byte is followed by a 50ms harness update.

#### Scenario: Footer absent
- **WHEN** response and turn sentinel render but the footer wait fails
- **THEN** drive_to_scrollback_with_turn still returns.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `inject_keys_paced`, `composer_holds`, `block_lines_containing`, `drive_to_scrollback_with_turn`。

### Requirement: Pager dual protocol tool call response fixtures
Tool-call fixture builders SHALL encode equivalent single and parallel calls for Responses and Chat Completions, with endpoint-specific created/delta/completed or indexed tool-call events, usage, finish and DONE markers. Registration helpers add both endpoint variants but do not prove client parsing or tool execution.

#### Scenario: Single call
- **WHEN** a call name, id and argument JSON are supplied
- **THEN** both endpoint-specific SSE scripts represent the same logical function call.

#### Scenario: Parallel calls
- **WHEN** indexed calls are supplied
- **THEN** each protocol receives its corresponding parallel event shape.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `responses_tool_call_sse`, `chat_tool_call_sse`, `expect_tool_turn`, `parallel_responses_sse`, `parallel_chat_sse`。

### Requirement: Pager OSC52 and SGR mouse PTY helpers
OSC52 helpers SHALL poll cumulative raw output, skip empty or invalid candidates, accept BEL or ESC termination, and decode standard base64 plus UTF-8. SGR mouse helpers encode zero-based coordinates as one-based wire positions. locate_screen_text reports a Unicode-scalar column rather than terminal display-cell width.

#### Scenario: Valid clipboard payload
- **WHEN** a valid non-empty OSC52 payload appears before timeout
- **THEN** its decoded UTF-8 text is returned.

#### Scenario: Wide prefix
- **WHEN** wide graphemes precede located screen text
- **THEN** the returned scalar-count column may differ from its display column.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `wait_for_osc52_payload`, `decode_osc52_payload`, `sgr_mouse_drag`, `locate_screen_text`。

### Requirement: Pager Minimal PTY lifecycle and plan fixtures
Minimal helpers SHALL spawn with `--minimal --no-leader`, wait on the minimal idle sentinel, provide tagged plan predicates, and quit by two Ctrl-Q chords with a 15-second kill fallback. session_dir polls for the first directory below the sessions root without validating expected cwd or session identity.

#### Scenario: Cold start
- **WHEN** the minimal status line appears
- **THEN** readiness succeeds without the full pager welcome sentinel.

#### Scenario: Ambiguous session directories
- **WHEN** several directories exist below sessions
- **THEN** the first filesystem iteration result is returned.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `spawn_minimal`, `wait_minimal_ready`, `quit_minimal`, `plan_body`, `session_dir`。

### Requirement: Pager wrap exit polling and diagnostic helpers
Exit polling SHALL preserve Running, PendingStatus and Exited until exit or deadline and propagate poll errors. Unix wrap helpers launch under isolated GROW_HOME, allow a live drive, wait up to 120 seconds, drain after exit, kill a running timeout and reject deadline PendingStatus. Diagnostic helpers provide argv echo, bounded polling/dumps, task-id extraction, best-effort casts and multimodal-tolerant user blobs.

#### Scenario: Running deadline
- **WHEN** wrap remains Running at the deadline
- **THEN** it is killed and code is None.

#### Scenario: Pending status deadline
- **WHEN** the child exited but status remains unavailable
- **THEN** the wrap helper panics rather than inventing a code.

#### Scenario: Multimodal user content
- **WHEN** user content is an array
- **THEN** all_user_message_blobs serializes it as JSON text.

证据：`crates/codegen/pager/tests/pty_e2e/common.rs` — `wait_for_exit_status`, `run_wrap_driving`, `fake_argv_echo_shell`, `dump_non_system_messages`, `extract_task_id`, `write_cast_if_requested`, `all_user_message_blobs`。

### Requirement: Pager resize triggered render ignored PTY check
The ignored resize test SHALL reset frame timing after welcome, resize to 40 by 100, wait 500ms and require at least one synchronized-update frame. It does not assert exact bytes, frame count, layout or idle behavior outside the measured interval.

#### Scenario: Resize
- **WHEN** the PTY is resized after timing reset
- **THEN** frame_count is greater than zero.

证据：`crates/codegen/pager/tests/pty_e2e/renders_on_action.rs` — `renders_on_action`。

### Requirement: Pager prompt to mock response ignored PTY checks
Two ignored tests SHALL exercise interactive and positional prompt submission. The interactive test injects prompt plus Enter after welcome; the positional test injects no keys. Each requires the mock response and a recorded chat completion within 30 seconds, while the positional case also rejects the literal screen substring `panicked`.

#### Scenario: Interactive prompt
- **WHEN** prompt and Enter are injected after welcome
- **THEN** a response sentinel and recorded request exist.

#### Scenario: Positional prompt
- **WHEN** the prompt is supplied as the launch argument
- **THEN** the response arrives without injected keys.

证据：`crates/codegen/pager/tests/pty_e2e/agent_response.rs` — `agent_response`；`crates/codegen/pager/tests/pty_e2e/initial_prompt_positional_auto_submits.rs` — `initial_prompt_positional_auto_submits`。

### Requirement: Pager undo tip seen count non persistence ignored PTY check
The ignored undo-tip test SHALL enable contextual hints, trigger the tip by wiping a substantial draft, quit, and require readable isolated config text does not contain `undo_tip_shown_count`. A missing or unreadable config is converted to empty text, and unrelated persisted settings are not rejected.

#### Scenario: Readable config
- **WHEN** config.toml can be read after the tip and quit
- **THEN** it contains no undo_tip_shown_count substring.

#### Scenario: Read failure
- **WHEN** config.toml is absent or unreadable
- **THEN** the assertion evaluates an empty fallback.

证据：`crates/codegen/pager/tests/pty_e2e/undo_tip_seen_count_never_persisted.rs` — `undo_tip_seen_count_never_persisted`。

### Requirement: Pager long response scroll liveness ignored PTY check
The ignored scroll test SHALL generate a 200-line response, inject J forty times with updates, then require pager liveness and absence of the literal current-screen substring `panicked`. It does not assert viewport position, visible line identity, memory use or complete stream delivery.

#### Scenario: Repeated J input
- **WHEN** forty scroll keys follow the long response sentinel
- **THEN** the pager remains running.

#### Scenario: Current screen marker
- **WHEN** scrolling completes
- **THEN** the current screen does not contain `panicked`.

证据：`crates/codegen/pager/tests/pty_e2e/scroll_does_not_crash.rs` — `scroll_does_not_crash`。

### Requirement: Pager macOS native clipboard text latency ignored PTY check
The macOS-only ignored serial clipboard test SHALL overwrite the host text clipboard, inject Ctrl-V on welcome, and require the sentinel to echo within three seconds without a literal `panicked` marker. Restoration is text-only and cannot restore a prior image clipboard; the assertion does not directly observe the raster pre-gate.

#### Scenario: Text paste
- **WHEN** Ctrl-V is injected with the sentinel on the host pasteboard
- **THEN** the sentinel prefix appears within three seconds.

#### Scenario: Prior image clipboard
- **WHEN** the clipboard previously held only an image
- **THEN** the text guard cannot restore it.

证据：`crates/codegen/pager/tests/pty_e2e/paste_ctrl_v_text_echoes_fast_macos.rs` — `paste_ctrl_v_text_echoes_fast_macos`。

### Requirement: Pager idle prompt input wake ignored PTY check
The ignored idle-input test SHALL wait three seconds after a mock response, inject a marker without Enter, and require it to render within two seconds. The delay is an empirical idle proxy and the test does not observe the internal animation or input-waker state.

#### Scenario: Marker after settle
- **WHEN** marker bytes are injected after the three-second settle
- **THEN** they appear within two seconds.

#### Scenario: Wake source
- **WHEN** the marker appears
- **THEN** the assertion does not identify which source woke the event loop.

证据：`crates/codegen/pager/tests/pty_e2e/input_echoes_at_idle_prompt.rs` — `input_echoes_at_idle_prompt`。

### Requirement: Pager undo tip session cap ignored PTY check
The ignored undo-tip test SHALL require three fresh shows separated by label expiry, then require the fourth wipe in the same session to show no tip after one second. It does not inspect the counter or prove behavior across sessions.

#### Scenario: First three
- **WHEN** each previous tip expires before the next wipe
- **THEN** wipes one through three each show the tip.

#### Scenario: Fourth
- **WHEN** three shows occurred in the session
- **THEN** the fourth wipe remains without the tip.

证据：`crates/codegen/pager/tests/pty_e2e/undo_tip_session_cap_blocks_fourth_show.rs` — `undo_tip_session_cap_blocks_fourth_show`。

### Requirement: Pager queued follow up send now tip ignored PTY check
The ignored queue-hint test SHALL submit a follow-up during a deliberately slow first turn and require both the send-now tip and `Queued` copy within ten seconds without a literal `panicked` marker. It does not activate send-now or verify queue persistence and delivery.

#### Scenario: Follow-up during stream
- **WHEN** a follow-up is submitted after TURNONE appears
- **THEN** send-now and Queued text render.

#### Scenario: Advertised action
- **WHEN** the tip is visible
- **THEN** the test quits without executing the advertised chord.

证据：`crates/codegen/pager/tests/pty_e2e/send_now_tip_after_mid_turn_queue.rs` — `send_now_tip_after_mid_turn_queue`。

### Requirement: Pager active GROW_HOME model config hot reload ignored PTY check
The ignored hot-reload test SHALL run with cwd equal to active `.grow`, append a valid mock model table to config.toml, wait 1800ms and require `/model` to display `mock/hot-added` without restart. It does not verify watcher events, atomic rename, malformed recovery or selecting the model.

#### Scenario: Live config append
- **WHEN** hot-added model config is appended while pager remains running
- **THEN** the same process displays the model in its selector.

#### Scenario: Selection
- **WHEN** the new model row appears
- **THEN** the test does not select it or issue a turn with it.

证据：`crates/codegen/pager/tests/pty_e2e/model_config_hot_reload_in_grow_home.rs` — `model_config_hot_reload_in_grow_home`。

### Requirement: Pager embedded blocked backend startup ignored PTY check
The ignored embedded test SHALL launch `--no-leader` against a loopback endpoint that accepts but never replies and require welcome within 30 seconds without a literal `panicked` marker. It does not identify individual startup calls or their timeout values.

#### Scenario: Non-responsive endpoint
- **WHEN** both configured backend URLs retain connections without replies
- **THEN** embedded welcome must render within 30 seconds.

#### Scenario: Timeout attribution
- **WHEN** welcome renders
- **THEN** no particular startup request is thereby proven to have timed out.

证据：`crates/codegen/pager/tests/pty_e2e/embedded_mode_boots_without_hanging_on_blocked_backend.rs` — `embedded_mode_boots_without_hanging_on_blocked_backend`。

### Requirement: Pager post activity Ctrl C no rewind ignored PTY check
The ignored cancellation test SHALL wait for visible streamed activity, inject Ctrl-C, require the cancellation marker, then require the prompt is absent from a composer-classified line and present in exactly one current-screen block-classified line. Raw history, persistence and cancellation transport are not inspected.

#### Scenario: Cancel after activity
- **WHEN** Ctrl-C follows the CANCELME marker
- **THEN** Turn cancelled by user appears.

#### Scenario: Visible prompt placement
- **WHEN** cancellation completes
- **THEN** the prompt is not in the composer and occurs in one classified block line.

证据：`crates/codegen/pager/tests/pty_e2e/ctrlc_after_activity_no_rewind_prompt_once.rs` — `ctrlc_after_activity_no_rewind_prompt_once`。

### Requirement: Pager waiting for model label ignored PTY check
The ignored waiting-label test SHALL delay mock events, require `Waiting for response` after prompt submission, remove the delay and require the configured response. It matches without the ellipsis and does not inspect WaitingReason or exact event ordering.

#### Scenario: Delayed first event
- **WHEN** each mock SSE event is delayed three seconds
- **THEN** Waiting for response appears within ten seconds.

#### Scenario: Delay removed
- **WHEN** mock delay becomes None
- **THEN** the response sentinel appears within 30 seconds.

证据：`crates/codegen/pager/tests/pty_e2e/waiting_for_model_label.rs` — `waiting_for_model_label_shows_before_first_token`。

### Requirement: Pager config reasoning effort menu ignored PTY check
The ignored effort test SHALL define a single ConfigHigh effort for the configured model, open `/effort`, require that label and reject the literal `Extended reasoning`. It does not select the row, inspect model request parameters or exclude every differently named undeclared row.

#### Scenario: Config label
- **WHEN** `/effort` opens for mock/grow-4.5
- **THEN** ConfigHigh appears within ten seconds.

#### Scenario: Synthetic label
- **WHEN** the menu is visible
- **THEN** Extended reasoning is absent from the current screen.

证据：`crates/codegen/pager/tests/pty_e2e/reasoning_efforts_from_config_toml_menu.rs` — `reasoning_efforts_from_config_toml_menu`。

### Requirement: Pager Windows native clipboard text ignored PTY check
The Windows-only ignored serial clipboard test SHALL save text before probing, log and return successfully when native roundtrip is unavailable, or otherwise paste the sentinel and require it within ten seconds without `panicked`. Restoration is text-only and elapsed latency below ten seconds is not constrained.

#### Scenario: Clipboard unavailable
- **WHEN** the native roundtrip probe fails
- **THEN** the test returns through its SKIP path without launching pager.

#### Scenario: Clipboard usable
- **WHEN** Ctrl-V follows writing the sentinel
- **THEN** the sentinel prefix appears within ten seconds.

证据：`crates/codegen/pager/tests/pty_e2e/paste_ctrl_v_text_echoes_fast_windows.rs` — `paste_ctrl_v_text_echoes_fast_windows`。

### Requirement: Pager bracketed multiline chip payload ignored PTY check
The ignored multiline paste test SHALL inject twelve bracketed lines, require `Pasted:`, submit and require one user-message blob contains the first and last sentinels. It does not assert the displayed count, exact threshold, every middle line or whole-message equality.

#### Scenario: Chip render
- **WHEN** the twelve-line payload is injected
- **THEN** Pasted: appears within five seconds.

#### Scenario: Boundary payload
- **WHEN** the chipped prompt is submitted
- **THEN** a recorded user blob contains both boundary sentinels.

证据：`crates/codegen/pager/tests/pty_e2e/paste_bracketed_chip_text_sends_full_payload.rs` — `paste_bracketed_chip_text_sends_full_payload`。

### Requirement: Pager scrollback Esc cancel ignored PTY check
The ignored scrollback test SHALL use one Tab and require `Space:prompt`, then inject one Esc and require the cancellation marker. After settling, the current screen must contain that marker once and no `panicked`; transport, persistence and off-screen duplicates are not inspected.

#### Scenario: Footer ownership
- **WHEN** one Tab follows the streaming sentinel
- **THEN** Space:prompt appears before cancellation input.

#### Scenario: Esc cancel
- **WHEN** Esc is injected from that rendered state
- **THEN** Turn cancelled by user appears within 15 seconds.

证据：`crates/codegen/pager/tests/pty_e2e/esc_cancels_running_turn_from_scrollback.rs` — `esc_cancels_running_turn_from_scrollback`。

### Requirement: Pager bracketed inline paste payload ignored PTY check
The ignored inline paste test SHALL inject two bracketed lines, require both sentinels to render within bounded waits, submit and require one user blob contains both. Contains assertions do not prove exact newline bytes, whole-message equality or absence of incidental attachment content.

#### Scenario: Inline echo
- **WHEN** the two-line bracketed payload is injected
- **THEN** both sentinel substrings render.

#### Scenario: Submitted content
- **WHEN** Enter is injected and the response renders
- **THEN** one user blob contains both sentinel substrings.

证据：`crates/codegen/pager/tests/pty_e2e/paste_bracketed_inline_text_echoes_and_sends_intact.rs` — `paste_bracketed_inline_text_echoes_and_sends_intact`。

### Requirement: Pager mid turn slash Esc precedence ignored PTY check
The ignored overlay test SHALL open `/mod` during a delayed response, require the model description, inject Esc, and require the description plus cancellation marker are absent after two settles. It does not poll turn liveness or prove the response did not end naturally.

#### Scenario: Overlay Esc
- **WHEN** Esc is injected while Switch the active model is visible
- **THEN** the description disappears without a visible cancellation marker.

#### Scenario: Later check
- **WHEN** another 600ms elapses
- **THEN** the current screen still lacks Turn cancelled by user.

证据：`crates/codegen/pager/tests/pty_e2e/mid_turn_slash_dropdown_esc_dismisses_not_cancel.rs` — `mid_turn_slash_dropdown_esc_dismisses_not_cancel`。

### Requirement: Pager text selection settings registration ignored PTY check
The ignored settings smoke test SHALL seed hold, open settings and search for any of three accepted selection labels for up to eight seconds, using filter and reopen retries. Passing does not prove hold is selected, config was loaded or copy behavior changed.

#### Scenario: Any accepted label
- **WHEN** Text selection, Flash after copy or Hold until dismissed appears
- **THEN** the smoke test succeeds.

#### Scenario: Seeded hold
- **WHEN** a different accepted label appears
- **THEN** the test may pass without observing the selected value.

证据：`crates/codegen/pager/tests/pty_e2e/keep_text_selection_settings_visible_pty.rs` — `keep_text_selection_settings_visible_pty`。

### Requirement: Pager welcome logo geometry ignored PTY checks
Two ignored welcome tests SHALL require the small motif and exclude the big motif at 50x120, then require the big motif and exclude the small motif at 50x180. Motif substrings do not prove full art dimensions or exact selection thresholds.

#### Scenario: Default size
- **WHEN** welcome renders at 50x120
- **THEN** only the small motif is present.

#### Scenario: Wide size
- **WHEN** welcome renders at 50x180
- **THEN** only the big motif is present.

证据：`crates/codegen/pager/tests/pty_e2e/welcome_screen_world_tree_logo_renders_correctly.rs` — `welcome_screen_logo_renders_correctly`, `welcome_screen_big_logo_renders_on_wide_terminal`。

### Requirement: Pager prompt Esc cancel draft visibility ignored PTY check
The ignored prompt test SHALL type a draft during a delayed stream, inject one Esc, require cancellation, and then require the current screen still contains the draft without a clear-confirm hint. It does not prove the text remains in the composer or is persisted.

#### Scenario: Cancel with draft
- **WHEN** Esc is injected after the draft renders during a stream
- **THEN** Turn cancelled by user appears.

#### Scenario: Visible draft
- **WHEN** cancellation settles
- **THEN** the draft remains somewhere on screen and press again to clear is absent.

证据：`crates/codegen/pager/tests/pty_e2e/esc_cancels_running_turn_from_prompt_preserves_draft.rs` — `esc_cancels_running_turn_from_prompt_preserves_draft`。

### Requirement: Pager idle empty Esc swallow ignored PTY check
The ignored idle-empty test SHALL inject Esc twice in a promoted session with no submitted message and empty prompt, require no scrollback footer, confirmation or rewind picker, then require subsequent typed input to render. Internal focus and pending-action state are not directly inspected.

#### Scenario: Empty session Esc
- **WHEN** two Esc presses occur with empty prompt and no submitted turn
- **THEN** no visible focus, confirmation or rewind surface appears.

#### Scenario: Later input
- **WHEN** STILLHERE is typed afterward
- **THEN** it renders within ten seconds.

证据：`crates/codegen/pager/tests/pty_e2e/esc_idle_empty_no_messages_is_swallowed_noop.rs` — `esc_idle_empty_no_messages_is_swallowed_noop`。

### Requirement: Pager idle double Esc clear and history ignored PTY check
The ignored double-Esc test SHALL show the clear confirmation after the first Esc, remove draft and confirmation after the second, then show the cleared draft after Up opens history. It does not inspect persistent history or the default confirmation timeout.

#### Scenario: Arm and clear
- **WHEN** two separated Esc presses target an idle draft
- **THEN** the first shows confirmation and the second clears both draft and hint.

#### Scenario: Recall
- **WHEN** Up is injected afterward
- **THEN** the cleared draft appears in the history surface.

证据：`crates/codegen/pager/tests/pty_e2e/esc_esc_clears_idle_prompt_and_records_history.rs` — `esc_esc_clears_idle_prompt_and_records_history`。

### Requirement: Pager plan revise empty Enter ignored PTY check
The ignored plan test SHALL seed plan.md through the first discovered session directory, park approval, focus revision and submit empty Enter. The revision nudge and waiting approval remain visible while `Enter:approve` stays absent. Session identity, tool expectation completion and persistence are not verified.

#### Scenario: Empty revision
- **WHEN** Enter is pressed with no revision notes
- **THEN** the nudge appears and approval remains open.

#### Scenario: Footer contract
- **WHEN** the empty revision is rejected
- **THEN** Enter:approve is absent from the current screen.

证据：`crates/codegen/pager/tests/pty_e2e/plan_revise_empty_enter_does_not_approve.rs` — `plan_revise_empty_enter_does_not_approve`。

### Requirement: Pager continue latest history ignored PTY check
The ignored continue test SHALL complete a turn in a stable git cwd, relaunch there with `--continue`, require one current-screen occurrence of the first sentinel, then accept a second turn. It does not inspect session identity, durable files or off-screen duplicates.

#### Scenario: Relaunch
- **WHEN** the second pager starts with --continue in the same cwd
- **THEN** the first turn sentinel renders once on the sampled screen.

#### Scenario: Follow-up
- **WHEN** again is submitted after resume
- **THEN** the second sentinel renders.

证据：`crates/codegen/pager/tests/pty_e2e/continue_resumes_session_with_history.rs` — `continue_resumes_session_with_history`。

### Requirement: Pager macOS Otty IME image suppression ignored PTY check
The macOS-only ignored test SHALL skip successfully when clipboard roundtrip is unavailable; otherwise it places a PNG on the host pasteboard, launches as Otty, injects bracketed Chinese text, and after three seconds requires no visible image chip. It does not submit or inspect attachment state and cannot restore a prior image.

#### Scenario: Otty IME text
- **WHEN** bracketed Chinese text arrives while a PNG is on the clipboard
- **THEN** the text renders and `[Image #` is absent after three seconds.

#### Scenario: Clipboard unavailable
- **WHEN** the native roundtrip probe fails
- **THEN** the test returns through its SKIP path.

证据：`crates/codegen/pager/tests/pty_e2e/bracketed_ime_paste_skips_clipboard_image_macos.rs` — `bracketed_ime_paste_skips_clipboard_image_macos`。

### Requirement: Pager undo tip new process reset ignored PTY check
The ignored reset test SHALL exhaust three undo-tip shows in one pager process, confirm the fourth is absent, then reuse the same HOME in a second process and require the first wipe to show the tip. It does not inspect disk or the internal counter.

#### Scenario: First process cap
- **WHEN** three TTL-separated shows precede a fourth wipe
- **THEN** the fourth tip is absent.

#### Scenario: Second process
- **WHEN** a fresh pager with the same HOME performs one wipe
- **THEN** the tip appears again.

证据：`crates/codegen/pager/tests/pty_e2e/undo_tip_resets_each_new_session.rs` — `undo_tip_resets_each_new_session`。

### Requirement: Pager macOS image paste responsive ordering ignored PTY check
The macOS-only ignored image test SHALL inject Ctrl-V and marker bytes together, require the marker within two seconds before visible `Image #`, then require `Image #1` within 20 seconds. It proves visible ordering, not worker-thread identity, decode, persistence or submitted attachment content.

#### Scenario: Early echo
- **WHEN** Ctrl-V and marker bytes share one injection
- **THEN** the marker appears before any visible image chip.

#### Scenario: Deferred attachment
- **WHEN** the early check passes
- **THEN** Image #1 appears within 20 seconds.

证据：`crates/codegen/pager/tests/pty_e2e/paste_ctrl_v_image_keeps_ui_responsive_macos.rs` — `paste_ctrl_v_image_keeps_ui_responsive_macos`。

### Requirement: Pager small screen tip slow turn lifetime ignored PTY check
The ignored small-screen test SHALL show the compact-mode tip at 24 rows, retain it 1500ms into a delayed turn, then require it disappears after the response and does not reappear after later input. It does not prove exact TTL or every height in the documented band.

#### Scenario: Mid-turn retention
- **WHEN** a delayed response runs for 1500ms after submit
- **THEN** pager is live and the tip remains visible.

#### Scenario: Expiry and later input
- **WHEN** the response finishes and the tip disappears
- **THEN** typing x does not show it again within 800ms.

证据：`crates/codegen/pager/tests/pty_e2e/small_screen_tip_survives_slow_turn.rs` — `small_screen_tip_survives_slow_turn`。

### Requirement: Pager bash chrome redundant cwd ignored PTY check
The Unix-only ignored bash test SHALL run `cd <session cwd> && printf`, require output and Run chrome, then require current-screen Run lines contain printf and exclude the exact redundant cwd prefix. History rows, off-screen chrome and exit status are outside the assertion.

#### Scenario: Exact session cwd prefix
- **WHEN** bash mode receives cd to the canonical session cwd followed by printf
- **THEN** Run chrome shows printf without that prefix.

证据：`crates/codegen/pager/tests/pty_e2e/bash_mode_strips_redundant_session_cd_from_chrome.rs` — `bash_mode_strips_redundant_session_cd_from_chrome`。

### Requirement: Pager queued bash promotion ignored PTY check
The Unix-only ignored test SHALL queue a bash command during a slow turn, poll for its marker file, require executed output and Run chrome, and require recorded user blobs exclude its sentinel. It does not assert exact drain order, exit status or all model request fields.

#### Scenario: Queued command
- **WHEN** bash input is submitted after STEPONE
- **THEN** its format text is visible in the queue.

#### Scenario: Drain
- **WHEN** the command is promoted
- **THEN** marker, expanded output and Run chrome become observable.

证据：`crates/codegen/pager/tests/pty_e2e/queued_bash_promotion_renders_output_pty.rs` — `queued_bash_promotion_renders_output_pty`。

### Requirement: Pager embedded focus stale row repair ignored PTY check
The ignored NVIM-context test SHALL inject a stale row directly into the virtual terminal, require it survives an ordinary update, then inject FocusGained and require it disappears. It simulates rather than executes nested tmux and nvim.

#### Scenario: Ordinary redraw
- **WHEN** feed_screen inserts the marker
- **THEN** it remains after 300ms.

#### Scenario: Focus gained
- **WHEN** CSI I is injected
- **THEN** the marker disappears within five seconds.

证据：`crates/codegen/pager/tests/pty_e2e/doubled_lines_out_of_band_repro.rs` — `out_of_band_stale_row_heals_on_focus_gained`。

### Requirement: Pager Windows image paste responsive ordering ignored PTY check
The Windows-only ignored clipboard test SHALL skip successfully when roundtrip is unavailable; otherwise marker text must render before an image chip and Image #1 must later appear. Visible ordering does not directly prove thread, decode or persistence ownership.

#### Scenario: Early marker
- **WHEN** Ctrl-V and marker bytes share one injection
- **THEN** the marker renders within ten seconds before Image #.

#### Scenario: Later chip
- **WHEN** the early check passes
- **THEN** Image #1 appears within 30 seconds.

证据：`crates/codegen/pager/tests/pty_e2e/paste_ctrl_v_image_keeps_ui_responsive_windows.rs` — `paste_ctrl_v_image_keeps_ui_responsive_windows`。

### Requirement: Pager spaced file path OSC8 ignored PTY check
The ignored WezTerm test SHALL render the spaced filename suffix and require raw output contains OSC8 plus `Demo%20App.app`. It does not pair-parse OSC8, click the target or forbid an additional truncated link when a full marker also exists.

#### Scenario: Spaced suffix
- **WHEN** the synthetic app-bundle path streams
- **THEN** its prefix and App.app suffix are visible.

#### Scenario: Encoded raw output
- **WHEN** the line flushes
- **THEN** raw PTY bytes contain OSC8 and the percent-encoded suffix.

证据：`crates/codegen/pager/tests/pty_e2e/file_path_with_space_emits_full_osc8_hyperlink.rs` — `file_path_with_space_emits_full_osc8_hyperlink`。

### Requirement: Pager path free image chip preview ignored PTY check
The ignored preview test SHALL paste an 8x8 PNG path, show a path-free Image #1 chip plus format and filename metadata, then hide the preview metadata after typing while retaining the chip. It does not submit the image, inspect request data or explicitly quit the harness.

#### Scenario: Preview
- **WHEN** the PNG path becomes an image chip
- **THEN** format and filename render without a path-bearing chip label.

#### Scenario: Dismiss
- **WHEN** text is typed afterward
- **THEN** chip and text remain while preview metadata disappears.

证据：`crates/codegen/pager/tests/pty_e2e/image_chip_preview_path_free_pty.rs` — `image_chip_preview_path_free_pty`。

### Requirement: Pager pre activity Ctrl C rewind ignored PTY check
The ignored rewind test SHALL delay all server events, require the submitted prompt is committed outside the composer, then inject Ctrl-C and require it returns to the composer with zero visible block copies and no cancellation marker. Each body may contain at most one string-form user copy, while separate retry bodies may each contain one.

#### Scenario: Before activity
- **WHEN** Ctrl-C arrives while Waiting for response
- **THEN** the prompt returns to the composer and its block disappears.

#### Scenario: Per-body uniqueness
- **WHEN** requests are inspected
- **THEN** no one body contains two string-form user copies.

证据：`crates/codegen/pager/tests/pty_e2e/send_then_ctrlc_rewinds_to_composer_no_history_dup.rs` — `send_then_ctrlc_rewinds_to_composer_no_history_dup`。

### Requirement: Pager drag autoscroll monotonic clamp ignored PTY check
The ignored synthetic mouse test SHALL scroll up from a 60-marker bottom clamp, hold a drag beyond the pane bottom, and require 25 sampled top-marker indices never decrease, finish at baseline and remain flat for the last five samples. It does not drive real mouse hardware and inherits scalar-based screen coordinate limits.

#### Scenario: Held drag
- **WHEN** press and two motion reports move below the pane
- **THEN** sampled marker indices are monotonic non-decreasing.

#### Scenario: Clamp
- **WHEN** sampling ends
- **THEN** the viewport equals baseline and its final five samples are equal.

证据：`crates/codegen/pager/tests/pty_e2e/drag_autoscroll_no_bounce_pty.rs` — `drag_autoscroll_no_bounce_pty`。

### Requirement: Pager bracketed paste immediate send ignored PTY check
The ignored race test SHALL inject three pasted lines and Enter in one buffer, require a response, then require some serialized body contains the first sentinel once, no body contains it more than once, and one user blob contains all three sentinels. Separate retry bodies may each contain one copy.

#### Scenario: Paste and Enter
- **WHEN** both arrive in one input burst
- **THEN** a payload-bearing model request and response are observed.

#### Scenario: Body counts
- **WHEN** serialized bodies are counted
- **THEN** at least one count is one and every count is at most one.

证据：`crates/codegen/pager/tests/pty_e2e/paste_bracketed_then_immediate_enter_sends_intact.rs` — `paste_bracketed_then_immediate_enter_sends_intact`。

### Requirement: Pager send page flip setting ignored PTY checks
Two ignored tests SHALL hold a second turn after an 80-line first response. Default send removes the old tail from the sampled screen and places the new prompt in its top half; disabling page flip leaves the tail visible. Exact offset and other terminal heights are not covered.

#### Scenario: Default
- **WHEN** the second prompt is sent
- **THEN** the old tail is absent and the prompt row is above half height.

#### Scenario: Disabled
- **WHEN** page_flip_on_send is false
- **THEN** the old tail remains visible.

证据：`crates/codegen/pager/tests/pty_e2e/page_flip_on_send_pty.rs` — `send_page_flips_by_default`, `send_keeps_viewport_when_page_flip_disabled`。

### Requirement: Pager forced wheel environment exact rows ignored PTY check
The ignored forced-wheel test SHALL combine Zed, wheel mode and one-line settings, inject three zero-gap wheel-up reports, and require exactly three rows of upward marker movement with pager liveness. It does not isolate each environment variable or drive physical wheel input.

#### Scenario: Forced burst
- **WHEN** three wheel-up reports arrive under the forced environment
- **THEN** the top marker index decreases by exactly three.

证据：`crates/codegen/pager/tests/pty_e2e/forced_wheel_mode_env_scrolls_exact_rows.rs` — `forced_wheel_mode_env_scrolls_exact_rows`。

### Requirement: Pager queued prompt Ctrl C wire uniqueness ignored PTY check
The ignored queued-prompt test SHALL hold A, queue B, inject Ctrl-C, release A and require B's response. A stays out of the composer, while the final Chat user_query list contains A and B once each. It does not require a visible cancellation marker or inspect Responses and earlier bodies.

#### Scenario: Promote B
- **WHEN** Ctrl-C occurs with B queued and A is released
- **THEN** B's response renders.

#### Scenario: Final Chat body
- **WHEN** user queries are counted
- **THEN** A and B each occur exactly once.

证据：`crates/codegen/pager/tests/pty_e2e/ctrlc_with_queued_prompt_no_dup.rs` — `ctrlc_with_queued_prompt_no_dup`。

### Requirement: Pager drag wheel selection extension ignored PTY check
The ignored SSH/OSC52 test SHALL drag upward, inject sixteen wheel reports mid-drag and release. Decoded payloads must include the anchor and a marker twelve rows above while excluding the row below. Synthetic SGR and substring checks do not prove exact copy range or real clipboard state.

#### Scenario: Extend upward
- **WHEN** the wheel burst occurs during the held drag
- **THEN** OSC52 copy includes content revealed at least twelve rows above.

#### Scenario: Direction boundary
- **WHEN** payloads are joined
- **THEN** the marker immediately below the anchor is absent.

证据：`crates/codegen/pager/tests/pty_e2e/drag_select_wheel_scroll_extends_pty.rs` — `drag_select_wheel_scroll_extends_pty`。

### Requirement: Pager stream Ctrl C recovery and cancel logs ignored PTY check
The ignored stream-cancel test SHALL render one current-screen cancellation marker, accept a new prompt afterward, and after quit require unified-log substrings for cancel received and processing. It does not inspect exact RPC identities, structured events or off-screen duplicates.

#### Scenario: Cancel and recover
- **WHEN** Ctrl-C interrupts the paced stream
- **THEN** one visible marker appears and the next prompt can produce a response.

#### Scenario: Log markers
- **WHEN** unified.jsonl is read after quit
- **THEN** receipt and processing message substrings both exist.

证据：`crates/codegen/pager/tests/pty_e2e/ctrl_c_cancel_during_stream_recovers_cleanly.rs` — `ctrl_c_cancel_during_stream_recovers_cleanly`。

### Requirement: Pager same turn interjection ignored PTY check
The ignored steer test SHALL use Ctrl-Enter during a held turn, require the draft moves from composer to a block, then require the second response without a cancellation marker. A matching Chat user message must carry the interjection and user_query envelopes; foreground ID and Responses shape are not inspected.

#### Scenario: Commit steer
- **WHEN** Ctrl-Enter submits the follow-up during TURNONE
- **THEN** it leaves composer, appears in a block and TURNTWO renders.

#### Scenario: Chat envelope
- **WHEN** the matching user message is found
- **THEN** both interjection prefix and user_query tags are present.

证据：`crates/codegen/pager/tests/pty_e2e/interjection_reaches_model_in_same_turn.rs` — `interjection_reaches_model_in_same_turn`。

### Requirement: Pager queued bash FIFO drain ignored PTY check
The Unix-only ignored test SHALL keep a queued bash row behind a held turn despite empty Enter, then execute it after release with Run chrome and no QBASH user blob. It does not inspect queue acknowledgement, exact start time or shell exit status.

#### Scenario: Before release
- **WHEN** empty Enter targets the queued bash row
- **THEN** no send-now hint, output or cancellation marker appears.

#### Scenario: After release
- **WHEN** the foreground barrier opens
- **THEN** bash output and Run chrome render without a user-prompt block.

证据：`crates/codegen/pager/tests/pty_e2e/bash_queued_mid_turn_drains_as_bash.rs` — `bash_queued_mid_turn_drains_as_bash`。

### Requirement: Pager usage context session modal ignored PTY check
The ignored modal test SHALL open `/usage`, navigate by Tab through context and session-info content, then close with Esc and require selected modal strings disappear from the current screen. It does not directly invoke the other slash aliases, inspect values or persisted transcript.

#### Scenario: Three tabs
- **WHEN** /usage opens and Tab is pressed twice
- **THEN** usage, context and session identity content become visible in sequence.

#### Scenario: Close
- **WHEN** Esc closes the modal
- **THEN** selected content strings are absent from the current screen.

证据：`crates/codegen/pager/tests/pty_e2e/usage_modal_pty.rs` — `usage_modal_opens_switches_and_closes_pty`。

### Requirement: Pager cancel rewind resend uniqueness ignored PTY check
The ignored resend test SHALL hold a stable pre-activity rewind, resend the restored prompt, require one visible block copy and at most one string-form matching user item per request body. Multiple bodies may each contain one and persistent history is not inspected.

#### Scenario: Stable rewind
- **WHEN** Ctrl-C precedes first activity
- **THEN** composer restoration with zero block copies holds for 1500ms.

#### Scenario: Resend
- **WHEN** Enter submits the restored prompt
- **THEN** the reply renders and each body contains at most one copy.

证据：`crates/codegen/pager/tests/pty_e2e/cancel_then_resend_prompt_appears_once.rs` — `cancel_then_resend_prompt_appears_once`。

### Requirement: Pager removed queued prompt Chat wire exclusion ignored PTY check
The ignored removal test SHALL queue alpha and bravo, remove the first selected row, release the foreground turn and require Chat user messages contain bravo but not alpha. It does not inspect Responses, queue RPC acknowledgement or persisted state.

#### Scenario: Remove alpha
- **WHEN** x is pressed on the focused first queue row
- **THEN** alpha disappears from the current screen.

#### Scenario: Promote survivor
- **WHEN** the foreground turn is released
- **THEN** Chat user messages eventually contain bravo and exclude alpha.

证据：`crates/codegen/pager/tests/pty_e2e/removed_queued_prompt_never_sent.rs` — `removed_queued_prompt_never_sent`。

### Requirement: Pager parallel same file edit row merge ignored PTY check
The ignored parallel-edit test SHALL enqueue two same-file search_replace calls and require one current-screen `Edit parallel_fix.py +2/-2` row with no `+1/-1`. It does not read the resulting file or prove both tool calls succeeded.

#### Scenario: Merged display
- **WHEN** both tool fixtures are sent in one model turn
- **THEN** one +2/-2 Edit row is visible.

#### Scenario: Runtime file
- **WHEN** the display assertion passes
- **THEN** final file contents remain unverified by this test.

证据：`crates/codegen/pager/tests/pty_e2e/edit_merge_parallel_pty.rs` — `edit_merge_parallel_pty`。

### Requirement: Pager idle double Esc rewind picker ignored PTY check
The ignored rewind test SHALL make the first Esc visually silent and the second open the picker from both prompt and confirmed scrollback focus. It does not choose a rewind target, inspect pending state or test the default timing window.

#### Scenario: Prompt and scrollback
- **WHEN** an idle empty session has a prior turn
- **THEN** both panes open rewind only on the second Esc.

证据：`crates/codegen/pager/tests/pty_e2e/esc_esc_opens_rewind_picker_silent_first_press.rs` — `esc_esc_opens_rewind_picker_silent_first_press`。

### Requirement: Pager nested quote selection bar exclusion ignored PTY check
The ignored SSH selection test SHALL drag from the outer nested quote bar and require joined OSC52 payloads contain the quote text, no bar and no leading whitespace. It does not require exactly one clipboard write or drive real pointer input.

#### Scenario: Nested quote copy
- **WHEN** selection begins on the outer bar and spans the text
- **THEN** copied payload text excludes all rendered quote bars.

证据：`crates/codegen/pager/tests/pty_e2e/nested_quote_drag_copy_excludes_bars_pty.rs` — `nested_quote_drag_copy_excludes_bars_pty`。

### Requirement: Pager selection gap row drag ignored PTY check
The ignored gap test SHALL release on the blank row between two paragraphs and require joined OSC52 payloads contain both tokens. Its ignored focus wait and cross-payload join do not prove one clipboard write spans the gap.

#### Scenario: Blank-row release
- **WHEN** one motion lands and releases on the exact middle row
- **THEN** joined payloads contain the top and bottom tokens.

证据：`crates/codegen/pager/tests/pty_e2e/drag_over_gap_rows_does_not_freeze_head_pty.rs` — `drag_over_gap_rows_does_not_freeze_head_pty`。

### Requirement: Pager blank chrome whole block drag ignored PTY check
The ignored chrome-origin test SHALL drag sideways only on an in-block blank row and require joined OSC52 payloads start at the first paragraph and end at the second. Multiple writes can jointly satisfy these bounds, and exact middle content is not checked.

#### Scenario: Blank chrome drag
- **WHEN** press, motion and release remain on the blank row
- **THEN** joined payload boundaries match the whole block.

证据：`crates/codegen/pager/tests/pty_e2e/drag_from_chrome_stays_block_pty.rs` — `drag_from_chrome_stays_block_pty`。

### Requirement: Pager streaming verb group fold ignored PTY check
The ignored grouped-read test SHALL observe a singleton header, then `Read 2 files` before final completion, and finally `Read 3 files`. It does not explicitly assert tool expectations, exact row count or mixed-verb behavior.

#### Scenario: Streaming progression
- **WHEN** three paced read_file turns execute
- **THEN** one-file, midflight two-file and settled three-file labels are observed in order.

证据：`crates/codegen/pager/tests/pty_e2e/verb_group_streaming_fold_pty.rs` — `verb_group_streaming_fold_pty`。

### Requirement: Pager gap entry anchor selection ignored PTY check
The ignored SSH selection test SHALL press on the blank gap below Worked for, enter the message at epsilon and require joined OSC52 payload text equals epsilon. Its scrollback footer wait is ignored and real pointer input is not exercised.

#### Scenario: Enter from gap
- **WHEN** first motion enters epsilon from the blank press row
- **THEN** the copied payload equals that word.

证据：`crates/codegen/pager/tests/pty_e2e/drag_enters_content_from_gap_pty.rs` — `drag_enters_content_from_gap_pty`。

### Requirement: Pager Minimal ANSI native scrollback integrity ignored PTY check
The ignored Minimal test SHALL force an 80-row highlighted block into native scrollback and require every numbered row plus head/tail sentinels occurs exactly once in full_text, with wide markers intact. ANSI bytes, style and exact spacer layout are not compared.

#### Scenario: Native scrollback
- **WHEN** the large fenced response commits
- **THEN** its head appears specifically in native scrollback.

#### Scenario: Complete extraction
- **WHEN** scrollback and screen text are combined
- **THEN** all exact payload rows occur once and head precedes tail.

证据：`crates/codegen/pager/tests/pty_e2e/ansi_scrollback_content_integrity.rs` — `ansi_scrollback_content_integrity`。

### Requirement: Pager collapsed edit double click ignored PTY check
The ignored edit test SHALL first show a collapsed +2/-1 one-line header without its body marker, then emit two synthetic click pairs on the header and require the marker. It does not read the edited file or validate real double-click timing.

#### Scenario: Collapsed state
- **WHEN** the scripted edit turn settles
- **THEN** its header is visible and body marker hidden.

#### Scenario: Expand
- **WHEN** the header receives two click pairs
- **THEN** the body marker appears.

证据：`crates/codegen/pager/tests/pty_e2e/edit_collapsed_oneliner_pty.rs` — `edit_collapsed_oneliner_pty`。

### Requirement: Pager above prompt strip entry selection ignored PTY check
The ignored SSH test SHALL press the blank row above the prompt border, enter epsilon in the last message and require joined OSC52 payload equals epsilon. Footer focus confirmation is ignored, and live status/banner rows are not covered.

#### Scenario: Enter from strip
- **WHEN** motion jumps from the blank strip into epsilon
- **THEN** copied payload text equals epsilon.

证据：`crates/codegen/pager/tests/pty_e2e/drag_from_above_prompt_strip_pty.rs` — `drag_from_above_prompt_strip_pty`。

### Requirement: Pager prompt suggestion ghost Tab accept ignored PTY check
The ignored suggestion test SHALL show an accept hint, hide it on divergent input, restore it when input clears, then remove it on Tab and permit text appended to the accepted suggestion. Ghost identity is inferred from the hint and the accepted prompt is not submitted.

#### Scenario: Diverge and restore
- **WHEN** x then Backspace are typed
- **THEN** the accept hint disappears then returns.

#### Scenario: Accept
- **WHEN** Tab and suffix text are typed
- **THEN** the hint disappears and combined editable text renders.

证据：`crates/codegen/pager/tests/pty_e2e/prompt_suggestion_ghost_tab_accepts.rs` — `prompt_suggestion_ghost_tab_accepts`。

### Requirement: Pager Tab scrollback focus mode matrix ignored PTY checks
Three ignored tests SHALL require Tab exposes the scrollback footer under default, Vim and simple configurations. Simple mode additionally requires idle double-Esc draft clearing. Footer text is the focus proxy; reverse focus and other modes are not covered.

#### Scenario: Default and Vim
- **WHEN** Tab follows a completed turn
- **THEN** Space:prompt becomes visible in both modes.

#### Scenario: Simple
- **WHEN** a draft is cleared by double Esc and Tab is pressed
- **THEN** Space:prompt becomes visible.

证据：`crates/codegen/pager/tests/pty_e2e/tab_focuses_scrollback_in_vim_and_default_modes.rs`。

### Requirement: Pager stuck drag Esc style recovery ignored PTY check
The ignored lost-release test SHALL show selection through changed row background or inverse cells, then require Esc plus later motion restores those tracked cells to baseline with pager liveness. It does not inspect selection state or real lost mouse-up behavior.

#### Scenario: Latch then Esc
- **WHEN** a drag has no release and Esc precedes more motion
- **THEN** tracked row style returns to baseline.

证据：`crates/codegen/pager/tests/pty_e2e/stuck_drag_recovers_on_esc_pty.rs` — `stuck_drag_recovers_on_esc_pty`。

### Requirement: Pager recap header selection exclusion ignored PTY check
The ignored recap test SHALL drag its unique body token and require joined OSC52 payloads contain that token without the checked standalone or prefixed Recap header forms. It does not require one clipboard write or validate all possible label formatting.

#### Scenario: Body copy
- **WHEN** the recap body token is selected
- **THEN** joined payloads contain it without the checked header forms.

证据：`crates/codegen/pager/tests/pty_e2e/recap_header_not_in_selection_pty.rs` — `recap_header_not_in_selection_pty`。

### Requirement: Pager Linux middle click primary selection ignored PTY check
The Linux-only ignored fake-X11 test SHALL paste PRIMARY once on middle click, never paste CLIPBOARD, send PRIMARY to a user blob, and log exactly one xclip primary-read argv with no clipboard-selection argv. Real X11 and xsel fallback are not exercised.

#### Scenario: Middle click
- **WHEN** SGR middle down/up arrives on welcome
- **THEN** PRIMARY appears once and CLIPBOARD remains absent.

#### Scenario: Fake tool log
- **WHEN** argv is inspected
- **THEN** xclip primary read occurs once with no clipboard selection read.

证据：`crates/codegen/pager/tests/pty_e2e/middle_click_pastes_primary_linux.rs` — `middle_click_pastes_primary_linux`。

### Requirement: Pager wheel burst frame amplification ignored PTY check
The ignored wheel smoke SHALL move the marker viewport upward with 30 reports and capture between one and 30 frames, then survive a mixed-direction sequence. Exact rows, event classification and cadence are not asserted.

#### Scenario: Burst bound
- **WHEN** 30 wheel-up events are sent
- **THEN** viewport moves and frame count is within 1..30.

#### Scenario: Reversal smoke
- **WHEN** mixed directions follow
- **THEN** pager remains live without panicked.

证据：`crates/codegen/pager/tests/pty_e2e/wheel_burst_scrolls_viewport_without_frame_amplification.rs` — `wheel_burst_scrolls_viewport_without_frame_amplification`。

### Requirement: Pager misclassified wheel flood teleport cap ignored PTY check

The ignored flood test SHALL build 700 marker rows with GROW_SCROLL_SPEED=100, inject 60 zero-delay wheel-up reports, drain for 800ms, and require pager liveness without a panicked marker. The viewport must move upward by at least 20 marker rows, capture at least two synchronized frames, and average no more than 30 travelled rows per captured frame. This aggregate ratio does not prove the stated per-flush viewport/2 cap, exact classification, individual frame deltas, cadence or wall-clock delivery.

#### Scenario: Dense flood
- **WHEN** 60 wheel reports are written without host sleeps
- **THEN** the pager remains live and travels upward at least 20 marker rows.

#### Scenario: Aggregate teleport guard
- **WHEN** travel and synchronized frame count are sampled after 800ms
- **THEN** at least two frames exist and travel divided by frames is at most 30.

#### Scenario: Per-frame cap
- **WHEN** the aggregate ratio passes
- **THEN** individual flush or frame travel can still differ and is not measured here.

证据：`crates/codegen/pager/tests/pty_e2e/misclassified_wheel_flood_does_not_teleport_viewport.rs` — `misclassified_wheel_flood_does_not_teleport_viewport`。

### Requirement: Pager Read header path-only selection ignored PTY check

The ignored Read-header test SHALL disable verb grouping, seed a real read_file call for a file under the isolated HOME, settle the follow-up response, focus scrollback by requiring Space:prompt, and drag across the displayed filename coordinates. At least one OSC52 payload must appear; newline-joined payloads must contain the filename or canonical path and exclude the two checked `Read ` prefix forms. It does not require one clipboard write, exact path-only equality, full-path display, real clipboard state or exclusion of every possible label layout.

#### Scenario: Raw Read row
- **WHEN** group_tool_verbs is false and the scripted read settles
- **THEN** the filename appears in a selectable Read header.

#### Scenario: Path drag
- **WHEN** scrollback focus is confirmed and the filename span is dragged
- **THEN** joined OSC52 payloads contain the filename or canonical path without the checked Read-prefix forms.

#### Scenario: Atomic copy
- **WHEN** multiple OSC52 writes jointly satisfy the assertions
- **THEN** the test can pass without one exact path-only payload.

证据：`crates/codegen/pager/tests/pty_e2e/read_tool_header_selection_copies_path_only_pty.rs` — `read_tool_header_selection_copies_path_only_pty`。

### Requirement: Pager raw quote source marker selection ignored PTY check

The ignored raw-quote test SHALL enable Vim mode, render a quote as a pretty bar, focus scrollback, click the agent entry, press `r`, and require the source line `> QUOTE_ALPHA first`. Dragging that raw line must produce OSC52 output whose newline-joined payloads contain the source line and contain no pretty bar character. It does not compare the complete source byte-for-byte, require exactly one clipboard write, assert exact copied boundaries, use a real pointer or cover non-Vim raw-mode access.

#### Scenario: Pretty quote
- **WHEN** the response settles before raw toggle
- **THEN** a rendered quote bar is visible.

#### Scenario: Raw entry
- **WHEN** the agent entry is selected and `r` is pressed in Vim scrollback
- **THEN** the source `>` quote line appears.

#### Scenario: Raw copy
- **WHEN** that line is dragged
- **THEN** joined OSC52 payloads contain the source marker and no pretty bar.

证据：`crates/codegen/pager/tests/pty_e2e/quote_block_raw_mode_copy_keeps_source_pty.rs` — `quote_block_raw_mode_copy_keeps_source_pty`。

### Requirement: Pager welcome and empty-session transition ignored PTY checks

Three ignored PTY tests SHALL require the API-key home page to show the welcome sentinel without the `Type a message` placeholder; typing `hello` without Enter must remove the welcome sentinel and render the draft in a new session. In a fresh empty session at the default 50x120 geometry, the big logo motif must occur at column 40 or later, then disappear from the current screen after Enter yields the mock response. These checks do not exhaust the welcome menu, prove persisted session identity, constrain all logo geometry or exclude the motif from off-screen history.

#### Scenario: Home
- **WHEN** pager starts with the harness API-key environment
- **THEN** the welcome sentinel appears and the prompt placeholder is absent.

#### Scenario: Uncaught text
- **WHEN** hello is typed on home without Enter
- **THEN** a session prompt renders the draft and home disappears.

#### Scenario: Empty-state logo
- **WHEN** the new session has no scrollback content
- **THEN** the big motif is positioned at column 40 or later and disappears after response content renders.

证据：`crates/codegen/pager/tests/pty_e2e/welcome_screen.rs` — `welcome_screen`、`typing_any_character_starts_session_and_leaves_home`、`agent_empty_state_logo_shows_until_content_streams`。

### Requirement: Pager dashboard overlay keyboard backout ignored PTY check

The ignored dashboard-overlay test SHALL use kitty CSI-u Ctrl-backslash to open the dashboard from a completed session, repeatedly attach the only agent row, and verify five prompt/focus paths. Esc with a non-empty draft must remain in the overlay and show `press again to clear`; after Ctrl-U, Esc on the empty prompt must return to the dashboard. Left on an empty prompt, Ctrl-backslash inside the overlay, and Tab followed by Esc after 400ms must each return to the dashboard list. It identifies the overlay through the sole row and response sentinel, does not confirm a stable session ID, require a scrollback-focus footer, test multiple rows or inspect persisted navigation state.

#### Scenario: Open dashboard
- **WHEN** kitty CSI-u Ctrl-backslash is sent from an idle session
- **THEN** + New Agent appears.

#### Scenario: Drafted Esc
- **WHEN** the attached overlay prompt contains OVLDRAFT
- **THEN** Esc arms clear and does not show the dashboard list.

#### Scenario: Empty prompt exits
- **WHEN** the draft is cleared
- **THEN** Esc and Left each return freshly attached overlays to the dashboard.

#### Scenario: Universal and scrollback exits
- **WHEN** a fresh overlay receives Ctrl-backslash or Tab then Esc
- **THEN** each path returns to the dashboard list without panicked.

证据：`crates/codegen/pager/tests/pty_e2e/dashboard_overlay_tab_esc_backout_and_ctrl_backslash.rs` — `dashboard_overlay_tab_esc_backout_and_ctrl_backslash`、`attach_overlay`。

### Requirement: Pager shared scroll marker and streaming PTY fixtures

The PTY scroll support SHALL emit SGR wheel press reports at caller coordinates, sleeping only between reports when the requested interval is nonzero; a burst expands one button into a repeated sequence. Marker content is a fenced sentinel followed by zero-padded `MARKER-nnnn` rows, and topmost-visible lookup scans screen lines for the first parseable four-digit marker. The settled fixture waits for the last marker, requires marker zero off-screen and another marker visible, injects Tab, ignores failure to observe Space:prompt, quiesces and resets frame timing. The streaming fixture configures chunk delay and a blocked terminal expectation, waits for the last marker from the initial block, requires marker zero and STREAMDONE off-screen, and returns the unreleased expectation plus marker baseline. Frame counts and character totals come from the harness parser; this helper does not assert their relationship to actual timing or movement.

#### Scenario: Wheel sequence
- **WHEN** a mixed or repeated button list is supplied
- **THEN** one SGR press is injected per entry with sleeps only between nonzero-interval writes.

#### Scenario: Settled markers
- **WHEN** the full marker response has rendered
- **THEN** overflow and a visible marker are required before timing reset, while footer focus is best-effort.

#### Scenario: Streaming markers
- **WHEN** the initial block arrives while the completion expectation is held
- **THEN** the tail sentinel remains absent and an unreleased turn handle is returned.

#### Scenario: Marker scan
- **WHEN** screen lines contain malformed and parseable marker prefixes
- **THEN** the first parseable four-digit marker index is returned.

证据：`crates/codegen/pager/tests/pty_e2e/scroll.rs` — `send_wheel_sequence`、`spawn_bottom_pinned_marker_scrollback_with_env`、`spawn_streaming_marker_turn`、`topmost_visible_marker`。

### Requirement: Pager trackpad flood conditional under-travel ignored PTY check

The ignored trackpad-flood test SHALL create 700 settled markers under TERM_PROGRAM=iTerm.app and GROW_SCROLL_SPEED=100, send 40 wheel-up reports with nominal 6ms gaps, drain 800ms, and require liveness, no panicked marker and upward viewport movement. If travel is below 200 rows and captured frames are at most 12, it prints a compressed-burst SKIP message and returns success; in every other path it requires travel of at least 200 rows. It does not observe arrival gaps, prove trackpad classification, assert a per-flush cap, require a minimum frame count or fail every under-travel outcome.

#### Scenario: Normal flood
- **WHEN** the final travel is at least 200 rows
- **THEN** the test quits successfully after liveness and direction checks.

#### Scenario: Conditional skip
- **WHEN** travel is below 200 and frame count is at most 12
- **THEN** the test reports SKIP and succeeds without the travel floor.

#### Scenario: Paced under-travel
- **WHEN** travel is below 200 and frame count exceeds 12
- **THEN** the test fails the travel-floor assertion.

证据：`crates/codegen/pager/tests/pty_e2e/trackpad_flood_does_not_under_travel.rs` — `trackpad_flood_does_not_under_travel`。

### Requirement: Pager wheel flood ghost-frame character floor ignored PTY check

The ignored ghost-frame test SHALL create 240 settled marker rows, send 30 wheel-up reports with nominal 6ms gaps, drain 600ms, and require liveness without panicked plus a decreased top marker. Captured frame count must be between two and 30 inclusive, and every captured frame timing entry must report at least ten characters. It does not assert durations, exact movement per frame, exact classification or that every counted character difference was caused only by viewport motion.

#### Scenario: Visible movement
- **WHEN** the wheel flood drains
- **THEN** the topmost marker index decreases.

#### Scenario: Frame bounds
- **WHEN** 30 reports were sent
- **THEN** captured frame count is within 2..30.

#### Scenario: Character floor
- **WHEN** each captured frame timing is inspected
- **THEN** every entry reports at least ten characters.

证据：`crates/codegen/pager/tests/pty_e2e/wheel_flood_paints_no_ghost_frames.rs` — `wheel_flood_paints_no_ghost_frames`。

### Requirement: Pager bottom overscroll follow reengagement ignored PTY check

The ignored follow test SHALL start a gated 240-marker, 240-tail-word stream with 30ms chunks and forced one-line wheel pricing. Eight zero-delay wheel-up reports must decrease the top marker, which must remain unchanged across a further 500ms while deltas arrive. Sixty zero-delay wheel-down reports must leave the pager live and Responding; without further input, STREAMDONE must become visible within 40 seconds. Releasing the expectation must then clear Responding and satisfy the turn. The oversized down burst does not prove exactly which clamped event reengages follow or the single-event landing-versus-overscroll distinction.

#### Scenario: Park
- **WHEN** eight wheel-up reports arrive mid-stream
- **THEN** the viewport moves upward and its top marker stays fixed during later deltas.

#### Scenario: Overscroll
- **WHEN** sixty wheel-down reports overrun the live bottom
- **THEN** the still-held turn remains live and Responding.

#### Scenario: Follow tail
- **WHEN** no more input is sent while tail words continue
- **THEN** STREAMDONE becomes visible before the gate is released.

#### Scenario: Completion
- **WHEN** the expectation is released
- **THEN** Responding clears and the expected turn is satisfied.

证据：`crates/codegen/pager/tests/pty_e2e/wheel_overscroll_at_bottom_reengages_follow_mid_stream.rs` — `wheel_overscroll_at_bottom_reengages_follow_mid_stream`。

### Requirement: Pager streaming wheel viewport progress ignored PTY check

The ignored streaming-wheel test SHALL start a gated 240-marker stream with 160 tail words paced at 30ms, inject 30 wheel-up reports at nominal 6ms intervals, and drain 600ms. Before gate release, pager must remain live without panicked, the top marker must decrease, Responding must remain visible and STREAMDONE absent. Releasing the expectation must clear Responding within 40 seconds and satisfy the turn. The paced fixture cannot force ACP input starvation or prove a bounded scheduling latency, exact scroll distance, continuous ACP readiness or behavior under an adversarial event-loop interleaving.

#### Scenario: Mid-stream wheel
- **WHEN** the completion gate is held and tail deltas are paced
- **THEN** the top marker decreases while Responding remains and STREAMDONE is absent.

#### Scenario: Liveness
- **WHEN** the wheel burst drains
- **THEN** pager remains running without panicked.

#### Scenario: Release
- **WHEN** the blocked expectation is released
- **THEN** Responding clears and the turn expectation is satisfied.

证据：`crates/codegen/pager/tests/pty_e2e/wheel_scrolls_viewport_during_streaming_turn.rs` — `wheel_scrolls_viewport_during_streaming_turn`。

### Requirement: Pager parked wait markerless FIFO ignored PTY check

The Unix-only ignored park test SHALL start a flag-gated background command plus a foreground identity hold, extract the first task-id envelope found in recorded request bodies, script a 600-second wait for that task, and release the hold. While parked, current screen must show one command still running and Enter queues, with no Worked for marker. Submitting `hurry up please` must show one queued and must not render the fallback answer during a further second. Releasing the task flag must allow the fallback answer and idle state, after which current screen must contain the queued text and a Worked for marker without the checked cancellation marker. It does not distinguish which fallback request produced the answer, count promoted requests, inspect persisted transcript or call graceful quit; harness Drop owns bounded cleanup.

#### Scenario: Park
- **WHEN** the long task wait is active
- **THEN** watching and Enter-queues cues appear without a Worked for marker.

#### Scenario: Queue
- **WHEN** hurry up please is submitted during the park
- **THEN** one queued remains visible and no fallback answer appears during the next second.

#### Scenario: Release
- **WHEN** the background flag is written
- **THEN** the turn drains and the promoted current-screen row contains the text and a marker without the checked cancellation label.

证据：`crates/codegen/pager/tests/pty_e2e/endline_park_is_markerless.rs` — `parked_enter_queues_until_wait_finishes`。

### Requirement: Pager endline background auto-wake chain ignored PTY check

The Unix-only ignored wake test SHALL run at 70 rows, start three flag-gated background commands, settle the owner turn, and require one Worked for marker plus a three-command watching cue. Releasing the three flags in order must produce WAKE_REPLY_ONE through THREE, marker counts two through four, and watching counts two, one, then absent. Final current-screen order must be owner marker, completion chip, wake reply and wake marker for each task; exactly three chips and four markers must exist, and no marker line may include still running. It does not inspect durable transcript, task identities, request causality or graceful quit, and the tall current screen is the sole ordering source.

#### Scenario: Owner settles
- **WHEN** three background commands remain gated
- **THEN** one marker and a three-command watching cue appear.

#### Scenario: Sequential wake
- **WHEN** each flag is released in order
- **THEN** the matching wake reply appears, marker count advances and remaining-command count decreases.

#### Scenario: Final chain
- **WHEN** all three tasks complete
- **THEN** three chips and four markers alternate with the three replies in the asserted row-major order.

证据：`crates/codegen/pager/tests/pty_e2e/endline_wakeups_close_with_markers.rs` — `endline_wakeups_close_with_markers`。

### Requirement: Pager repeated wait park marker suppression ignored PTY check

The Unix-only ignored repark test SHALL start one gated background task, issue a four-second wait, run one foreground command, then issue a 600-second wait for the same extracted task id. Park one must show one-command and Enter-queues cues without Worked for; the intermediate command description must then render. During park two, Esc:cancel and braille spinner glyphs below that description must be absent while the one-command cue remains and no marker exists. Releasing the task must render the fallback answer, reach idle, leave exactly one current-screen Worked for marker and no panicked marker. It does not assert the first wait's result or elapsed time, persisted marker count, exact spinner row, unique task-id provenance or foreground command output.

#### Scenario: First park
- **WHEN** the four-second wait is active
- **THEN** park cues appear with no Worked for marker.

#### Scenario: Between parks
- **WHEN** the short wait returns
- **THEN** the foreground command description renders before the long wait.

#### Scenario: Second park
- **WHEN** the long wait blocks on the same task
- **THEN** watching remains while cancel chrome, lower braille glyphs and markers are absent.

#### Scenario: Terminal end
- **WHEN** the task flag is released
- **THEN** the final answer reaches idle with exactly one current-screen marker.

证据：`crates/codegen/pager/tests/pty_e2e/reparked_wait_stays_markerless.rs` — `reparked_wait_stays_markerless`。

### Requirement: Pager resumed wait spinner chrome ignored PTY check

The Unix-only ignored spinner test SHALL park the current turn on a 600-second wait for a real flag-gated task, require one-command and Enter-queues cues, and require Esc:cancel to disappear. Before releasing the task it sets 150ms response chunk pacing; RESUMED_STREAM must then appear, Esc:cancel must return within four seconds, and twelve 90ms screen samples must observe at least two distinct glyphs from the eight-glyph spinner set. Current screen must not contain `❯ RESUMED_STREAM`; after pacing is removed the turn must become idle without panicked. The glyph scan is global rather than tied to the status cell, and absence of that one prompt-prefixed substring does not independently prove durable same-turn identity.

#### Scenario: Parked chrome
- **WHEN** the blocking wait owns the turn
- **THEN** watching cues appear and Esc:cancel disappears.

#### Scenario: Resume
- **WHEN** the task completes while the fallback response is paced
- **THEN** RESUMED_STREAM and Esc:cancel become visible.

#### Scenario: Spinner motion
- **WHEN** twelve screen samples are taken
- **THEN** at least two allowed braille spinner glyphs are observed.

#### Scenario: Same-row proxy
- **WHEN** the resumed text is visible
- **THEN** the checked prompt-prefixed form is absent before the turn reaches idle.

证据：`crates/codegen/pager/tests/pty_e2e/spinner_reappears_after_wait_resumes.rs` — `spinner_reappears_after_wait_resumes`。

### Requirement: Pager Bash output sampled fold ignored PTY check

The Unix-only ignored Bash-output test SHALL establish a session, execute a 12-line successful `!` command, and require L12, L06, L01, L03 and L09 on the current screen. After scrollback focus is evidenced by Ctrl+e:, a synthetic double-click on Run (user) must remove L06; after 500ms, a second relocated double-click must restore L06. A 12-line command ending in false must subsequently show E12 and E06, and pager must not show panicked. It does not assert all 12 lines, exact order, exit status styling, truncation glyphs, persistent output or real double-click timing.

#### Scenario: Successful sample
- **WHEN** the 12-line command finishes
- **THEN** the five sampled head, middle and tail lines are visible.

#### Scenario: Fold cycle
- **WHEN** Run (user) receives two separated synthetic double-clicks
- **THEN** L06 disappears and then returns.

#### Scenario: Failure sample
- **WHEN** the 12-line command exits through false
- **THEN** E12 and E06 are visible without panicked.

证据：`crates/codegen/pager/tests/pty_e2e/bash_full_output_double_click_fold_pty.rs` — `bash_full_output_double_click_fold_pty`、`double_click_text`。

### Requirement: Pager shell-like Bash file completion ignored PTY check

The Unix-only ignored file-completion test SHALL disable as-you-type suggestions, point history at a missing file, and seed two alpha files, notes.md, an unrelated script and a spaced directory containing a sentinel file. In Bash mode, first Tab after `cat al` must fill `cat alpha_` while alpha_one is absent from that screen; after a 1500ms settle, second Tab must show both alpha candidates. After clearing, Tab on quoted `cat "no` must show the spaced directory; Down plus Tab must accept the directory with its quote open, and a later Tab must insert the sole inner file and close the quote. Enter must execute the resulting command and render the file sentinel. It does not assert notes.md presence or ranking, every unrelated candidate's absence, exact dropdown lifetime, quote escaping beyond this path or non-Bash shells.

#### Scenario: Common prefix
- **WHEN** first Tab follows `cat al`
- **THEN** the shared alpha_ prefix fills without the checked alpha_one dropdown row.

#### Scenario: Second Tab
- **WHEN** the refreshed common-prefix state settles
- **THEN** both alpha file candidates appear.

#### Scenario: Quoted directory
- **WHEN** Down and Tab choose the spaced directory
- **THEN** the open quote is preserved for drill-down.

#### Scenario: Single inner file
- **WHEN** the nested provider result settles and Tab is pressed
- **THEN** the filename and closing quote are inserted, and Enter prints its sentinel.

证据：`crates/codegen/pager/tests/pty_e2e/bash_mode_file_completion_shell_like.rs` — `bash_mode_file_completion_shell_like`、`seed_cwd`。

### Requirement: Pager Bash Tab token-only dropdown ignored PTY check

The Unix-only ignored dropdown test SHALL disable as-you-type suggestions, seed two equal-length file candidates and one matching Bash history row, and type `!cat SUGGEST`. An immediate screen snapshot before Tab must contain neither file. After Tab, both file rows must appear while the history sentinel is absent. Down plus Tab must accept the second file; appending Y must render `cat SUGGESTBBB.txtY`, close the dropdown so the first file is absent, and leave no panicked marker. It does not wait a negative pre-Tab interval, execute the accepted command, inspect provider requests, assert dropdown ordering directly or prove history absence outside the sampled screen.

#### Scenario: Immediate pre-Tab sample
- **WHEN** the Bash prefix has just been injected
- **THEN** neither seeded file is present in that screen snapshot.

#### Scenario: Token-only fetch
- **WHEN** Tab results land
- **THEN** both files appear and the seeded history sentinel is absent.

#### Scenario: Accept second row
- **WHEN** Down and Tab are followed by Y
- **THEN** the composed second filename renders while the first candidate disappears.

证据：`crates/codegen/pager/tests/pty_e2e/bash_mode_tab_completion_dropdown.rs` — `bash_mode_tab_accepts_dropdown_item_in_place`、`suggestions_env`。

### Requirement: Pager queued Bash edit execution ignored PTY check

The Unix-only ignored queue-edit test SHALL hold a streaming turn, enqueue a Bash printf command, open the queue pane, enter edit mode with the Run shell command label, prepend an EDITED printf plus comment marker, save and observe the edited row. After the foreground turn reaches its barrier and is released, the test must observe `CLAIMTHREE_EDITED_OK`, Run (user), no CLAIMTHREE substring in collected user-message blobs and no panicked marker. It does not assert that the original command output is absent after successful edited execution, count shell executions, inspect persisted queue kind, verify exact edited command text on wire or require the deliberately queued fallback model expectation to remain unclaimed.

#### Scenario: Edit Bash row
- **WHEN** the queued command is selected and e is pressed
- **THEN** Run shell command appears and the saved row contains EDITED.

#### Scenario: Drain
- **WHEN** the held foreground turn is released
- **THEN** the edited shell output and Run (user) chrome appear.

#### Scenario: Model exclusion
- **WHEN** all collected user-message blobs are inspected
- **THEN** none contains CLAIMTHREE.

#### Scenario: Duplicate original
- **WHEN** both original and edited commands execute
- **THEN** the final assertions can still pass if edited output is also visible.

证据：`crates/codegen/pager/tests/pty_e2e/verify_bashq_claim3_edit_keeps_bash.rs` — `verify_bashq_claim3_edit_keeps_bash`。

### Requirement: Pager mixed verb group fold member navigation ignored PTY check

The ignored verb-group test SHALL enable grouping and script three reads, two greps, one edit and two more reads. The settled screen must show `Read 3 files, Searched 2 patterns`, an independent `Read 2 files`, the edit filename and no a2.txt. With Ctrl+e: proving scrollback focus, a header double-click must expose a1, a2 and a3 below a caret-free header at the same screen row, with member zero carrying a right caret. Right must show member zero's unique body; Left must restore the expanded header and hide that body; a second Left must hide a1 and a2, restore the collapsed caret header at the original row and avoid panicked. It does not require a3 to be absent after collapse, assert edit success, inspect all member bodies, test Vim aliases or prove exact group persistence.

#### Scenario: Settled runs
- **WHEN** the eight scripted tool calls and final response complete
- **THEN** the mixed read/search header, separate edit row and second read header appear while a2 is hidden.

#### Scenario: Expand
- **WHEN** the first header is double-clicked with scrollback focus
- **THEN** all three read members appear below a stationary caret-free group header.

#### Scenario: Member navigation
- **WHEN** Right then Left operate on member zero
- **THEN** its body appears, then hides while the expanded group header returns.

#### Scenario: Collapse
- **WHEN** Left is pressed again
- **THEN** a1 and a2 hide and the right-caret group header remains at its original row.

证据：`crates/codegen/pager/tests/pty_e2e/verb_group_fold_expand_collapse_pty.rs` — `verb_group_fold_expand_collapse_pty`。

### Requirement: Pager verb group header and member scoped selection ignored PTY check

The ignored selection test SHALL enable grouping, settle two successful reads under an SSH clipboard route, focus scrollback by requiring Space:prompt, and scope each gesture to OSC52 payloads added after its starting count. Dragging the folded `Read 2 files` header must yield joined new payloads containing the label and excluding dragme1. After double-click expansion exposes dragme1, dragging that member must include dragme1 and exclude the header; dragging the expanded header must include the label and exclude dragme1. Each gesture may emit multiple payloads that are joined, so the test does not prove a single atomic clipboard write, exact equality, exclusion of dragme2, full path handling or real clipboard state.

#### Scenario: Folded header
- **WHEN** the aggregated label is dragged
- **THEN** new joined payloads include the label and exclude dragme1.

#### Scenario: Expanded member
- **WHEN** the group is expanded and dragme1 is dragged
- **THEN** new joined payloads include the member and exclude the header.

#### Scenario: Expanded header
- **WHEN** the header row is dragged after expansion
- **THEN** new joined payloads include the label and exclude dragme1.

#### Scenario: Multiple writes
- **WHEN** one gesture emits several new payloads
- **THEN** joining can satisfy the scoped assertions without one exact copy.

证据：`crates/codegen/pager/tests/pty_e2e/verb_group_header_drag_copy_pty.rs` — `verb_group_header_drag_copy_pty`、`wait_for_new_osc52`、`drag_copy`。

### Requirement: Pager verb group settings live relayout ignored PTY check

The ignored settings test SHALL seed grouping on, settle three read calls into `Read 3 files`, then use F2, a `group tool calls` filter, Enter and Space to show the setting as off. Within eight seconds the existing transcript must lose the group header and show t1, t2 and t3. Repeating the modal flow to show on must make the group header reappear while t2 is absent within eight seconds, without panicked. The helper may first press Space when the Space:prompt hint is visible and sends up to four Esc presses to leave settings. It does not inspect the written config, start a new session, require t1 and t3 absent after refold, assert exact cache invalidation or cover environment overrides.

#### Scenario: Toggle off
- **WHEN** the live transcript starts with one three-read header
- **THEN** the modal row reports off and all three member filenames become visible.

#### Scenario: Toggle on
- **WHEN** the same modal row is toggled again
- **THEN** it reports on, the group header returns and t2 disappears.

#### Scenario: Modal exit
- **WHEN** the toggle helper completes
- **THEN** up to four Esc presses are used until Group tool calls and Appearance labels are absent.

证据：`crates/codegen/pager/tests/pty_e2e/verb_group_settings_toggle_pty.rs` — `verb_group_settings_toggle_pty`、`toggle_group_tool_calls`。

### Requirement: Pager thinking member verb group fold ignored PTY check

The ignored thinking-fold test SHALL enable verb grouping and thinking blocks, stream a reasoning sentinel followed by one read, then perform a second read. With 350ms chunk pacing, the reasoning sentinel must be observed before the final sentinel. Once settled, `Read 2 files` must appear while both the Thought for header and reasoning body are absent. After Ctrl+e: confirms scrollback focus and the group header is double-clicked, Thought for must appear as a collapsed member while the reasoning body remains absent, with no panicked marker. It does not open the thought member, assert its duration, inspect request protocol selection, prove durable fold state or test grouping when thinking ingestion is disabled.

#### Scenario: Streaming thought
- **WHEN** the paced reasoning response is in flight
- **THEN** the reasoning sentinel appears before final completion.

#### Scenario: Settled fold
- **WHEN** both reads and final response settle
- **THEN** the tools-only header appears with neither thought header nor reasoning body.

#### Scenario: Expanded group
- **WHEN** the tools header is double-clicked under scrollback focus
- **THEN** a Thought for member row appears while its body stays hidden.

证据：`crates/codegen/pager/tests/pty_e2e/verb_group_thinking_fold_pty.rs` — `verb_group_thinking_fold_pty`。

### Requirement: Pager edit highlight in-place style-change artifact ignored PTY check

The ignored edit-highlight test SHALL enable expanded edit blocks, create a Python fixture with 2500 padding lines, and script one search_replace whose target line contains both `min_length=2` and `upgrade target`. It samples raw PTY chunks and fully painted styled rows until that target's serialized style-run snapshot first appears and later differs while the final sentinel is visible, then requires no panicked marker. It writes first/upgraded/final text and HTML, raw ANSI and an asciicast under fixed `/tmp/edit_hl_video`; most artifact writes ignore errors, while directory and cast creation are required. The final HTML must contain any style attribute or span. It does not assert specific target colors, token scopes, unchanged text identity beyond the two substrings, edit file contents, artifact isolation or graceful quit.

#### Scenario: Initial style
- **WHEN** a fully painted row contains both target substrings
- **THEN** its serialized style runs become the baseline and optional first-frame artifacts are attempted.

#### Scenario: In-place change
- **WHEN** a later matching row serializes differently and final response is visible
- **THEN** the changed style snapshot satisfies the upgrade assertion.

#### Scenario: Artifacts
- **WHEN** the flow completes
- **THEN** fixed-path final dumps and cast are written or attempted, and some HTML styling markup must exist.

#### Scenario: Color semantics
- **WHEN** unrelated style markup or a non-syntax style change occurs
- **THEN** the assertions may still pass.

证据：`crates/codegen/pager/tests/pty_e2e/edit_hl_inplace_refresh_pty.rs` — `edit_hl_inplace_refresh_pty`、`target_line_style_snapshot`、`write_asciicast`。

### Requirement: Pager sequential edit merge and text break ignored PTY check

The ignored edit-merge test SHALL apply three sequential, widely separated one-line search_replace calls to one file before agent text. The settled current screen must contain exactly one `Edit merge_fix.py +3/-3` header and no +1/-1 or +2/-2 text. Double-click expansion must show at least one unchanged-lines marker plus the first and third edit markers; folding must remove the first marker. A fourth same-file edit in a later user turn after the agent-text break must show its own +1/-1 row; after 30 zero-delay wheel-up reports bring the earlier row back, exactly two matching Edit header rows and both diffstats must be visible. It does not assert the second hunk, exact gap counts, final file contents, tool results, durable grouping or exact scroll distance.

#### Scenario: Sequential run
- **WHEN** three same-file edits precede the agent-text break
- **THEN** one +3/-3 header remains and per-call diffstats are absent.

#### Scenario: Expanded samples
- **WHEN** the merged header is double-clicked
- **THEN** an unchanged-lines marker and first and third hunk markers appear.

#### Scenario: Text break
- **WHEN** a fourth edit runs in the next prompt
- **THEN** its +1/-1 row remains separate.

#### Scenario: Combined screen
- **WHEN** wheel-up restores the earlier group
- **THEN** exactly two Edit header rows with +3/-3 and +1/-1 are visible.

证据：`crates/codegen/pager/tests/pty_e2e/edit_merge_sequential_pty.rs` — `edit_merge_sequential_pty`、`edit_header_rows`。

### Requirement: Pager queued message screen and blob uniqueness ignored PTY check

The Unix-only ignored queued-message test SHALL hold a foreground tool while one background task runs, queue `queued exactly once probe`, then enter a 600-second wait for the extracted task id. During park, the one-queued hint must appear, Worked for must be absent, exactly one current-screen line must contain the queued text, and zero collected user-message blobs may contain it. After task release, a prompt-prefixed queued block must appear; within ten seconds exactly one current-screen line must contain the text, and exactly one collected user-message blob may contain it, without panicked. The wire assertion counts blobs containing the substring rather than substring occurrences inside a blob, and current-screen line counts do not cover off-screen or durable duplicates.

#### Scenario: Held row
- **WHEN** the owner turn parks with the queued follow-up
- **THEN** one current-screen row contains it and no user blob contains it.

#### Scenario: Drain
- **WHEN** the task flag is released
- **THEN** the queued text appears as a prompt-prefixed turn.

#### Scenario: Sampled uniqueness
- **WHEN** the drained state settles
- **THEN** one current-screen line and one matching user blob are counted.

#### Scenario: In-blob duplicate
- **WHEN** one user blob contains the probe twice
- **THEN** the matching-blob count remains one.

证据：`crates/codegen/pager/tests/pty_e2e/queued_message_renders_once_not_twice.rs` — `queued_message_renders_once_not_twice`。

### Requirement: Pager auto-compact first-content-row ignored PTY checks

Two ignored PTY tests SHALL use an 18-row terminal and define layout position as the first screen row containing any non-whitespace. From a normal-height settled agent view, that row must be greater than zero; after resize to 18 rows it must equal zero while pager remains live; resizing back must restore a value greater than zero. A pager spawned directly at 18 rows must likewise reach a settled agent view with first content at row zero without a resize event. Neither test identifies the row-zero content as the status bar, reads the persisted compact setting, checks horizontal layout, tests the exact 20-row threshold or covers heights at or below 16.

#### Scenario: Shrink
- **WHEN** a settled normal-height agent view is resized to 18 rows
- **THEN** first nonblank content moves from below row zero to row zero.

#### Scenario: Grow
- **WHEN** the short view returns to the default height
- **THEN** first nonblank content returns below row zero.

#### Scenario: Short startup
- **WHEN** pager starts at 18 rows and enters an agent session
- **THEN** first nonblank content is already on row zero without resize.

#### Scenario: Content identity
- **WHEN** unrelated content occupies row zero
- **THEN** the position assertion can still pass.

证据：`crates/codegen/pager/tests/pty_e2e/auto_compact_top_row.rs` — `auto_compact_top_row`、`auto_compact_at_startup`、`first_content_row`。

### Requirement: Pager Linux Otty bracketed image probe gating ignored PTY check

The Linux-only ignored IME test SHALL place fake wl-paste and wl-copy executables before inherited PATH, advertise a fake Wayland display, serve an image plus mutable clipboard text, and drive each pager to the dashboard. Under TERM_PROGRAM=otty, bracketed Chinese text that differs from empty clipboard text must render without an Image chip after 500ms; when clipboard text is changed to the bracketed caption, Image #1 must appear. In a fresh pager without TERM_PROGRAM, the same mismatched Chinese bracketed payload with empty clipboard text must still produce Image #1. It does not use a real Wayland compositor, record clipboard tool calls, verify image bytes or caption attachment, cover the agent prompt, macOS Otty or terminals other than the absent-name control.

#### Scenario: Otty mismatch
- **WHEN** bracketed Chinese text differs from clipboard text
- **THEN** the text renders and no Image chip is visible after 500ms.

#### Scenario: Otty match
- **WHEN** bracketed payload equals the mutable clipboard caption
- **THEN** Image #1 appears.

#### Scenario: Unnamed terminal
- **WHEN** the mismatched payload is sent in a fresh pager without TERM_PROGRAM
- **THEN** the historical image probe path produces Image #1.

证据：`crates/codegen/pager/tests/pty_e2e/bracketed_ime_paste_skips_clipboard_image_linux.rs` — `bracketed_ime_paste_skips_clipboard_image_linux`、`spawn_on_dashboard`。

### Requirement: Pager iTerm raw readline picker and dashboard editing ignored PTY check

The ignored iTerm test SHALL set TERM_PROGRAM=iTerm.app, settle a session renamed to ITERMROW, and open the command palette with Ctrl-P. Raw Option-Backspace must remove ITERMDELETE; Meta-B, inserted MID, Meta-F and END must yield `ITERMONE MIDITERMWORDEND`. Esc must clear then close the palette. Kitty Ctrl-backslash must open the dashboard; clicking the titled row must attach its overlay, Esc must return, and Ctrl-R must open an empty rename editor. Raw Alt-Left, MID, Alt-Right and END must produce and commit `LEFT MIDRIGHTEND`. Two Ctrl-Q presses must render confirmation and exit with status zero. It injects byte sequences under an environment label rather than a real iTerm process, and does not inspect persisted title storage, cursor cells, other modifier encodings or Unicode editing.

#### Scenario: Palette words
- **WHEN** Option-Backspace and Meta-B/F sequences edit the palette query
- **THEN** the deleted and recomposed expected query states render.

#### Scenario: Dashboard row
- **WHEN** Ctrl-backslash opens the list and the titled row is clicked
- **THEN** the attached Dashboard overlay appears and Esc returns to the list.

#### Scenario: Rename words
- **WHEN** Alt-Left/Right surround an inserted MID token
- **THEN** LEFT MIDRIGHTEND is committed and visible.

#### Scenario: Quit
- **WHEN** Ctrl-Q is pressed twice
- **THEN** confirmation appears and the child exits zero.

证据：`crates/codegen/pager/tests/pty_e2e/iterm_readline_editing.rs` — `iterm_raw_readline_sequences_edit_picker_and_dashboard_rename`、`click_visible_text`。

### Requirement: Pager mid-text skill foreground propagation ignored PTY check

The ignored skill-style test SHALL create a trusted Git workspace with `.grow/skills/test-skill/SKILL.md`, type `great /test-skill do it` from home, and poll styled rows until the run containing `/test-skill` has a foreground value different from the run containing `great`. After submit and final response, the echoed row's token and body foreground values must differ, and the echo token foreground must equal the composer token foreground, without panicked. A foreground may be absent on one side as long as the Option values differ; the test does not require a named teal color, inspect theme tokens, execute or expand the skill, verify model input, handle a token split across runs or cover untrusted/non-Git workspaces.

#### Scenario: Advertised token
- **WHEN** the workspace skill registry reaches the composer
- **THEN** token and body foreground Option values differ.

#### Scenario: Echo
- **WHEN** the prompt is submitted and response settles
- **THEN** the echoed token still differs from its body word.

#### Scenario: Propagation
- **WHEN** composer and echo token foreground values are compared
- **THEN** their Option values are equal.

#### Scenario: Skill behavior
- **WHEN** the styled prompt reaches the model
- **THEN** no expansion or execution assertion is made.

证据：`crates/codegen/pager/tests/pty_e2e/mid_text_skill_token_echo_styled_pty.rs` — `mid_text_skill_token_echo_styled_pty`、`run_fg_on_row`、`seed_test_skill`。

### Requirement: Pager background shell PID exit cleanup ignored PTY check

The Unix-only ignored cleanup test SHALL script a background shell command that writes `$$`, waits on `/bin/sleep 600`, then would write its exit result. After the tool turn settles, the pidfile must exist, parse above one and identify a live process. Sending real SIGINT to pager must yield either Exited with any code or PendingStatus within 15 seconds, and kill(pid, 0) for the recorded shell PID must fail within another 15 seconds. Because `$$` records the shell while sleep is a child followed by another shell command, the test does not identify or assert death of the sleep descendant, distinguish ESRCH from permission errors, require pager exit zero or verify process-group/session ownership.

#### Scenario: Background witness
- **WHEN** the scripted tool call settles
- **THEN** the shell-written PID exists and kill-zero reports it alive.

#### Scenario: Pager signal
- **WHEN** SIGINT is sent to the real pager process
- **THEN** exit observation is Exited or PendingStatus within 15 seconds.

#### Scenario: Recorded cleanup
- **WHEN** pager exit is observed
- **THEN** kill-zero for the recorded shell PID stops succeeding within 15 seconds.

#### Scenario: Descendant
- **WHEN** the shell dies but its sleep child survives
- **THEN** the recorded-PID assertion can still pass.

证据：`crates/codegen/pager/tests/pty_e2e/background_task_reaped_on_quit.rs` — `background_task_reaped_on_quit`、`pid_alive`。

### Requirement: Pager extensions plugin contextual footer hints ignored PTY check

The ignored extensions test SHALL seed one enabled and one disabled user plugin in one collapsed source group, open `/plugins`, expand the two-plugin group and require both rows. The Plugins footer must show `a install`, exclude `a add` and `space toggle`. Filtering and selecting the disabled row must show the exact contextual `space enable` rather than the combined fallback while retaining install; selecting the enabled row must show `space disable` with install. A no-match filter must show `space enable/disable`. It does not press Space, verify enablement mutation or persistence, inspect plugin details, cover other tabs, sources or keyboard labels, or assert the no-match list is otherwise empty.

#### Scenario: Plugin tab
- **WHEN** the seeded source group is expanded
- **THEN** both plugin rows and `a install` appear without the two legacy labels.

#### Scenario: Disabled selection
- **WHEN** search commits the disabled plugin and j selects its row
- **THEN** the footer shows contextual `space enable` plus install.

#### Scenario: Enabled selection
- **WHEN** search commits the enabled plugin and j selects its row
- **THEN** the footer shows `space disable` plus install.

#### Scenario: No match
- **WHEN** search commits an absent plugin name
- **THEN** the footer falls back to `space enable/disable`.

证据：`crates/codegen/pager/tests/pty_e2e/extensions_modal_copy_hints_pty.rs` — `extensions_modal_copy_hints_pty`、`screen_has_space_verb`、`seed_plugins_for_copy_hints`。

### Requirement: Pager pretty quote selection prefix exclusion ignored PTY check

The ignored quote-selection test SHALL settle two consecutive pretty quote rows under an SSH clipboard route, require a bar exactly two scalar columns before the first token, and focus scrollback by Space:prompt. A synthetic drag starts on that bar, moves through both text rows and remains held; the first row's serialized per-character background/inverse cells must change, while the bar and following space cells stay at baseline and the first content cell changes. On release, at least one OSC52 payload must exist; newline-joined payloads must contain both quote lines separated by newline, contain no bar and not start with whitespace. It does not require one clipboard write, exact payload equality or trailing boundary, real pointer/clipboard behavior, wide-character cell mapping or persistence of selection styling.

#### Scenario: Quote geometry
- **WHEN** the settled pretty quote is located
- **THEN** the second row is consecutive and a bar exists two scalar positions before alpha.

#### Scenario: Held overlay
- **WHEN** drag begins on the bar and extends through bravo without release
- **THEN** content styling changes while bar and prefix space styling remain baseline.

#### Scenario: Clipboard
- **WHEN** the drag releases
- **THEN** joined OSC52 payloads contain both lines without bar or leading whitespace.

#### Scenario: Multiple writes
- **WHEN** separate payloads jointly contain the expected text
- **THEN** the joined assertion can pass without one atomic copy.

证据：`crates/codegen/pager/tests/pty_e2e/quote_block_drag_copy_excludes_bars_pty.rs` — `quote_block_drag_copy_excludes_bars_pty`、`row_cells`。

### Requirement: Pager auto-wake cancel queued prompt replay ignored PTY check

The first Unix-only ignored auto-wake test SHALL start a six-second background sleep, settle the owner, extract the first task id from recorded request bodies, then script the synthetic wake to poll that task and run a 15-second foreground hold. Once a later request contains `=== Task <id> ===`, the test submits a CLARIFY marker, sends one Ctrl-C, and requires some recorded request body to contain the marker within 20 seconds. After graceful quit and `--continue` in the same HOME, full replay must contain that marker; replay of TURN1 or the marker is also sampled redundantly. It does not count marker occurrences, identify the exact user item or request, require the cancel marker, prove which turn Ctrl-C stopped, require the best-effort settle wait to succeed or assert one durable copy.

#### Scenario: Synthetic wake
- **WHEN** the completed-task poll result reaches a later request
- **THEN** the foreground hold keeps the wake running.

#### Scenario: Queued user prompt
- **WHEN** CLARIFY is submitted and Ctrl-C follows
- **THEN** at least one later request body contains CLARIFY.

#### Scenario: Resume
- **WHEN** the first pager quits and `--continue` starts in the same HOME
- **THEN** full replay contains CLARIFY.

#### Scenario: Uniqueness
- **WHEN** wire or replay contains multiple CLARIFY copies
- **THEN** the at-least-one assertions still pass.

证据：`crates/codegen/pager/tests/pty_e2e/auto_wake_cancel_preserves_queued_user_prompt.rs` — `auto_wake_cancel_preserves_queued_user_prompt`。

### Requirement: Pager cancelled turn deferred completion reminder ignored PTY check

The second Unix-only ignored auto-wake test SHALL run one gated background task and hold the ordinary model turn at its terminal boundary. Ctrl-C followed by gate release must render Turn cancelled by user and reach idle before the task flag is written. Task release must add a completion chip, while a two-second stable window within five seconds contains no fallback auto-wake response. The next POST_CANCEL user request must include its own marker, Background task, completed and the task id in one serialized request body, then receive the fallback response. During a final stable window, exactly one recorded request body may contain POST_CANCEL. This counts matching bodies rather than occurrences or structured user items, and does not exclude a delayed synthetic request outside the sampled windows or inspect durable reminder consumption.

#### Scenario: Cancel ordinary turn
- **WHEN** the model completion boundary is held
- **THEN** Ctrl-C renders cancellation and reaches idle before task completion.

#### Scenario: Deferred wake
- **WHEN** the background task then completes
- **THEN** a chip appears while the fallback response stays absent for the stable window.

#### Scenario: User consumption
- **WHEN** POST_CANCEL is submitted
- **THEN** one request body contains the user marker plus completion reminder fields.

#### Scenario: Matching body count
- **WHEN** one body contains POST_CANCEL more than once
- **THEN** the final count still equals one.

证据：`crates/codegen/pager/tests/pty_e2e/auto_wake_cancel_preserves_queued_user_prompt.rs` — `cancel_before_task_completion_defers_auto_wake_until_user_prompt`、`unified_log_diagnostics`。

### Requirement: Pager basename Read header demo fallback ignored PTY check

The ignored demo generator SHALL disable verb grouping, read a file under `very/deep/nested/project/src/module`, and require a settled Read header line containing the basename but neither the full path, `very/deep` nor `/module/`. It samples raw PTY output and attempts collapsed/final text and HTML plus raw ANSI and assertions files under fixed `/tmp/basename_path_video`, while cast creation is required. After Tab, a best-effort Space:prompt wait and an optional click, Ctrl-F is attempted; if the full path is not detected within 15 seconds, Esc, another optional click and Enter provide a fallback. Final screen must satisfy a fuzzy full-path predicate using either the exact path, newline-stripped full nest plus filename, or separate `very/deep`, `nested/project` and filename fragments. It does not prove Block Viewer opened, contiguous full-path display, focus ownership, artifact isolation or a specific expanded surface.

#### Scenario: Collapsed header
- **WHEN** the read and final response settle
- **THEN** a Read line shows the basename without the checked parent/full-path forms.

#### Scenario: Viewer attempt
- **WHEN** Tab, optional row click and Ctrl-F are sent
- **THEN** the test waits for its fuzzy full-path predicate.

#### Scenario: Fallback
- **WHEN** the predicate is still false
- **THEN** Esc, optional re-click and Enter attempt an expanded view.

#### Scenario: Demo artifacts
- **WHEN** the final fuzzy predicate passes
- **THEN** fixed-path screen, HTML, ANSI and cast outputs are written or attempted.

证据：`crates/codegen/pager/tests/pty_e2e/basename_path_demo_pty.rs` — `basename_path_demo_pty`、`screen_shows_full_path`、`write_asciicast`。

### Requirement: Pager width reflow marker anchor ignored PTY check

The ignored resize test SHALL render a response with ten wrapping paragraphs, sixteen short guards, one ASCII anchor and thirty short rows below. Synthetic wheel reports must place the marker at screen row 20..32 while both top and bottom sentinels are absent. A width-only resize from 120 to 80 columns at the default row count must leave pager live without panicked, keep the marker visible, and change its screen row by at most two. The parking helper ignores individual wheel injection errors and may return a final marker position after its deadline; the test does not assert exact logical top content, wrapped row counts, follow-state internals, focus after Esc, vertical or repeated resize, Unicode reflow or restoration to the wide width.

#### Scenario: Middle park
- **WHEN** wheel refinement succeeds
- **THEN** the marker is within rows 20..32 and neither transcript sentinel is visible.

#### Scenario: Width reflow
- **WHEN** columns change from 120 to 80 while rows stay fixed
- **THEN** the marker remains visible with absolute row drift at most two.

#### Scenario: Liveness
- **WHEN** the narrow layout settles
- **THEN** pager remains running without panicked.

#### Scenario: Other anchors
- **WHEN** content above the marker changes while its row stays close
- **THEN** the test does not compare the viewport's exact logical top line.

证据：`crates/codegen/pager/tests/pty_e2e/resize_preserves_scroll_position.rs` — `resize_preserves_scroll_position`、`park_marker_mid`、`scroll_anchor_response`。

### Requirement: Pager thinking visibility settings live toggle ignored PTY check

The ignored thinking-settings test SHALL seed show_thinking_blocks on and provide equivalent word-streamed reasoning plus answer fixtures for Responses and Chat protocols. After the answer and collapsed Thought for header appear, a helper may try up to eight Up/Enter cycles under confirmed scrollback focus to reveal the body; the pre-toggle requirement accepts either the body sentinel or the existing header. F2 filtering, Enter and Space must show the setting off; within eight seconds both body sentinel and header must be absent. Repeating the modal flow to on must make either body or header reappear within eight seconds. It does not require body expansion to succeed, inspect persisted config, distinguish which protocol ran, assert no panicked marker, preserve expansion state or verify future-turn ingestion behavior.

#### Scenario: Initial thinking
- **WHEN** the scripted reasoning turn settles
- **THEN** Thought for appears and optional navigation may reveal the body sentinel.

#### Scenario: Hide
- **WHEN** the setting row is toggled to off
- **THEN** both the current body sentinel and header disappear.

#### Scenario: Restore
- **WHEN** the setting row is toggled back to on
- **THEN** either body sentinel or header reappears.

#### Scenario: Expansion attempt
- **WHEN** all eight Up/Enter cycles miss the body
- **THEN** the original Thought for header still satisfies the pre-toggle assertion.

证据：`crates/codegen/pager/tests/pty_e2e/show_thinking_blocks_toggle_hides_existing_pty.rs` — `show_thinking_blocks_toggle_hides_existing_pty`、`toggle_show_thinking_blocks`、`responses_api_with_reasoning_stream`、`chat_completion_with_reasoning_stream`。

### Requirement: Pager drag autoscroll scrolled-out block selection ignored PTY check

The ignored drag-autoscroll test SHALL render a three-line anchor block followed by 120 marker rows under the SSH OSC52 clipboard route. After synthetic wheel-up reveals the anchor, a synthetic drag begins on its first token and is held at the bottom edge until the topmost visible filler marker is at least four and both anchor endpoints are absent. Releasing must emit at least one OSC52 payload; newline-joined payloads must contain both anchor endpoints and no MARKER-, while pager remains live without panicked. It does not require one atomic clipboard write, exact selected text, a real pointer or clipboard, or directly inspect visible_blocks, the saved content-width snapshot or head reclamping.

#### Scenario: Scrollout
- **WHEN** the held bottom-edge drag autoscrolls to marker four or later
- **THEN** both anchor endpoint strings are absent from the current screen.

#### Scenario: Clipboard
- **WHEN** the held drag releases
- **THEN** joined OSC52 payloads contain both anchor endpoints and no filler marker.

#### Scenario: Liveness
- **WHEN** the gesture completes
- **THEN** pager is still running without panicked.

#### Scenario: Internals
- **WHEN** the screen and payload assertions pass
- **THEN** the claimed snapshot and reclamp mechanisms remain inferred rather than observed.

证据：`crates/codegen/pager/tests/pty_e2e/drag_select_autoscroll_full_scrollout_copy_pty.rs` — `drag_select_autoscroll_full_scrollout_copy_pty`、`topmost_visible_marker`。

### Requirement: Pager manual rename prompt border style ignored PTY check

The first ignored rename test SHALL settle a session, submit paced `/rename PTYRENAMETITLE`, and wait for the `Session renamed to` acknowledgement. A prompt top-border row must contain the title, a horizontal rule and the upper-right corner. Styled output must give the title neither inverse nor bold, keep its background equal to a plain border run and make its foreground differ from that rule; pager must not show panicked and double Ctrl-Q must exit zero. It does not inspect summary.json, require an exact color, measure the title's right alignment or prove that the acknowledgement follows a durable filesystem write.

#### Scenario: Rename
- **WHEN** the paced slash command is submitted
- **THEN** the acknowledgement and title-bearing border row appear.

#### Scenario: Style
- **WHEN** styled cells are inspected
- **THEN** the title is non-bold, non-inverse, shares border background and differs in foreground.

#### Scenario: Exit
- **WHEN** double Ctrl-Q is sent
- **THEN** the process exits with status zero.

#### Scenario: Durability
- **WHEN** the acknowledgement appears
- **THEN** the test does not read the persisted summary file.

证据：`crates/codegen/pager/tests/pty_e2e/rename_title_shows_in_prompt_border.rs` — `rename_title_shows_in_prompt_border`、`assert_title_styled`、`submit_rename`。

### Requirement: Pager manual rename resume and plain-border control ignored PTY check

The second ignored rename test SHALL create separate renamed and never-renamed project sessions in one isolated HOME, gracefully quit both, then resume each with `--continue`. The renamed replay must restore the known response, title-bearing border row and checked title style. After a two-second hydration allowance, the control session's prompt top border must consist only of the two corners and horizontal rules. Both resumed pagers must avoid panicked and exit zero. It does not inspect event or summary storage, wait for or identify an auto-title, prove cwd-to-session selection beyond observed replay, or verify title updates across concurrent processes.

#### Scenario: Renamed resume
- **WHEN** the renamed project starts with --continue
- **THEN** history, border title and checked style return.

#### Scenario: Plain resume
- **WHEN** the never-renamed project starts with --continue
- **THEN** its detected prompt top border remains structurally plain.

#### Scenario: Isolation
- **WHEN** the two sessions use separate project directories
- **THEN** each observed replay follows its project cwd.

#### Scenario: Persistence internals
- **WHEN** the title returns
- **THEN** no persisted event or summary file is read directly.

证据：`crates/codegen/pager/tests/pty_e2e/rename_title_shows_in_prompt_border.rs` — `rename_title_survives_resume_and_stays_absent_without_rename`、`prompt_top_border_row`、`is_plain_border_row`。

### Requirement: Pager scroll debug HUD environment and flood ignored PTY check

The ignored env-on HUD test SHALL spawn bottom-pinned 120-row marker scrollback with `GROW_SCROLL_DEBUG=1` and forced trackpad mode. Before scrolling, the screen must contain `scroll debug` and `mode:trackpad`. A 30-report upward burst at eight-millisecond spacing, a settle, and one repaint notch must leave pager live without panicked, show `last:trackpad`, and reduce the topmost visible marker index. It proves a visible runtime overlay and observed upward travel, but not every report's classification, exact travel, flush cadence, overlay geometry or absence of rendering interference beyond the marker comparison.

#### Scenario: Initial HUD
- **WHEN** debug and trackpad env overrides are active
- **THEN** the HUD title and forced mode echo are visible.

#### Scenario: Finalized breadcrumb
- **WHEN** the flood settles and a repaint notch follows
- **THEN** last:trackpad appears.

#### Scenario: Observer travel
- **WHEN** the flood is delivered with the HUD on
- **THEN** the visible marker index moves upward.

#### Scenario: Granularity
- **WHEN** the final screen assertions pass
- **THEN** per-report classification and frame timing remain unobserved.

证据：`crates/codegen/pager/tests/pty_e2e/scroll_debug_hud_env_toggles_overlay.rs` — `scroll_debug_hud_env_shows_hud_and_tracks_flood`。

### Requirement: Pager scroll debug HUD default-off ignored PTY check

The ignored default-off HUD test SHALL spawn the same bottom-pinned marker scrollback without the debug environment override and require the current screen not to contain `scroll debug`, then cleanly quit. It does not send scroll input, sample a stable negative window, check other HUD strings or distinguish hidden overlay state from absence of its title text.

#### Scenario: Default
- **WHEN** marker scrollback starts without GROW_SCROLL_DEBUG
- **THEN** the current screen lacks the HUD title.

#### Scenario: Later frames
- **WHEN** a delayed frame would reveal the HUD
- **THEN** the test has already completed its single negative assertion.

证据：`crates/codegen/pager/tests/pty_e2e/scroll_debug_hud_env_toggles_overlay.rs` — `scroll_debug_hud_absent_without_env`。

### Requirement: Pager scroll debug command live toggle ignored PTY check

The ignored command-toggle test SHALL begin without the HUD title, send Space after a helper that leaves scrollback focused, then submit `/debug scroll` and require the title within five seconds. Submitting the same command again and waiting 500 milliseconds must remove the title while pager remains live without panicked. It does not inspect the runtime flag, persist or reload the setting, verify all HUD cells clear, exercise malformed arguments, or require an acknowledgement separate from title visibility.

#### Scenario: Enable
- **WHEN** the first /debug scroll command is submitted
- **THEN** the HUD title appears.

#### Scenario: Disable
- **WHEN** the command is submitted again
- **THEN** the HUD title is absent after the repaint delay.

#### Scenario: Liveness
- **WHEN** the round trip completes
- **THEN** pager remains running without panicked.

#### Scenario: State
- **WHEN** title visibility changes
- **THEN** the internal flag and persistence are not inspected.

证据：`crates/codegen/pager/tests/pty_e2e/scroll_debug_hud_env_toggles_overlay.rs` — `debug_scroll_command_toggles_hud_live`。

### Requirement: Pager repeated double-click word-select tip acceptance ignored PTY check

The Unix-only ignored word-select tip test SHALL seed flash selection mode with contextual hints enabled, settle selectable body text and confirm scrollback focus. The first synthetic double-click must leave the tip title absent after 600 milliseconds; a second gesture must show the tip plus either settings-path text and the Ctrl+Y chord. Ctrl+Y must show the word_select confirmation and an asynchronously polled config.toml must contain the exact assignment. A later double-click must leave the tip title absent and pager must avoid panicked. It does not assert the word was selected or copied, parse TOML structurally, verify one config key, use a real pointer, or prove the first negative over the full repeat window.

#### Scenario: First gesture
- **WHEN** one synthetic double-click lands mid-token
- **THEN** the tip title is absent after 600 milliseconds.

#### Scenario: Repeated gesture
- **WHEN** the second gesture lands within the test's timing
- **THEN** the tip advertises settings and Ctrl+Y.

#### Scenario: Accept
- **WHEN** Ctrl+Y is pressed while the tip is visible
- **THEN** a confirmation appears and config text eventually contains word_select.

#### Scenario: Post-accept
- **WHEN** another double-click is sent
- **THEN** the tip stays absent, without proving selection output.

证据：`crates/codegen/pager/tests/pty_e2e/word_select_tip_on_double_click_pty.rs` — `word_select_tip_shows_and_ctrl_y_accepts`、`double_click_at`。

### Requirement: Pager word-select mode suppresses selection tip ignored PTY check

The Unix-only ignored mode-gate test SHALL seed `keep_text_selection = word_select`, settle body text, confirm scrollback focus, send two synthetic double-click gestures separated by 600 milliseconds, and after another 800 milliseconds require the tip title absent and no panicked marker. It does not inspect selected cells, clipboard output or config after startup, and a missing tip alone does not prove word selection occurred.

#### Scenario: Mode gate
- **WHEN** two qualifying gestures occur while word_select is seeded
- **THEN** the tip title remains absent.

#### Scenario: Selection behavior
- **WHEN** the title remains absent
- **THEN** the test does not observe a selected word or clipboard payload.

证据：`crates/codegen/pager/tests/pty_e2e/word_select_tip_on_double_click_pty.rs` — `word_select_tip_skipped_when_mode_is_word_select`。

### Requirement: Pager word-select contextual hint opt-out ignored PTY check

The Unix-only ignored opt-out test SHALL write flash mode with `[ui.contextual_hints].word_select = false`, explicitly blank inherited `GROW_CONTEXTUAL_HINTS`, settle body text, confirm scrollback focus, and send two synthetic double-click gestures with the same 600/800-millisecond timing. The tip title must remain absent. It does not inspect parsed configuration, prove the environment master was otherwise unset, check selection output, assert pager liveness or scan for panicked.

#### Scenario: Per-tip opt-out
- **WHEN** two qualifying gestures occur with the word_select hint disabled
- **THEN** the tip title remains absent.

#### Scenario: Environment
- **WHEN** the pager is spawned
- **THEN** the inherited master override is replaced by an empty value.

#### Scenario: Liveness
- **WHEN** the negative assertion passes
- **THEN** this test does not separately poll the process or scan for panic text.

证据：`crates/codegen/pager/tests/pty_e2e/word_select_tip_on_double_click_pty.rs` — `word_select_tip_skipped_when_contextual_hint_disabled`。
### Requirement: Pager announcement replacement and hidden prune unit checks

The announcement unit tests SHALL route in-memory updates and verify snapshot replacement plus hidden-id pruning and a matching persistence effect. They do not execute persistence or cover expiry, empty, slash-gate and malformed paths.

#### Scenario: Replace
- **WHEN** a new announcement update arrives
- **THEN** active and current projections equal it.

#### Scenario: Prune
- **WHEN** a hidden id no longer exists
- **THEN** it is removed and a matching persistence effect exists.

证据：`crates/codegen/pager/src/app/acp_handler/tests/announcements.rs` — 两项测试。

### Requirement: Pager workflow update common projection unit check

The workflow unit test SHALL send one active revision through the public handler and require session and scrollback projections. It does not cover revision ordering, replay, deduplication or terminal lifecycle.

#### Scenario: Run
- **WHEN** the fixture update is sent
- **THEN** one active deep-research run with Research phase is stored.

#### Scenario: Block
- **WHEN** its workflow block is retrieved
- **THEN** phase and active-agent count match the update.

证据：`crates/codegen/pager/src/app/acp_handler/tests/workflows.rs` — `workflow_updates_use_the_common_public_projection`。

### Requirement: Pager child permission parent queue and unknown cancellation unit checks

The child-permission tests SHALL register a child, queue its permission on the parent while retaining identity, keep the response pending until standard allow-once selection, then resolve and pop it. An unregistered child SHALL receive Cancelled without queueing. Leader ordering and rendered UI are not exercised.

#### Scenario: Registered
- **WHEN** child permission follows spawn registration
- **THEN** it waits in the parent FIFO with child session id.

#### Scenario: Approve
- **WHEN** allow-once is selected
- **THEN** Selected resolves and the queue empties.

#### Scenario: Unknown
- **WHEN** the child was not registered
- **THEN** Cancelled resolves and the queue remains empty.

证据：`crates/codegen/pager/src/app/acp_handler/tests/subagent_permission_routing_tests.rs` — 两项测试。
### Requirement: Pager child cwd and git-head root direct-child routing unit checks

The git-head tests SHALL cover independent child cwd/worktree derivation plus root, direct-child and unknown-session cache-field routing. They do not cover shared cache contents, nested children, malformed payloads or real git state.

#### Scenario: Derivation
- **WHEN** child cwd, worktree and info presence vary
- **THEN** the four expected path/flag combinations hold.

#### Scenario: Routing
- **WHEN** root, direct child or unknown id is notified
- **THEN** only the matching view changes, or false returns.

证据：`crates/codegen/pager/src/app/acp_handler/tests/git_head.rs` — 八项测试。

### Requirement: Pager plugin push collapse seeding and installed notice unit checks

The plugin tests SHALL verify loaded-data refresh preserves prior collapse state, a first push seeds origin groups only once, later expansion survives, and installed update tuples become durable version text. Skills refetch and actual installation are not exercised.

#### Scenario: Seed
- **WHEN** the first push wins modal loading
- **THEN** user and config origins seed collapsed once.

#### Scenario: Preserve
- **WHEN** later data arrives after expansion
- **THEN** collapse state is retained.

#### Scenario: Installed
- **WHEN** two version updates arrive
- **THEN** durable notice text contains both transitions.

证据：`crates/codegen/pager/src/app/acp_handler/tests/plugins.rs` — 三项测试。

### Requirement: Pager goal projection retired wire replay and history isolation unit checks

The goal tests SHALL verify long-term projection fields, rejection of retired plan_markdown, replay-only hydration, behavior-independent state updates without transcript duplication, clear state bookkeeping, and exactly one collapsed command notice after stale post-clear updates. They do not exercise goal runtime or persistence.

#### Scenario: Projection
- **WHEN** an active update arrives
- **THEN** long-term goal fields map correctly.

#### Scenario: Retired
- **WHEN** plan_markdown appears
- **THEN** update is rejected with no goal state.

#### Scenario: Replay
- **WHEN** status sequence hydrates during replay
- **THEN** state changes without scrollback.

#### Scenario: Clear
- **WHEN** cleared and stale follow-up updates arrive
- **THEN** state stays cleared and only the command notice remains.

证据：`crates/codegen/pager/src/app/acp_handler/tests/goals.rs` — 四项测试。

### Requirement: Pager background task replay routing demotion and completion unit checks

The background-task unit module SHALL exercise generic replay restoration for TaskBackgrounded and scheduled create/delete; Execute-to-background demotion including late detection and description cleanup; root/direct-child routing for start, completion and monitor events; monitor prefix classification; session_restart quiet completion versus other-signal failure output; inactive-owner mutation with false redraw; unknown-session rejection; and replay marker storage. These are direct in-memory handler tests. They do not run an ACP transport, persisted replay, a background process, cumulative stdout parsing, scheduled firing, unknown-task completion, nested descendants, or the actual test binary.

#### Scenario: Replay envelope
- **WHEN** generic grow/session/update replays background and schedule lifecycle updates
- **THEN** background state restores and scheduled create/delete nets back to empty.

#### Scenario: Demotion
- **WHEN** a pending Execute is backgrounded normally or after late detection
- **THEN** the existing row stops running, tracking and deferred keys drain, and no duplicate row is created.

#### Scenario: Ownership
- **WHEN** start, completion or monitor events address a root or direct child
- **THEN** only the addressed concrete view mutates; an inactive owner still mutates but returns false.

#### Scenario: Monitor and description
- **WHEN** structured or legacy monitor data and blank wire descriptions vary
- **THEN** monitor classification and the tested deferred-description fallback hold.

#### Scenario: Completion
- **WHEN** a restored task ends with session_restart or a live task ends with another signal
- **THEN** the former finalizes quietly while the latter adds a failure block.

#### Scenario: Unknown and replay
- **WHEN** the session is unknown or notification metadata marks replay
- **THEN** unknown events return false; a restored task records restored_from_replay.

证据：`crates/codegen/pager/src/app/acp_handler/tests/background_tasks.rs` — 22 项测试。

### Requirement: Pager command feedback memory correlation and compaction replay unit checks

The command-feedback unit module SHALL verify that newer command progress survives an older terminal notice, its own terminal clears live feedback, and later progress for that completed correlation is rejected; replay progress stays non-live and duplicate event-id terminal notices produce one immutable row. Memory result checks require a matching pending live invocation, open the browser once, clear pending state on a command warning or replay start, and reject stale or replayed results. Manual compaction completion is appended immediately without a later finish-turn duplicate, while duplicate replay deliveries for manual, async, failed and cancelled terminals each leave one row. These tests use in-memory handlers and do not execute persistence, reconnect transport, memory loading, modal rendering, compaction, or the test binary.

#### Scenario: Progress ownership
- **WHEN** older and newer progress correlations coexist before the older terminal arrives
- **THEN** the newer live status remains until its own terminal clears it, and late progress is rejected.

#### Scenario: Replay notice
- **WHEN** progress and a duplicated terminal event id replay
- **THEN** progress does not animate and one terminal row remains.

#### Scenario: Memory correlation
- **WHEN** unsolicited, stale, matching and repeated memory results arrive
- **THEN** only the first matching live result opens a browser.

#### Scenario: Memory invalidation
- **WHEN** a command warning or begin_replay clears a pending browse
- **THEN** later live or replayed empty results do not open a browser.

#### Scenario: Manual compaction
- **WHEN** manual synchronous completion precedes finish_turn
- **THEN** the immutable completion row appears immediately and is not appended again.

#### Scenario: Compaction replay
- **WHEN** each supported terminal outcome is replayed twice with one event id
- **THEN** one scrollback row remains for that outcome.

证据：`crates/codegen/pager/src/app/acp_handler/tests/command_feedback.rs` — 六项测试。

### Requirement: Pager parked lifecycle and broadcast interjection unit checks

The interjection unit module SHALL verify that an interjection arriving while a turn is parked adds its prompt block without a turn marker, background completions and repeated subagent finishes remain markerless during that park, and re-parking after intervening parent output also creates no marker. Broadcast checks require a matching active root and an attached viewer to render, an unknown session to remain unchanged, an originator-owned interjection id to suppress its echo and be forgotten, and a foreign id to render. These direct in-memory fixtures do not exercise ACP transport, child-session interjection delivery, persistence/replay, terminal drawing, queue submission, model ingestion, or a real parked process.

#### Scenario: Parked interjection
- **WHEN** a broadcast arrives during a task-output park
- **THEN** the interjection appears and no turn marker is inserted.

#### Scenario: Parked completions
- **WHEN** background tasks or repeated subagents finish during the park
- **THEN** no turn markers or work-only status lines are introduced by the tested sequence.

#### Scenario: Re-park
- **WHEN** parent output occurs between two waits on the same task
- **THEN** the second park remains markerless.

#### Scenario: Root and viewer
- **WHEN** a broadcast addresses the root session in driver or viewer state
- **THEN** the interjection block renders and the active owner reports affected.

#### Scenario: Unknown
- **WHEN** the broadcast session does not match
- **THEN** false returns and no interjection block is added.

#### Scenario: Echo identity
- **WHEN** the optional id is owned locally or belongs to another pane
- **THEN** the local echo is consumed and forgotten, while the foreign interjection renders.

证据：`crates/codegen/pager/src/app/acp_handler/tests/interjection.rs` — 九项测试。

### Requirement: Pager coordination sideband lifecycle replay and source-tool unit checks

The coordination unit module SHALL verify that a live incoming-inquiry sideband is restored after full or cursor reload finalization without taking foreground activity, while an unstructured receipt is a finite notice. Outgoing source audit notices stay hidden in live and replay paths, but actual list_active_sessions and ask_session tool calls retain ordinary running-to-collapsed terminal rows and their full prompt output. A target inquiry start, approval and terminal update mutate one passive row in place, survive foreground finish, preserve manual expansion and expose audit details; replay duplicates and later start/terminal events cannot duplicate or resurrect it. Failed, cancelled and rejected outcomes finish that original row as unsuccessful, and unrelated runtime-health errors remain visible notices. These are in-memory reducer checks and do not run coordination IPC, reload storage, tool execution, terminal rendering, multi-owner routing, or the test binary.

#### Scenario: Reload sideband
- **WHEN** a live start is followed by full or cursor replay finalization and a fresh transient snapshot
- **THEN** one running passive row survives while the foreground session remains Idle.

#### Scenario: Unstructured receipt
- **WHEN** coordination details cannot deserialize to an inquiry identity
- **THEN** a finite Notice is rendered with no running entry.

#### Scenario: Source projection
- **WHEN** outgoing audits and real coordination tools are processed
- **THEN** audits stay hidden while tools use normal terminal rows with full converted output.

#### Scenario: Target lifecycle
- **WHEN** start, approval and completion share one inquiry id
- **THEN** one independent row updates in place, survives foreground finish, preserves expansion and contains audit details.

#### Scenario: Replay and lateness
- **WHEN** start/completion replay and then arrive again live
- **THEN** the finished row remains single and cannot be resurrected.

#### Scenario: Failure and health
- **WHEN** failure, cancellation, rejection or runtime-unavailable notices arrive
- **THEN** inquiry terminals become unsuccessful foldable rows and runtime health remains a visible Notice.

证据：`crates/codegen/pager/src/app/acp_handler/tests/coordination.rs` — 八项测试。

### Requirement: Pager permission argument recap pane and cleanup unit checks

The permission unit module SHALL verify MCP argument projection for UseTool and MCPTool, omission for non-MCP, missing and null shapes, Unicode-scalar line capping, and the 200-line-plus-summary storage cap. Recap checks require manual application to clear recap live status and leave one fresh terminal block, automatic application to preserve that status, and late-auto admission to depend on automatic, replay and idle flags. Permission enqueue checks require the first request from Scrollback to stash that pane and focus Prompt, preserve Prompt/Queue/Tasks ownership, avoid re-stealing on a second request, and restore Scrollback after selection. Replay cleanup cancels queued requests and restores draft/pane state; child-terminal cleanup cancels only matching requests, advances retained focus and preserves shared stashes. These are in-memory helper/dispatch tests and do not execute an MCP tool, render the permission or recap UI, run transport replay, exercise auto-approval, validate every permission option, or run the test binary.

#### Scenario: MCP arguments
- **WHEN** raw input is MCP, another tool shape, missing or null
- **THEN** only MCP nonnull tool_input is pretty-projected.

#### Scenario: Argument bounds
- **WHEN** one line exceeds the scalar limit or the payload exceeds the line limit
- **THEN** the line ends with one ellipsis and stored output is capped with a hidden-count summary.

#### Scenario: Recap application
- **WHEN** manual or automatic recap is applied with live recap status
- **THEN** manual clears status and stays fresh; automatic appends while retaining the status.

#### Scenario: Late automatic recap
- **WHEN** automatic, replay and idle flags vary
- **THEN** only a busy live automatic recap is dropped by the tested predicate.

#### Scenario: Pane ownership
- **WHEN** the first permission arrives from Scrollback, Prompt, Queue or Tasks and a second can follow
- **THEN** only the first Scrollback transition is stashed and moved to Prompt.

#### Scenario: Selection restore
- **WHEN** the stashed Scrollback request resolves by allow-once
- **THEN** the queue empties and Scrollback ownership restores.

#### Scenario: Replay cleanup
- **WHEN** transport interactions are cleared for replay
- **THEN** the request is cancelled and the prior draft and pane restore.

#### Scenario: Child cleanup
- **WHEN** one of two queued requests is reassigned to a terminating child
- **THEN** only that request is cancelled, the retained request resets to Options and stashes remain.

证据：`crates/codegen/pager/src/app/acp_handler/tests/permissions.rs` — 十六项测试。

### Requirement: Pager MCP progress owner modal patch and catalog refresh unit checks

The MCP handler unit module SHALL verify seeded progress updates in place without moving started_at, unseeded creation, N-server and zero-server progress-to-initialized clearing, and session-id routing that updates or clears a background root without touching the active root or requesting redraw. Unknown roots and direct child session ids are rejected. Server-status checks require a loaded owner modal to receive status and tool details while an active non-owner remains untouched; closed foreground/background modals and loading data schedule no effect or redraw, and a missing status does not coerce to Unavailable. The canonical shell payload round-trips Ready. A configuration catalog delta without tools schedules one FetchMcpsList for the owning agent, while a payload without sessionId is rejected. These in-memory tests do not start MCP servers, exercise shell transport, render the modal, cover errored data or pending-fetch deduplication, patch every status/reason, or run the test binary.

#### Scenario: Progress mutation
- **WHEN** progress is seeded or absent and updates arrive
- **THEN** values update in place with the seeded timestamp preserved, or a new progress object is created.

#### Scenario: Lifecycle
- **WHEN** three-server or zero-server progress reaches initialized
- **THEN** the root progress overlay clears.

#### Scenario: Owner routing
- **WHEN** progress or initialized addresses a background root
- **THEN** only that root mutates and false redraw is returned.

#### Scenario: Rejected owner
- **WHEN** initialized addresses an unknown or direct child session
- **THEN** the parent and active root progress remain unchanged.

#### Scenario: Loaded modal
- **WHEN** server status with tools addresses a background owner modal
- **THEN** that owner row becomes Ready with two tools while the active agent remains unchanged.

#### Scenario: Cheap paths
- **WHEN** the owner modal is closed or its data is Loading
- **THEN** no row mutation, effect or redraw is produced.

#### Scenario: Wire status
- **WHEN** status is missing or the canonical shell Ready payload round-trips
- **THEN** malformed input leaves Initializing unchanged and canonical serialization retains ready.

#### Scenario: Catalog refresh
- **WHEN** a config delta omits tools with or without sessionId
- **THEN** the owner receives one FetchMcpsList only when session identity is present.

证据：`crates/codegen/pager/src/app/acp_handler/tests/mcp.rs` — 十八项测试。

### Requirement: Pager settings verb grouping auto gate and soft permission default unit checks

The settings unit module SHALL verify that a present remote group_tool_verbs value and its later omission re-resolve the full local/environment/default chain, while an effective flip clears stale verb-group expansion. Disabling the Auto gate clears every root session in Auto regardless of the active UI mirror and emits one session-scoped Ask transition per Auto session while preserving an AlwaysApprove sibling. Settings payload announcements are ignored even when another field applies. Permission soft-default checks require a user-owned mode to block re-arm, field omission to preserve the latch and display without recomputation, a present field to reach the applier, remote AlwaysApprove to update default and UI without persistence, explicit null to recompute back to Ask, and a disabled Auto gate to clamp remote Auto to Ask. The grouping assertions can be short-circuited by higher-priority host configuration as stated in the tests. These in-memory checks do not exercise settings transport, disk persistence, announcement delivery, UI drawing, agent acknowledgement, concurrent pushes, or the test binary.

#### Scenario: Verb resolution
- **WHEN** remote grouping is enabled and then omitted
- **THEN** the resolved chain is recomputed instead of retaining the old remote value.

#### Scenario: Grouping flip
- **WHEN** an expanded read group exists and the effective setting flips off
- **THEN** the stale group header expansion clears and rows render individually.

#### Scenario: Auto kill switch
- **WHEN** multiple roots are Auto while the active mirror says Ask
- **THEN** all Auto roots leave Auto and receive scoped Ask notifications; AlwaysApprove remains.

#### Scenario: Announcements
- **WHEN** settings/update contains announcements plus a supported field
- **THEN** announcements remain sourced from their dedicated push while the supported field applies.

#### Scenario: User claim and omission
- **WHEN** permission mode is user-owned or the soft-default field is absent
- **THEN** remote re-arm is blocked for the user claim and omitted data preserves soft state.

#### Scenario: Soft application
- **WHEN** a latched soft default receives AlwaysApprove
- **THEN** default and display update without a persistence effect.

#### Scenario: Explicit null
- **WHEN** the latched remote permission field is explicitly null
- **THEN** soft AlwaysApprove recomputes to Ask without persistence.

#### Scenario: Auto clamp
- **WHEN** a latched remote Auto arrives while its gate is disabled
- **THEN** Ask is displayed and AlwaysApprove remains disarmed.

证据：`crates/codegen/pager/src/app/acp_handler/tests/settings.rs` — 十一项测试。

### Requirement: Pager scheduled task fire upsert linkage and owner routing unit checks

The scheduled-task unit module SHALL verify that firing a known task changes only next_fire_at, including clearing it with None, while an unknown task is created from a payload only when a next fire exists. Every fire updates last_subagent_id to its latest detached child. A created update upserts prompt, schedule and next fire without changing created_at or prior child linkage. Fire, create and delete notifications route by root session id even when another agent is active, mutate only the owner and return false for background redraw. These in-memory checks do not exercise scheduler persistence, clock parsing/countdown rendering, replay, direct child session routing, provisional-row clearing, malformed or unknown sessions, missed-task removal delivery, task execution, or the test binary.

#### Scenario: Known fire
- **WHEN** an existing task fires with a new or absent next time
- **THEN** only next_fire_at changes and no duplicate is inserted.

#### Scenario: Unknown fire
- **WHEN** a missing task fires with a next time
- **THEN** a new entry is built from the payload with created_at near now.

#### Scenario: Expired unknown
- **WHEN** a missing task fires without a next time
- **THEN** the handler reports affected but inserts no entry.

#### Scenario: Child linkage
- **WHEN** successive fires carry different detached subagent ids
- **THEN** last_subagent_id tracks the latest value.

#### Scenario: Created upsert
- **WHEN** a create update repeats an existing task id
- **THEN** mutable schedule fields update while created_at and linkage persist.

#### Scenario: Background owner
- **WHEN** fire, create or delete addresses a non-active root
- **THEN** only that root changes and no redraw is requested.

证据：`crates/codegen/pager/src/app/acp_handler/tests/scheduled_tasks.rs` — 九项测试。

### Requirement: Pager session demultiplex race fallback and activity ownership unit checks

The session-routing unit module SHALL verify that assistant chunks route to their owning active or inactive root, or to a registered direct child through its parent, with redraw requested only for the active root; unknown ids are dropped once the active root has an assigned session. During the pre-assignment race, an active root whose session id is None receives an otherwise unmatched chunk, including when multiple roots are unassigned. Plan and AvailableCommands updates mutate the addressed inactive root, command updates increment its generation, and behavior availability refreshes an already-open Settings snapshot. Mapped background Bash stdout updates the inactive owner without renewing foreground activity. Plan activity renews the watchdog only when its prompt id matches the current foreground prompt. Alternating chunks for two roots remain isolated. These in-memory checks do not run ACP transport, nested descendants, viewer adoption, event-id replay/deduplication, malformed payloads, terminal rendering, timeout behavior, or the test binary.

#### Scenario: Root chunk
- **WHEN** a chunk addresses an active or inactive root
- **THEN** it lands only in that root and redraw reflects active ownership.

#### Scenario: Direct child
- **WHEN** a registered child chunk arrives while another root is active
- **THEN** it lands in the parent-owned child view without redraw.

#### Scenario: Unknown
- **WHEN** an unmatched id arrives after roots have assigned sessions
- **THEN** the chunk is dropped without mutation.

#### Scenario: Creation race
- **WHEN** one or several roots have no session id
- **THEN** the currently active unassigned root receives the otherwise unmatched chunk.

#### Scenario: Inactive projections
- **WHEN** plan, commands or behavior availability address a root
- **THEN** todo and commands mutate their owner, and an open Settings snapshot refreshes.

#### Scenario: Background output
- **WHEN** mapped Bash stdout addresses an inactive root
- **THEN** its task buffer updates without moving the foreground prompt activity anchor.

#### Scenario: Plan activity
- **WHEN** plan metadata omits, mismatches or matches the current prompt id
- **THEN** only the matching identity renews foreground liveness.

#### Scenario: Two-root isolation
- **WHEN** chunks for A and B arrive in sequence while B is active
- **THEN** each root retains only its own text.

证据：`crates/codegen/pager/src/app/acp_handler/tests/session_routing.rs` — 十一项测试。

### Requirement: Pager interaction resolution background ownership and plan reopen unit checks

The interaction unit module SHALL verify that InteractionResolved removes the matching root permission, primary-owned child permission, question or plan approval while an unrelated tool-call id leaves state intact; a child resolution with the same call id retains the root request. Permissions for an inactive root queue on that owner without responding, while its AlwaysApprove mode selects allow-once without redraw. A child request does not inherit the parent AlwaysApprove mode and remains in the parent interaction queue; an unknown-session permission is cancelled. Ask-user requests route independently to background roots and sibling direct-child views, with a background child contributing parent needs_input; an unknown session drops the response sender without returning an error payload. Closing a plan viewer preserves approval and its pending response, reopening restores feedback controls, and approving after reopen preserves newly typed prompt text while returning approved. These in-memory checks do not exercise leader relay, ACP wire transport, invalid parameter decoding, nested descendants, replacement of an existing question/approval, rendered UI, timeout behavior, or the test binary.

#### Scenario: Resolve
- **WHEN** a matching permission, child permission, question or plan approval is resolved
- **THEN** only the matching interaction is dismissed and redraw is reported.

#### Scenario: Unrelated resolution
- **WHEN** the tool-call id does not match
- **THEN** the pending interaction remains unchanged.

#### Scenario: Inactive permission
- **WHEN** a request addresses an inactive root in Ask or AlwaysApprove
- **THEN** Ask queues without response; AlwaysApprove selects allow-once without redraw.

#### Scenario: Child permission
- **WHEN** a direct child asks while its parent is AlwaysApprove
- **THEN** the request stays pending in the primary queue and does not inherit auto-approval.

#### Scenario: Unknown permission
- **WHEN** no root owns the session
- **THEN** no queue changes and a Cancelled response is sent.

#### Scenario: Question ownership
- **WHEN** background roots or sibling direct children ask questions
- **THEN** each concrete view retains its own pending question and parent activity reflects child input.

#### Scenario: Unknown question
- **WHEN** no concrete view owns an ask-user request
- **THEN** the sender is dropped, no error response is sent and unrelated UI remains empty.

#### Scenario: Plan viewer close
- **WHEN** the plan viewer closes before a decision
- **THEN** approval state and response sender remain pending.

#### Scenario: Plan reopen
- **WHEN** the viewer reopens and approval occurs after new prompt input
- **THEN** feedback controls return, approved is sent and the newer prompt text survives.

证据：`crates/codegen/pager/src/app/acp_handler/tests/interactions.rs` — 十六项测试。

### Requirement: Pager model catalog authoritative change control and child isolation unit checks

The model unit module SHALL verify that process catalog updates preserve a selected model and reasoning effort while present, use the shell default when a selection disappears or no root is active, and make each inactive root fall back independently rather than copying the active model. A follower ModelChanged applies silently, updates user preference and optional effort, while an invoking root with a local sampling control defers the change until that control drains. An unrelated AgentChanged is not blocked by sampling control. An unknown authoritative model waits until a catalog publication makes it resolvable, and an unknown session is dropped. A direct child receives only its own ModelChanged, refreshes an ordinary catalog without losing selection, and rejects a lower event sequence using its own highwater; a workflow-pinned child retains its workflow catalog and agent names across process reload. These are in-memory state tests and do not run leader broadcast, config RPC, model sampling, provider traffic, selector rendering, replay, same-ModelId endpoint or wire-route changes, native conversation reset, nested descendants, or the test binary.

#### Scenario: Catalog preserve
- **WHEN** a selected model and effort remain in a refreshed catalog
- **THEN** the session selection and user effort survive while the process template follows the shell default.

#### Scenario: Catalog fallback
- **WHEN** a selected model disappears or no root is active
- **THEN** the affected session or template uses the shell default.

#### Scenario: Independent roots
- **WHEN** an inactive root loses its selected model
- **THEN** it uses the shell default rather than the active root selection.

#### Scenario: Follower change
- **WHEN** a known ModelChanged arrives with no local control pending
- **THEN** model, preference and optional effort update without a scrollback switch notice.

#### Scenario: Local control
- **WHEN** ModelChanged races an in-flight sampling control
- **THEN** the update waits and the newest server state applies after that control drains.

#### Scenario: Control domain
- **WHEN** AgentChanged arrives during sampling control
- **THEN** agent identity applies immediately and is not replayed on sampling drain.

#### Scenario: Catalog race
- **WHEN** ModelChanged names an unknown model later published
- **THEN** the old selection remains until publication retries the held authoritative state.

#### Scenario: Session isolation
- **WHEN** ModelChanged targets an unknown root or one direct child
- **THEN** unknown is dropped; child updates without changing the root.

#### Scenario: Child catalog
- **WHEN** an ordinary child receives process catalog refresh and later ModelChanged
- **THEN** available models refresh while its current selection is preserved until the explicit change.

#### Scenario: Child highwater
- **WHEN** child events arrive with sequence 12 then 11
- **THEN** the second is rejected and child cursor/highwater remain at 12.

#### Scenario: Workflow pin
- **WHEN** a workflow child has pinned model and agent metadata
- **THEN** process catalog reload does not replace its catalog, selection or workflow names.

证据：`crates/codegen/pager/src/app/acp_handler/tests/models.rs` — 十八项测试。

### Requirement: Pager plan approval Behavior confirmation and held FIFO unit checks

The plan-mode unit module SHALL verify that a nonblank plan approval uses request content even without a tracked tool, opens or can reopen its preview, dismisses competing modal and block viewers, remains interactive in AlwaysApprove, and routes to its background root without affecting the active root. Missing content returns an error and a later invalid request preserves the current plan. Tool-call titles cannot activate or deactivate Plan; supported CurrentModeUpdate is authoritative, unknown ids are ignored, and an idempotent supported update still requests refresh. ConfirmationRequired retains the source Behavior, projects the target/banner, keeps a deferred Behavior FIFO held, but permits an unrelated deferred Agent identity to apply. Applied or a matching initial identity releases held work under the new Behavior, while an initial different identity does not. Applied clears Plan, leaving Workflow closes its management view, Rejected retains the source without a local toast, and terminal outcomes or replay start clear confirmation. These in-memory checks do not execute plan tools, model turns, ACP transport, preview rendering/scrolling, keyboard confirmation, replacement response cancellation, persistence, nested child routing, or the test binary.

#### Scenario: Approval content
- **WHEN** a nonblank approval arrives with or without tracked tool state
- **THEN** request markdown becomes the retained preview.

#### Scenario: Approval collision
- **WHEN** another modal or block viewer is open
- **THEN** the competing surface closes and plan approval remains interactive.

#### Scenario: Invalid approval
- **WHEN** content is missing before or after a valid approval
- **THEN** an error is returned without creating or replacing the valid plan.

#### Scenario: Mode and owner
- **WHEN** approval arrives under AlwaysApprove or for a background root
- **THEN** it still waits for interaction on the owning root and redraw follows visibility.

#### Scenario: Tool titles
- **WHEN** plan-like ToolCall names or updates arrive
- **THEN** they neither activate nor deactivate Plan.

#### Scenario: Authoritative mode
- **WHEN** Plan, Normal, unknown or repeated supported CurrentModeUpdate arrives
- **THEN** supported identity projects mode and refresh; unknown leaves state unchanged.

#### Scenario: Confirmation
- **WHEN** confirmation_required targets Normal from Plan
- **THEN** Plan remains authoritative, target/banner appear and held Behavior work stays queued.

#### Scenario: Control domains
- **WHEN** a deferred Agent identity exists during Behavior confirmation
- **THEN** Agent identity applies without releasing Behavior-held work.

#### Scenario: FIFO release
- **WHEN** Applied or the matching deferred identity arrives
- **THEN** deferred mode clears and one queued prompt becomes SendPrompt under the new identity.

#### Scenario: Mismatched initial mode
- **WHEN** Normal arrives while Plan is deferred
- **THEN** the FIFO remains held until Plan arrives.

#### Scenario: Terminal projection
- **WHEN** Behavior is applied, rejected, leaves Workflow or replay begins
- **THEN** state follows the authoritative outcome, workflow view and confirmation clear where asserted, and rejection adds no local toast.

证据：`crates/codegen/pager/src/app/acp_handler/tests/plan_mode.rs` — 二十四项测试。

### Requirement: Pager ACP test fixture topology and typed notification builders

The ACP test fixture module SHALL build a base AgentSession as AgentId zero with an optional session id, default model state, /tmp cwd, Ask permission and an unconsumed channel, then wrap it in empty scrollback. Its one-root app activates that root through switch_to_agent; its two-root topology assigns sess-owner to agent zero, sess-active to agent one and activates agent one; its parent-child topology registers one direct child in both metadata and concrete views. Shared builders produce typed ACP/Grow notifications for text, plans, commands, tools, replay/event metadata, permissions, interactions, git, tasks, monitors, models, Behavior and MCP with fixed test defaults, plus state extractors and mutation helpers used by sibling modules. These helpers do not validate arbitrary wire input, perform transport I/O, create nested descendants, or prove behavior beyond the tests that consume them.

#### Scenario: Base session
- **WHEN** a sibling test requests a root fixture
- **THEN** it receives AgentId zero, optional identity, /tmp, Ask mode, default models and empty scrollback.

#### Scenario: Two roots
- **WHEN** a routing test requests owner and active roots
- **THEN** agent zero owns sess-owner while agent one owns sess-active and is selected canonically.

#### Scenario: Direct child
- **WHEN** a child fixture is requested
- **THEN** the child id is inserted in parent metadata and subagent_views.

#### Scenario: Typed builders
- **WHEN** a test needs an ACP or Grow lifecycle message
- **THEN** the helper constructs the canonical typed envelope or explicitly shaped JSON with documented fixed fields.

#### Scenario: Extractors
- **WHEN** a test reads latest events, messages, todo, tool rows or subagent snapshots
- **THEN** the helper projects only its named narrow view of in-memory state.

证据：`crates/codegen/pager/src/app/acp_handler/tests/mod.rs`。

### Requirement: Pager ACP subagent replay disk fixture isolation and snapshots

The shared subagent replay fixture SHALL allocate a temporary Grow home, install it through the test override and clear that override from a Drop guard. It writes an encoded /tmp session directory with canonical summary, forces updates.jsonl to end in a newline, can write a one-event SubagentSpawned timeline with fixed model and transport metadata, and can spawn through grow/session/update with optional prior updates. Snapshot helpers require the child and its lifecycle row to exist and project selected spawn/finish fields; count helpers inspect only tool rows or exact nonblank user prompts. This fixture performs real temporary file writes but does not run session/load transport, test corruption/partial writes, fsync or locking, validate arbitrary timelines, retain the temporary directory, or represent production cleanup.

#### Scenario: Isolation
- **WHEN** a replay disk closure begins and ends
- **THEN** a unique temporary home is installed thread-locally and cleared by Drop.

#### Scenario: Updates
- **WHEN** child update content lacks a trailing newline
- **THEN** the fixture writes one before saving updates.jsonl beside a canonical summary.

#### Scenario: Timeline
- **WHEN** a subagent spawn timeline is requested
- **THEN** one fixed Spawned event is serialized as JSONL with test model transport metadata.

#### Scenario: Spawn
- **WHEN** optional child updates exist before the spawn notification
- **THEN** files are written first and the typed lifecycle then enters the handler.

#### Scenario: Snapshots
- **WHEN** spawn or finish state is projected
- **THEN** the helper requires existing metadata/rows and returns only its fixed field subset.

#### Scenario: Counts
- **WHEN** tool rows or exact prompt text are queried
- **THEN** only matching concrete child scrollback blocks are counted, and blank prompt queries return zero.

证据：`crates/codegen/pager/src/app/acp_handler/tests/mod.rs`。

### Requirement: Pager ACP workflow title and transient context fixture unit checks

The four direct tests in the shared ACP module SHALL verify that workflow command projection retains name, description and workflowSource, duplicate open-workflow refresh requests coalesce into one FetchWorkflowsList for the owning session, and same-name workflow entries with different description/path project differently. Canonical SessionInfoUpdate title metadata sets generated title, while source user also sets display name. A transient context-pressure SessionInfo update with totalTokens refreshes context used without appending scrollback. These tests use in-memory handlers and do not execute workflow discovery, title persistence, context compaction, modal rendering, transport, malformed metadata, stale sequence ordering, or the test binary.

#### Scenario: Workflow projection
- **WHEN** one project workflow command is projected
- **THEN** name, description and source are retained.

#### Scenario: Refresh coalescing
- **WHEN** the same open modal refresh is queued twice
- **THEN** one owner-scoped FetchWorkflowsList remains.

#### Scenario: Metadata change
- **WHEN** same workflow name has different description and path
- **THEN** the projected command collections differ.

#### Scenario: Generated title
- **WHEN** generated then user SessionInfo titles arrive
- **THEN** generated title follows both and display name is set only by the user source in the sequence.

#### Scenario: Context pressure
- **WHEN** transient metadata carries totalTokens 123456
- **THEN** context used becomes 123456 and scrollback length is unchanged.

证据：`crates/codegen/pager/src/app/acp_handler/tests/mod.rs`。

### Requirement: Pager reconnect reload staging, transcript rollback and todo stash unit checks

reconnect fixtures SHALL exercise begin_session_reload/finish_session_reload through an in-memory AppView and SHALL verify full replay replacement, failed reload rollback, cursor-tail merge, superseded-generation fencing, transient-load cleanup and Plan/todo stash semantics. The checks characterize committed scrollback, reconnect cursor, ACP/Grow highwaters, entry-id allocation, loading status and foreground state; they do not establish a real reconnect transport or persisted session/load behavior.

#### Scenario: Full replay success
- **WHEN** A pre-outage transcript is followed by replay chunks and a successful reload finalization
- **THEN** The replayed transcript replaces the stash, reload status/batch state disappears, the cursor follows the replay tail and the root is Idle.

#### Scenario: Reload failure rollback
- **WHEN** Partial replay plus live ACP and Grow traffic advances state inside a reload window that then fails
- **THEN** The pre-outage transcript, cursor and both stream highwaters are restored; staged blocks are discarded and later entry ids do not reuse ids allocated by staging.

#### Scenario: Cursor-resolved tail
- **WHEN** Only a live post-cursor tail arrives and the existing transcript has a running entry
- **THEN** The old transcript is kept, the tail is appended, the old running entry is force-idled, and the next live delta opens one new entry with the ACP highwater advanced.

#### Scenario: Generation fencing
- **WHEN** A reload supersedes an unfinished reload or interrupts a fresh-view load
- **THEN** Partial staging from the old generation is discarded, stale finalization is rejected, the original stash is recoverable, and transient loading feedback/batches do not leak into committed scrollback.

#### Scenario: Todo stash outcomes
- **WHEN** Plan updates occur before, during, or not at all during a reload
- **THEN** Failure restores the pre-outage todo pane; a cursor merge keeps an in-window live plan, while a merge without a plan keeps the stashed plan.

证据：`crates/codegen/pager/src/app/acp_handler/tests/reconnect.rs`。

### Requirement: Pager ACP/Grow event deduplication, replay gating and reconnect cursor unit checks

root event fixtures SHALL send ACP and Grow notifications through the in-memory handlers and SHALL assert per-stream highwaters, applied-only reconnect cursor advancement, replay admission, stale prompt drops and context-token freshness. ACP and Grow highwaters are tested as separate domains; replay is exempt from live highwater seeding, unexpected replay is dropped, and updates without an eventId remain applicable. These are direct reducer checks and do not prove wire ordering, durable replay storage or terminal rendering.

#### Scenario: Root token routing
- **WHEN** A token notification addresses the active root before or after its session_id is assigned
- **THEN** The root context usage is updated in both cases.

#### Scenario: ACP highwater
- **WHEN** Duplicate or lower event ids, a higher id, an id-less update, and replay ids are delivered
- **THEN** Duplicate/lower live events are dropped without affecting highwater, higher events apply and advance it, id-less updates apply without changing it, and replay does not seed it so a reset-low live id can apply after resume.

#### Scenario: Grow highwater isolation
- **WHEN** Grow updates are duplicated, stale, unhandled, or delivered before a delayed lower-id ACP chunk
- **THEN** Handled Grow duplicates/lower ids are dropped, unhandled kinds advance neither cursor nor highwater, and a direct Grow id does not suppress a lower ACP event because the streams have separate highwaters.

#### Scenario: Applied-only cursor
- **WHEN** Plan, background stdout, unexpected replay, duplicate, and stale-prompt updates are delivered
- **THEN** Applied arms advance the reconnect cursor; dropped or unhandled updates leave it unchanged, and replay outside a load window does not append or advance it.

#### Scenario: Replay/live cursor ordering
- **WHEN** A live Grow update is followed by an older replay update in one reload window
- **THEN** The cursor remains at the live event rather than regressing to the older replay id.

#### Scenario: Context freshness
- **WHEN** A fresh high-token event is followed by a stale lower-token event, or by a higher event
- **THEN** A deduped stale event cannot regress context_used, while a genuinely higher event updates it and advances the ACP highwater.

证据：`crates/codegen/pager/src/app/acp_handler/tests/reconnect.rs`。

### Requirement: Pager reconnect replay terminal, subagent view and held-model unit checks

reconnect replay fixtures SHALL verify that a replayed subagent spawn hydrates a child view while leaving an unpaired child row unresolved, that a terminal observed in replay prevents reconnect adoption of the running prompt, and that an unknown Grow ModelChanged is ingested into cursor/highwater state until a later catalog update can resolve it. The assertions characterize pager state only; they do not prove leader routing, model catalog RPC, or durable replay persistence.

#### Scenario: Replayed child spawn without finish
- **WHEN** A reload replay contains SubagentSpawned but no SubagentFinished, followed by a live child delta after swap
- **THEN** The child row remains unfinished, its child view is tracked, and the delivered child delta still renders into that view.

#### Scenario: Terminal in replay
- **WHEN** A running prompt receives a replayed TurnCompleted before reconnect finalization/adoption
- **THEN** The prompt is recorded as terminal in replay, finalization succeeds, adoption is skipped and the root is Idle.

#### Scenario: Held unknown model
- **WHEN** ModelChanged names an unavailable model and a later update names a known catalog model
- **THEN** The unknown value is held while its event cursor and Grow highwater advance; the later known value applies and advances both markers.

证据：`crates/codegen/pager/src/app/acp_handler/tests/reconnect.rs`。

### Requirement: Pager subagent live lifecycle and owner routing unit checks

子代理 ACP 单元测试 SHALL 通过内存 AppView 与公共 handler fixture 验证嵌套子代理登记到根 Agent 的扁平 descendant map，子代理 model/agent/activity 与终态字段回写父任务投影；SubagentPermissionDecision 只在父滚动区生成结构化审计块并保留显示标题、访问摘要、原因和结果标签，spawn 传入的 permission/effective mode 独立初始化 child view；普通与 Grow update 两种方法对同一 spawn/finish 生命周期产生等价 Started/Completed 投影。workflow-owned child 不产生孤儿生命周期行；非活动 owner 仍登记和更新而不请求重绘，未知 session 与畸形 params 不修改状态；context compact 也更新其匹配的非活动 owner。测试不执行 ACP transport、leader、真实 child process 或终端渲染。

#### Scenario: Nested projection
- **WHEN** WHEN root spawns child and child spawns grandchild, then child model/agent/progress/finish events arrive
- **THEN** THEN the root owns flat descendant routing, parent SubagentInfo receives the projected fields, and finish marks the descendant terminal.

#### Scenario: Permission audit projection
- **WHEN** WHEN a registered child emits permission decisions with unavailable, approved, denied, and timed-out outcomes
- **THEN** THEN one structured parent scrollback block is emitted per decision, the compact identity uses the Subagents title rather than the session id, access/reason text is retained, and the outcome labels are preserved.

#### Scenario: Mode and method parity
- **WHEN** WHEN a spawn supplies independent permission and effective modes, or the same lifecycle is delivered through grow/session_notification and grow/session/update
- **THEN** THEN the child view keeps its own mode and both methods produce the same spawn and terminal snapshots.

#### Scenario: Workflow and owner boundaries
- **WHEN** WHEN a workflow child, an inactive root, an unknown session, malformed params, or an inactive root's AutoCompactCompleted update is handled
- **THEN** THEN workflow children avoid orphan rows, known inactive owners mutate without redraw, unknown/malformed input is a no-op, and context usage changes the addressed owner.

#### Scenario: Activity projection
- **WHEN** WHEN a live child chunk and SubagentProgress arrive before SubagentFinished
- **THEN** THEN the mutable child activity label is stamped and later cleared while the immutable Started scrollback row remains intact.

证据：`crates/codegen/pager/src/app/acp_handler/tests/subagents.rs`。

### Requirement: Pager subagent durable replay, transcript restore and lazy-open unit checks

子代理回放测试 SHALL 用 `_meta.isReplay` 与 eventId 构造生命周期事件，并用独立临时 grow home 写入 parent timeline、child summary 与 updates.jsonl；测试 SHALL 覆盖 spawn/finish 回放的终态合并、重复事件幂等、finish-before-spawn 顺序、terminal entity 保留、kill orphan 的 fallback/真实状态，以及重连后 child control identity 不被重置。恢复时 SHALL 从 parent 的 canonical spawn fact 找到 direct child 的 owned disk layout，回放 child updates 和 prompt enrichment，按 eventId 高水位消除 live overlap；resume 期间延迟 transcript load 到首次 fullscreen open，finished child 仍可懒加载完整 transcript，prompt echo 只显示一次，已完成回放不会重复，缺 updates.jsonl 为 no-op。测试证据仅覆盖内存 handler 加临时文件 fixture，不证明真实磁盘迁移、ACP transport、进程存活或跨测试并发行为。

#### Scenario: Replay lifecycle convergence
- **WHEN** WHEN replayed spawn/finish events are duplicated, arrive finish-before-spawn, or a replayed child is killed
- **THEN** THEN the entity remains terminal with one Started and one terminal row, an existing terminal's status/counters are preserved, ordering does not append a reversed Started row, and kill finalization uses cancelled only when no real terminal status exists.

#### Scenario: Control identity preservation
- **WHEN** WHEN a child has an armed dispatch generation and the durable spawn fact is replayed after session reload
- **THEN** THEN the semantic control generation and dispatch token remain identical.

#### Scenario: Owned restore and live deduplication
- **WHEN** WHEN parent timeline and child updates.jsonl contain a nested spawn plus model/agent events, then the same event is delivered from the live buffer
- **THEN** THEN descendant state, prompt/model/agent enrichment and event highwater restore once, and the replay/live overlap adds no second scrollback projection.

#### Scenario: Lazy transcript replay
- **WHEN** WHEN a child spawn occurs during parent loading_replay, or a finished child is opened after resume
- **THEN** THEN child updates are not eagerly loaded, opening fullscreen loads the transcript exactly once, the finished child is idle while deferred, and the completed footer is restored on open.

#### Scenario: Prompt and empty-log boundaries
- **WHEN** WHEN a timeline task prompt is also echoed in child updates, or the child has no updates.jsonl
- **THEN** THEN the task appears exactly once with tool calls retained, while an absent updates file leaves child scrollback empty and marks replay complete without inventing content.

证据：`crates/codegen/pager/src/app/acp_handler/tests/subagents.rs`。

### Requirement: Pager session control projection live replay and reconnect unit checks

The pager control-state unit tests SHALL establish the following facts. Pending control updates remain transient and the newest revision owns the live status; an Applied terminal for Sampling, Agent or Behavior commits one visible terminal notice and clears the live status, while a stale terminal is ignored. Clean replay hydrates control state without a new notice unless it settles the exact local intent preserved across reconnect. A pre-assignment snapshot seeds the epoch before the session id is bound; a receipt-only historical terminal cannot replace a newer live target; duplicate terminals are single per epoch and a retired epoch cannot append a late terminal. A child terminal retains its supplied durable event id and child replay hydrates silently. These are in-memory handler checks only: they do not exercise ACP transport, persistence, actual control RPCs, terminal rendering, nested descendants or provider behavior.

#### Scenario: Live transient and terminal projection
- **WHEN** A fresh session receives a snapshot, two Pending revisions and the latest Applied terminal, followed by an older Applied terminal.
- **THEN** Only the latest desired target remains in live control status; Pending adds no scrollback row, the latest terminal appends one row and clears live status, and the stale terminal is ignored.

#### Scenario: Domain and replay behavior
- **WHEN** Fresh Sampling, Agent and Behavior terminals arrive, then historical terminals are replayed with and without an exact pending local intent.
- **THEN** Each fresh domain appends one notice; ordinary replay only hydrates state, while a replay that resolves the exact local intent clears pending state and remains visible.

#### Scenario: Reconnect epoch ownership
- **WHEN** A control snapshot arrives before session assignment, a receipt-only old target races a newer live target, and terminals repeat across old and new epochs.
- **THEN** The new epoch is accepted after binding, the old receipt cannot obscure the live target, each epoch contributes one immutable row, and a retired epoch cannot append a late row.

#### Scenario: Child durable identity
- **WHEN** A direct child receives a live control terminal with an event id and then a replayed terminal.
- **THEN** The child row keeps the event id and replay does not add another visible row.

证据：`crates/codegen/pager/src/app/acp_handler/tests/session_events.rs`。

### Requirement: Pager auto compaction image and child event unit checks

The pager session-event unit tests SHALL establish the following facts. AutoCompactStarted moves the in-flight prompt into the held prompt and marks compaction activity; AutoCompactFailed preserves the held prompt. ImageDropped emits one newline-joined scrollback notice. Successful ImageCompressed is invisible in the TUI in live and replay state, while the empty-image fallback emits a persistent scrollback warning without a toast. Async completion preserves the foreground prompt, applies the latest todo/context state and emits one notification; live synchronous completion is deferred until turn finish, prefers a confirmed context count over a stale estimate, and falls back to the estimate when unconfirmed. Child completion updates both SubagentInfo and the child context numerator, child start does not reset that numerator, a missing child view returns false while still updating existing SubagentInfo, and unknown or unhandled child events return false. The existing command-feedback requirement already covers duplicate replay terminal rows, so this draft does not claim that overlapping replay assertion as new coverage.

#### Scenario: Compaction start and failure
- **WHEN** Compaction starts with an in-flight prompt and then fails.
- **THEN** The prompt is held and removed from in-flight state at start, and remains held after failure.

#### Scenario: Image notice projection
- **WHEN** Dropped notes, successful compression and re-encode fallback notifications arrive in live or replay state.
- **THEN** Dropped notes are one newline-joined row; successful compression is invisible; fallback is a persistent row with no transient toast.

#### Scenario: Async and deferred completion
- **WHEN** Async completion races newer todos or an in-flight prompt, or live synchronous completion is followed by context refresh and turn finish.
- **THEN** New todos and the foreground prompt survive, one async notice remains, synchronous output waits for turn finish, and the confirmed count wins over an estimate while an unconfirmed completion uses its estimate.

#### Scenario: Child context routing
- **WHEN** A child receives compact start/completion, or the child view is missing or the event is unsupported.
- **THEN** Completion updates child view and SubagentInfo context values; start leaves the existing numerator unchanged; missing or unknown targets return false with only the asserted data update.

#### Scenario: Unsupported root event
- **WHEN** An unrelated session event reaches the root apply_session_event helper.
- **THEN** The helper returns false and does not claim a visible mutation.

证据：`crates/codegen/pager/src/app/acp_handler/tests/session_events.rs`。

### Requirement: Pager retry state error projection unit checks

The pager retry-state unit tests SHALL establish the following facts. Retrying clears the in-flight prompt without adding a warning, retry activity or failure flag. A failure tagged with an old or already finalized prompt id is ignored. Exhausted rate-limited and non-rate-limited states mark the model failure as reported; an empty rate-limit reason uses the neutral user message, while an API 429 wrapper exposes the server detail. Failed 401, BYOK and generic provider errors remain RetryFailed, BYOK rejection clears the in-flight prompt, context_length becomes ContextTooLarge, and a recent CompactionFailed row suppresses a duplicate context prompt. These assertions are in-memory reducer checks and do not execute retries, providers, diagnostics backends or transport.

#### Scenario: Retrying and stale failure
- **WHEN** Retrying is applied, or a failure carries a prompt id belonging to an older/finalized turn.
- **THEN** Recovery clears the prompt without user-facing failure state, and stale/foreign failure changes nothing visible or in session flags.

#### Scenario: Exhaustion and rate-limit text
- **WHEN** Rate-limited or ordinary retry exhaustion is projected, with empty or wrapped server reasons.
- **THEN** The failure flag is set, the neutral rate-limit message is used when reason is empty, and the API wrapper is removed while server detail remains.

#### Scenario: Provider and context failures
- **WHEN** 401, BYOK, generic, context_length and prior CompactionFailed errors are projected.
- **THEN** Provider failures remain RetryFailed, BYOK clears the prompt, context_length emits ContextTooLarge, and an existing compaction failure prevents a duplicate context row.

证据：`crates/codegen/pager/src/app/acp_handler/tests/session_events.rs`。

### Requirement: Pager typed UI notice category and correlation unit checks

The pager typed-notice unit tests SHALL establish the following facts. A command UiNotice without an event id may be delivered twice with the same correlation id and still creates two rows; each row retains Command/Error mapping and has no event id. A lifecycle UiNotice retains Lifecycle/Warning mapping and its recovery details. These tests only exercise in-memory notice projection. The coordination target-row assertion in this file is covered by the existing coordination requirement and is intentionally excluded from this additive draft.

#### Scenario: Command correlation without event identity
- **WHEN** The same command notice is delivered twice without event ids.
- **THEN** The second row is appended, correlation does not act as immutable identity, and the projected notice remains Command/Error with event_id=None.

#### Scenario: Lifecycle recovery details
- **WHEN** A lifecycle warning notice includes recovery details.
- **THEN** The projected row remains Lifecycle/Warning and preserves the recovery text.

#### Scenario: Child command notice identity
- **WHEN** A direct child receives a command warning with a durable event id.
- **THEN** The child row retains the Error tone and supplied event id.

证据：`crates/codegen/pager/src/app/acp_handler/tests/session_events.rs`。

### Requirement: Pager dashboard location picker and git worktree toggle unit checks

Dashboard dispatcher tests SHALL pin location-input expansion, picker-instance scoped completion, foreground-only cwd changes, success/error modal behavior, git ancestry refresh, and the repository gate for worktree mode.

#### Scenario: Location resolution
- **WHEN** absolute, relative, home-prefixed, or blank input is resolved
- **THEN** paths expand against cwd/home and blank input is rejected.

#### Scenario: Picker and cwd boundary
- **WHEN** a picker is reopened or a cwd change is requested from foreground, background, valid, or invalid state
- **THEN** stale completions are ignored, only foreground valid directories update cwd/root and close, and errors retain the picker.

#### Scenario: Worktree gate
- **WHEN** cwd git ancestry and the picker or keyboard toggle vary
- **THEN** worktree dispatch persists only inside a git repository and unavailable toggles explain the refusal.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard worktree session creation and attachment preservation unit checks

Dashboard worktree tests SHALL pin dialog staging versus normal creation, attach and stay-on-dashboard confirmation paths, missing-repository refusal, staged model/effort/plan propagation, image/chip preservation through dispatch, cancellation rewind, deferred clipboard probing, and independent dispatch/peek stashes.

#### Scenario: Dialog staging
- **WHEN** worktree mode is active in or outside a repository and a button or prompt dispatch occurs
- **THEN** repository dispatch opens a label dialog and stashes intent while non-repository creation follows the normal path.

#### Scenario: Confirmed creation
- **WHEN** a worktree dialog is confirmed with attach true or false
- **THEN** CreateWorktreeSession carries the label/model, replays prompt configuration, and either opens detail or stays on dashboard.

#### Scenario: Attachment races
- **WHEN** image prompts are stashed, cancelled, resent, or awaiting a clipboard probe
- **THEN** image bytes/chips survive, dispatch waits for the probe, and dispatch plus peek stashes reissue independently.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard roster source and loading lifecycle unit checks

Dashboard roster tests SHALL pin availability outside leader mode, eager source-specific loading, loading completion on success or failure, storage of local idle sessions, and leader-mode selection between live and local rosters.

#### Scenario: Open and fetch
- **WHEN** dashboard opens with leader mode disabled or enabled
- **THEN** it opens in both cases and fetches respectively local sessions or the live roster, with leader loading visible.

#### Scenario: Completion and source
- **WHEN** roster tasks succeed or fail and leader mode changes
- **THEN** loading clears and dashboard_roster returns the correct stored source.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard slash staging and deferred spawn configuration unit checks

Session-less dashboard command tests SHALL pin next-session model, effort, behavior, and permission staging; shared selector input; plan prompt transformation; rejection of unavailable session-only commands; open-time reseeding; live peek response classification; and propagation of staged configuration with serialized first-prompt admission.

#### Scenario: Command staging
- **WHEN** model, effort, plan, clarify, permission, or bare selector commands run
- **THEN** configuration is staged independently for the next session with typed success/error feedback and transformed plan content.

#### Scenario: Command rejection
- **WHEN** unknown, removed, diagnostic, fork, or compact commands run on the session-less dashboard
- **THEN** no agent spawns, input clears, and the matching error feedback is surfaced.

#### Scenario: Spawn configuration
- **WHEN** dispatch, new-agent, worktree, or SessionCreated paths consume staged values
- **THEN** model and effort reach creation/deferred switches, behavior is optimistic and deferred, permissions apply, and the first prompt remains parked until authoritative mode update.

#### Scenario: Peek activity
- **WHEN** turn and tool activity conflict with stale scrollback
- **THEN** live activity determines Working versus Response.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard new session dispatch focus and attach unit checks

Dashboard dispatch and attach tests SHALL pin one-character minimum input, whitespace rejection, always-new-session semantics independent of selected rows, Enter versus attach behavior, top-level and subagent attachment, lazy child transcript replay, deterministic initial focus, repeat spawning, manual-scroll reset, and unopened-dashboard no-op selection.

#### Scenario: Dispatch routing
- **WHEN** nonempty or empty input is sent with button, top-level, subagent, or no selection
- **THEN** every nonempty dispatch creates a new session, plain Enter stays, attach dispatch opens detail, and whitespace is rejected.

#### Scenario: Attach routing
- **WHEN** top-level or subagent rows attach
- **THEN** top-level detail and overlay state align; child rows open the parent with child focus and lazily replay deferred child history.

#### Scenario: Open and navigation state
- **WHEN** dashboard opens or reopens from agent, welcome, empty, or populated state and arrow selection runs
- **THEN** focus resets to new-session semantics, repeat dispatch remains new-session, manual scroll clears, and absent dashboard selection is inert.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard exit overlay and stop transition unit checks

Dashboard exit and overlay tests SHALL pin return-agent restoration, stale-return fallback, subagent-row restoration, overlay exit to dashboard, idle-session close, only-session refusal, running-turn and compaction cancellation, and pending-stop disarming on overlay movement.

#### Scenario: Exit restoration
- **WHEN** dashboard or overlay exits from agent, welcome, dead-return, or subagent context
- **THEN** it returns only to a live recorded target and restores the relevant child row when available.

#### Scenario: Overlay stop
- **WHEN** the attached session is idle, sole, turn-running, or compacting
- **THEN** it closes and returns, refuses while preserving the sole session, or cancels active work without detaching.

#### Scenario: Pending action scope
- **WHEN** mouse exit or cycle moves away from an armed stop
- **THEN** that stop confirmation disarms while unrelated pending actions remain.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard overlay visible-order cycling unit checks

Dashboard overlay cycle tests SHALL pin wraparound through visible top-level rows, filter and pinned ordering, current-view anchoring, operation after dashboard exit or lazy dashboard creation, and no-op guards for singleton, non-agent, disabled, or hidden-current contexts.

#### Scenario: Visible cycle
- **WHEN** previous or next cycles across multiple visible agents
- **THEN** navigation wraps in rendered order, respects filters and pins, and anchors on the actually viewed agent.

#### Scenario: Lazy attach
- **WHEN** cycle begins from an ordinary agent with no open dashboard
- **THEN** a real multi-agent cycle materializes configured dashboard state and attaches the landed agent.

#### Scenario: No-op guards
- **WHEN** fewer than two visible agents, a non-agent view, disabled dashboard, or hidden current agent is encountered
- **THEN** no arbitrary view switch or dashboard materialization occurs.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard rename promotion and reopen state unit checks

Dashboard state tests SHALL pin top-level rename commit/cancel/Esc paths, empty and subagent refusal, header promotion rendering and input routing, in-memory draft/filter preservation across reopen, toggle-close behavior, and stale pin collection.

#### Scenario: Rename
- **WHEN** a top-level rename is typed, committed, cancelled, escaped, blank, or attempted on a child
- **THEN** only a nonblank top-level commit emits RenameSession and changes display name; other paths clear/refuse without mutation.

#### Scenario: Promotion CTA
- **WHEN** pinned, dismissible, captioned, captionless, or absent header promotion renders
- **THEN** button/caption, hit rectangle, and Ctrl+O routing follow promotion state.

#### Scenario: Reopen state
- **WHEN** dashboard exits, reopens, opens while open, or contains missing pins
- **THEN** draft/filter memory persists, the second open closes, and stale pins are dropped.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard delete shortcuts button and list navigation unit checks

Dashboard interaction tests SHALL pin two-step idle deletion with stable next/previous selection, nonduplicated confirmation feedback, shortcuts modal idempotence, create-with-detail attachment, button focus invariants, bounded up/down navigation, and typed state filtering.

#### Scenario: Idle deletion
- **WHEN** Ctrl+X is confirmed on middle or last idle rows
- **THEN** deletion is requested and completion selects the next row or previous fallback without planting duplicate feedback.

#### Scenario: Shortcut modal
- **WHEN** shortcuts help opens repeatedly or closes
- **THEN** it builds once, preserves query state, and clears on close.

#### Scenario: Button navigation
- **WHEN** create-with-detail or focus/up/down actions target the virtual new-agent button
- **THEN** view attachment and row/section focus move within bounded list rules.

#### Scenario: Filter action
- **WHEN** a known state filter is dispatched
- **THEN** dashboard stores the typed state filter.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard peek recent-line projection unit checks

Dashboard peek extraction tests SHALL pin zero/empty output, placeholder text for tool and background blocks, first-line-only projection, chronological ordering, ANSI stripping, and live activity precedence for response labels.

#### Scenario: Empty and limits
- **WHEN** count is zero or scrollback is empty
- **THEN** no recent lines are returned.

#### Scenario: Projection
- **WHEN** message, tool, or background blocks are projected
- **THEN** only the first meaningful line or exact placeholder is returned in chronological order.

#### Scenario: Sanitization
- **WHEN** projected text contains ANSI control sequences
- **THEN** escape bytes are removed while visible content remains.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard stop cancellation and delete revalidation unit checks

Dashboard stop tests SHALL pin confirmation expiry, direct subagent kill, post-delete foreground return, roster busy refusal, top-level turn cancellation, background-task stop, queued-prompt drop, confirmation-time busy revalidation, and missing-session-history refusal.

#### Scenario: Stop by work kind
- **WHEN** Ctrl+X targets child, running top-level, scheduled background work, or queued-only work
- **THEN** it respectively emits child kill, cancels the turn, deletes scheduled tasks, or drops queued prompts without arming deletion.

#### Scenario: Delete safety
- **WHEN** confirmation expires, the row becomes busy, or lacks a session id
- **THEN** deletion rearms or refuses with feedback and never deletes the invalid target.

#### Scenario: Completion
- **WHEN** a foreground dashboard-owned session deletion completes
- **THEN** the agent is removed and the dashboard becomes active.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard permission peek reply and attachment unit checks

Dashboard permission and peek-reply tests SHALL pin request-id and row validation, selected permission delivery, idle immediate send versus running queue, image block/chip and rewind preservation, subagent reply refusal, and reject-with-followup metadata.

#### Scenario: Permission resolution
- **WHEN** a matching, stale, or missing-row permission option is selected
- **THEN** matching responses reach the oneshot and dequeue; invalid selections close peek and surface feedback without consuming requests.

#### Scenario: Peek reply delivery
- **WHEN** text or image replies target idle or running top-level agents
- **THEN** idle replies send immediately, running replies queue, drafts clear, and image/chip state survives whitespace and rewind.

#### Scenario: Restricted reply
- **WHEN** reply targets a subagent or a rejection carries feedback
- **THEN** child reply is refused and top-level RejectOnce includes followup_message metadata.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager dashboard question peek row visibility and roster attach unit checks

Dashboard question and rendering tests SHALL pin single and multi-question response progression, child-question refusal, peek auto-open and multiline growth, visibility rules for empty local sessions, local-roster reuse, and cross-cwd disk resume.

#### Scenario: Question progression
- **WHEN** top-level Ask questions are answered
- **THEN** single answers submit and clear; multi-question answers advance with draft reset before final submission, while child rows remain parked for fullscreen handling.

#### Scenario: Peek rendering
- **WHEN** selection appears or clears and reply text gains lines
- **THEN** peek opens/closes with selection and its reply rectangle grows for multiline input.

#### Scenario: Row visibility
- **WHEN** local sessions are empty-idle, working, titled, or pinned
- **THEN** only unpinned empty-idle sessions are hidden.

#### Scenario: Roster attach
- **WHEN** roster row matches an existing local agent or only disk history
- **THEN** it reuses and focuses the local top level while clearing child focus, or emits LoadSession with the roster cwd.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/dashboard.rs`。

### Requirement: Pager settings registry inventory kind and default end-to-end checks

设置模态框集成测试 SHALL 双向核对默认注册表与显式 ALL_SETTINGS_EXERCISED 清单；固定 Bool、Enum、DynamicEnum、Int、Group 的键集合与默认值投影，验证初始选择落在 compact_mode、所有顶层 Bool 产生类型化布尔 Action，且同载荷的 SettingValue 变体保持不相等。Group 本身没有标量默认值，子项不作为顶层行。

#### Scenario: Registry inventory closure
- **WHEN** the default registry and the explicit exercised-key matrix are compared in both directions
- **THEN** neither an unexercised registered key nor a stale matrix key is accepted.

#### Scenario: Kind catalog
- **WHEN** registered entries are partitioned by SettingKind
- **THEN** the pinned Bool, Enum, DynamicEnum, Int, and Group key sets match exactly and no String-kind production entry remains.

#### Scenario: Default projection
- **WHEN** current_value_for and default_value_for are evaluated from default UI and pager snapshots
- **THEN** each non-group key resolves to the independently pinned bool, enum, string-sentinel, or integer value.

#### Scenario: Typed value boundary
- **WHEN** top-level Bool rows toggle or equal-looking SettingValue variants are compared
- **THEN** the correct typed setter is returned and Bool, Enum, String, and Int variants remain distinct.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings modal lifecycle boolean keyboard mouse and reset end-to-end checks

设置模态框集成测试 SHALL 固定 Browse 生命周期与基础键鼠语义：F2、Ctrl+,、Cmd+, 关闭，Browse Esc 交由 chrome；Space/Enter 与有效鼠标命中发出对应布尔 setter，contextual_hints 子页可键鼠切换子项；标题、列表外、空 hit-rect 与滚动边界不产生动作。Release 以及会写设置的 Repeat Space/Enter 被丢弃，Repeat 导航保留；d 只为可见可重置设置发出 OpenResetConfirm。

#### Scenario: Close and fallthrough
- **WHEN** close shortcuts or Browse Escape are delivered
- **THEN** explicit close shortcuts return Close while plain Browse Escape remains Unchanged for modal chrome.

#### Scenario: Boolean and group activation
- **WHEN** a bool or contextual-hints group child is activated by its supported keyboard or mouse path
- **THEN** the matching typed setter is emitted and group Escape returns to Browse.

#### Scenario: Mouse boundaries
- **WHEN** clicks or scrolling target a selectable row, header, outside area, missing hit rectangles, or list boundary
- **THEN** selection and toggles occur only for valid setting targets and boundary operations stay unchanged.

#### Scenario: Event repetition policy
- **WHEN** release or repeat key events arrive
- **THEN** release and write-producing repeat activation are dropped while repeat navigation may advance.

#### Scenario: Reset targeting
- **WHEN** d is pressed on visible scalar settings or a category header
- **THEN** scalar rows emit OpenResetConfirm for their own key and headers remain unchanged.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings filter editor navigation cache and rendering end-to-end checks

过滤集成测试 SHALL 验证 / 进入 FilterFocused，文本、字素、光标、删除与 Ctrl 清除命令按单行查询语义处理；多词查询采用 AND，类别标题仅随命中设置出现，选择只落在可见设置。Enter 保留查询并回到 Browse，Browse Backspace 可继续删查询；g/G、PageUp/PageDown、方向键、鼠标、缓存重建与窄视口渲染都以过滤后的索引为准，无匹配时展示查询。

#### Scenario: Filter entry and exit
- **WHEN** slash opens filtering and Enter or Escape leaves it
- **THEN** query input is consumed without action leakage, Enter preserves the active query, and Escape clears it.

#### Scenario: Canonical text editing
- **WHEN** grapheme, cursor, delete, line, Ctrl-kill, or unsafe display input is applied
- **THEN** the single-line query follows canonical edit commands and unsafe characters are consumed without insertion.

#### Scenario: AND matching and selection
- **WHEN** one or more query words narrow the registry
- **THEN** only rows matching every word plus their category headers remain and the selected row stays or clamps to a visible setting.

#### Scenario: Filtered navigation and mouse
- **WHEN** arrows, page keys, g/G, or mouse hits operate under an active filter
- **THEN** they navigate or activate only visible filtered settings and hidden-row coordinates remain inert.

#### Scenario: Cache and render
- **WHEN** filtered indices are repeatedly read, the query mutates, or a small viewport renders
- **THEN** unchanged reads reuse the cache, mutations rebuild it, scroll is clamped, and no-match output includes the query.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings theme enum preview commit and mouse end-to-end checks

主题枚举集成测试 SHALL 经生产入口进入 PickingEnum，按注册表顺序预览 theme、auto_dark_theme、auto_light_theme，Enter 用对应 Set Action 提交当前选择，Esc 用 Preview Action 恢复原值并返回 Browse。合成未知 Enum 仅验证结构回退；Browse 鼠标首次选择主题行，而 PickingEnum 内鼠标点击当前保持无动作。

#### Scenario: Picker entry
- **WHEN** Enter activates an enum row
- **THEN** PickingEnum is seeded from the current/default canonical value.

#### Scenario: Theme preview
- **WHEN** Up or Down changes a preview-capable theme-family choice
- **THEN** the matching PreviewTheme-family action carries the newly focused canonical value.

#### Scenario: Theme commit and revert
- **WHEN** Enter commits or Escape cancels a theme-family picker
- **THEN** Enter emits the matching Set action, while Escape emits a preview restore and both return to Browse.

#### Scenario: Synthetic enum boundary
- **WHEN** an unrecognized synthetic preview enum is navigated and cancelled
- **THEN** mode and index transitions are verified but no production Action mapping is claimed.

#### Scenario: Theme mouse boundary
- **WHEN** a theme row or an open theme picker receives a mouse click
- **THEN** Browse first selects the row without an action and picker clicks remain no-ops.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings permission mode canonical picker and ownership end-to-end checks

permission_mode 集成测试 SHALL 固定其 Agent 分类、Shell owner、无预览枚举目录 auto/ask/always-approve，以及 UiConfig 持久默认优先于会话投影的读取语义。键盘和鼠标进入 picker 时按当前默认定位，导航不发预览，Enter 发 SetDefaultPermissionMode，Esc 仅返回 Browse；缺失或无效持久值回退 ask，并核对 canonical 与权限类型投影。

#### Scenario: Permission metadata
- **WHEN** permission_mode metadata is read
- **THEN** it is a Shell-owned Agent enum without preview and exposes the pinned canonical catalog.

#### Scenario: Persistent default projection
- **WHEN** current_value_for reads explicit, missing, or invalid permission defaults
- **THEN** persistent defaults are shown and absent or unrecognized values fall back to ask.

#### Scenario: Picker navigation
- **WHEN** keyboard or mouse opens and navigates permission_mode
- **THEN** the picker seeds from the persistent default and navigation changes focus without preview actions.

#### Scenario: Commit and cancel
- **WHEN** a permission choice is committed or cancelled
- **THEN** Enter emits SetDefaultPermissionMode with the canonical mode and Escape returns to Browse without an action.

#### Scenario: Canonical permission mapping
- **WHEN** permission-mode canonicals are converted to and from permission kinds
- **THEN** catalog strings round-trip and always-approve projects to its pinned permission kind.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings reset overlay expansion and footer rendering end-to-end checks

设置渲染集成测试 SHALL 验证 reset overlay 仅突出目标行、将其他列表行全部 DIM，并保持提示与 reset/cancel 操作区不变暗；Browse 与 picker 显示 Ask Grow 帮助。Right/l 与 Left/h 以及展开图标切换描述展开，长描述按宽度换行；restart pill 只随已展开的 restart_required 行显示。重置提示包含设置标签、以 Reset 开头并以问号结尾。

#### Scenario: Reset spotlight
- **WHEN** reset confirmation overlays a rendered modal
- **THEN** every non-target list cell is dimmed, the target row is not, and prompt/action rows remain full intensity.

#### Scenario: Reset copy
- **WHEN** reset confirmation content is built for registered settings
- **THEN** the visible prompt, breadcrumb, reset/cancel shortcuts, label, Reset prefix, and question suffix are present.

#### Scenario: Help footer
- **WHEN** Browse or PickingEnum is rendered
- **THEN** the Ask Grow footer and example request are visible.

#### Scenario: Expansion controls
- **WHEN** arrow, vim alias, or triangle controls expand and collapse a row
- **THEN** expanded_keys and rendered description content change together.

#### Scenario: Restart indicator
- **WHEN** a restart-required row is collapsed, edited, or expanded
- **THEN** the restart pill remains hidden while collapsed and appears when expanded.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings dynamic model and integer stepper end-to-end checks

动态模型与整数编辑集成测试 SHALL 验证 max_thoughts_width 以 120 打开 EditingValue，Up/Down 每次步进 5、Left/Right 每次步进 10 并夹在 40..=500，Enter 发 SetMaxThoughtsWidth，文本键和带修饰 Esc 不修改，普通 Esc 取消。default_model 从 snapshot 构造 DynamicEnum，模型行提交解析后的 ModelId，零号无覆盖行发 ClearDefaultModel；鼠标二次点击可打开 picker/editor，默认投影保持空模型与宽度 120。

#### Scenario: Integer entry and commit
- **WHEN** max_thoughts_width is opened, stepped, and committed
- **THEN** its buffer starts at 120, uses the pinned increments and bounds, and emits SetMaxThoughtsWidth.

#### Scenario: Integer rejection and cancel
- **WHEN** text keys, modified Escape, or plain Escape reach the integer editor
- **THEN** text and modified Escape are unchanged, while plain Escape returns to Browse without a setter.

#### Scenario: Dynamic model commit
- **WHEN** default_model picker selects a snapshot model
- **THEN** Enter emits SetDefaultModel with the matching resolved ModelId.

#### Scenario: Dynamic model clear
- **WHEN** row zero is committed with no current override
- **THEN** ClearDefaultModel is emitted.

#### Scenario: Mouse and defaults
- **WHEN** dynamic-model or integer rows are clicked twice or defaults are queried
- **THEN** the corresponding editor opens and the empty-model/120 defaults round-trip.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings default permission and Mermaid enum end-to-end checks

default_selected_permission 与 render_mermaid 集成测试 SHALL 固定各自分类、Shell owner、无预览枚举目录和默认投影；键盘或鼠标按两阶段/指示器路径进入 PickingEnum，导航不发预览，Enter 分别发 SetDefaultSelectedPermission 或 SetRenderMermaid，Esc 返回 Browse 且不发动作。

#### Scenario: Metadata and defaults
- **WHEN** both enum registrations and current values are inspected
- **THEN** their categories, Shell ownership, no-preview flags, canonical choices, and pinned defaults match.

#### Scenario: Keyboard picker
- **WHEN** Enter opens either enum and navigation changes its choice
- **THEN** PickingEnum is seeded correctly and navigation yields Changed without a preview Action.

#### Scenario: Commit and cancel
- **WHEN** a choice is committed or cancelled
- **THEN** the corresponding typed Set action is emitted on Enter and Escape returns without an action.

#### Scenario: Mouse entry
- **WHEN** an unfocused row body, focused row body, or value indicator is clicked
- **THEN** the first body click selects and a second body click or indicator click opens the picker.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings screen hunk CLI and model-family metadata end-to-end checks

设置集成测试 SHALL 固定 screen_mode、hunk_tracker_mode 的无预览枚举交互与 canonical 目录，Enter 分别发 SetScreenMode/SetHunkTrackerMode，并核对 restart_required；show_tips 与 auto_update 通过键鼠发类型化布尔 setter，属于 Shell 且要求重启，并可被搜索；fork_secondary_model 属于 Models、Shell owner、DynamicEnum、无需重启，读取 UiConfig 非基线值且可搜索，动态模型项使用已知模型校验器。

#### Scenario: Screen and hunk pickers
- **WHEN** screen or hunk tracker mode is opened, navigated, and committed
- **THEN** navigation has no preview and Enter emits its typed setter for a pinned canonical choice.

#### Scenario: Mode catalogs
- **WHEN** screen and hunk metadata is inspected
- **THEN** their canonical lists, defaults, and restart policy match the registry contract.

#### Scenario: CLI boolean batch
- **WHEN** show_tips or auto_update is toggled by keyboard or mouse
- **THEN** the matching typed bool setter is emitted and metadata marks both Shell-owned restart settings.

#### Scenario: CLI discovery and defaults
- **WHEN** the CLI batch is queried by current_value_for or search
- **THEN** default values and expected keywords resolve to those settings.

#### Scenario: Model family
- **WHEN** fork_secondary_model metadata, current values, validator, and search are inspected
- **THEN** it is a discoverable non-restart Shell DynamicEnum under Models and preserves configured values.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings vim mode and text-selection enum end-to-end checks

vim_mode 集成测试 SHALL 通过 Space、Enter 与两阶段鼠标切换，读取 pager snapshot 的已启用值并发相反 setter，同时固定其 Appearance 分类与标签区别。keep_text_selection SHALL 作为 Mouse 分类、Shell owner、无预览枚举暴露 flash/hold；picker 导航不预览，Enter 发 SetKeepTextSelection，Esc 取消，键鼠入口一致，实时 hold snapshot 使 picker 定位 hold。

#### Scenario: Vim toggle
- **WHEN** vim_mode is activated by keyboard or mouse from default or enabled snapshot
- **THEN** SetVimMode carries the inverse current value.

#### Scenario: Vim metadata
- **WHEN** vim_mode and simple_mode labels are inspected
- **THEN** vim_mode is categorized as configured and its scrollback wording stays distinct from input-mode wording.

#### Scenario: Text selection metadata
- **WHEN** keep_text_selection metadata is read
- **THEN** it is a Shell-owned Mouse enum without preview and exposes flash/hold semantics plus Shift-drag guidance.

#### Scenario: Text selection picker
- **WHEN** the enum is opened, navigated, committed, or cancelled
- **THEN** navigation has no preview, Enter emits SetKeepTextSelection, and Escape returns without an action.

#### Scenario: Text selection mouse and snapshot
- **WHEN** row body/indicator clicks or a hold cache snapshot initialize the picker
- **THEN** mouse entry follows two-stage/indicator rules and hold selects the hold choice.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings mouse controls and refresh cadence end-to-end checks

鼠标设置集成测试 SHALL 固定 scroll_speed 为 Shell-owned Mouse Int 1..=100、scroll_lines 为 1..=10，步进提交分别发 SetScrollSpeed/SetScrollLines，鼠标可进入编辑；scroll_mode 为无预览 auto/wheel/trackpad Enum 并提交 SetScrollMode。invert_scroll 按键鼠切换且默认 false。display_refresh_auto_cadence 按键鼠切换，属于 Shell-owned Appearance Bool，默认 false、要求重启、minimal 隐藏，并从 UiConfig 当前值投影。

#### Scenario: Scroll integers
- **WHEN** scroll speed or line-count rows are inspected, opened, stepped, and committed
- **THEN** their owner/category/bounds are pinned and typed integer setters carry the edited value.

#### Scenario: Scroll mode
- **WHEN** scroll_mode is inspected or committed
- **THEN** its no-preview canonical catalog is auto/wheel/trackpad and Enter emits SetScrollMode.

#### Scenario: Scroll mouse entry
- **WHEN** focused integer or enum rows receive their supported mouse activation
- **THEN** they enter EditingValue or PickingEnum.

#### Scenario: Invert scroll
- **WHEN** invert_scroll is activated from off or on state
- **THEN** keyboard and mouse emit SetInvertScroll with the inverse value and metadata defaults it off.

#### Scenario: Refresh cadence
- **WHEN** display refresh auto cadence is toggled or read
- **THEN** typed actions reflect the current value and metadata fixes its category, owner, default, restart, minimal visibility, and current-value projection.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager settings thinking suggestions folds and tool grouping end-to-end checks

尾部布尔设置集成测试 SHALL 验证 show_thinking_blocks、prompt_suggestions、respect_manual_folds、group_tool_verbs 经 Space、Enter 与两阶段鼠标发对应 setter，并从进程缓存或 PagerLocalSnapshot 读取当前值后取反。元数据固定 thinking/tool grouping 为 Appearance Shell-owned 默认 true，prompt suggestions 为 Editor Shell-owned 默认 true，manual folds 为 Appearance Pager-owned 默认 false；Appearance 顺序为 thinking 紧邻 folds 之前、tool grouping 紧邻 folds 之后。

#### Scenario: Thinking blocks
- **WHEN** show_thinking_blocks is toggled from cached off or on
- **THEN** all activation paths emit the inverse typed action and metadata pins its default, owner, category, and order.

#### Scenario: Prompt suggestions
- **WHEN** prompt_suggestions is toggled from cached off or on
- **THEN** all activation paths emit the inverse typed action and metadata pins Editor/Shell/default-on.

#### Scenario: Manual folds
- **WHEN** respect_manual_folds is toggled from default or enabled pager snapshot
- **THEN** all activation paths emit the inverse typed action and metadata pins Appearance/Pager/default-off.

#### Scenario: Tool grouping
- **WHEN** group_tool_verbs is toggled from cached on or off
- **THEN** all activation paths emit the inverse typed action and metadata pins Appearance/Shell/default-on and its registry adjacency.

证据：`crates/codegen/pager/tests/settings_e2e.rs`。

### Requirement: Pager prompt core editing, submission, stash and geometry unit checks

The prompt-widget unit suite SHALL verify send eligibility, continuation handling, editing key classification, undo and redo, terminal-gated select-all, draft stash ownership, and composer height calculations. It SHALL distinguish empty or whitespace drafts and trailing-backslash continuations from sendable text; preserve text, cursor and staged image ownership across stash/restore while cleaning an abandoned staged file; map supported newline, deletion, clear, word-kill, undo and redo keys to edits while leaving unsupported keys ignored; enable Super-A only for Ghostty; and compute height from rendered width, chrome, info row, history-browse freeze and maximum bounds. These tests do not prove real terminal key encoding, shell submission, history persistence, image decoding beyond the synthetic fixture, or pixel-accurate layout in a live terminal.

#### Scenario: Submission gate
- **WHEN** a draft is empty, whitespace-only, sendable, or ends in a continuation backslash
- **THEN** try_send and can_send return the corresponding result and continuation replaces the trailing backslash with a newline.

#### Scenario: Editing keys
- **WHEN** character, newline, delete, kill, clear, undo, redo or unsupported key events reach the widget
- **THEN** the buffer and PromptEvent classification follow the tested binding while unrelated control, Escape and Tab events remain ignored.

#### Scenario: Terminal-gated select all
- **WHEN** Super-A is received with the Ghostty gate enabled or disabled
- **THEN** enabled prompts select all chip-safe text for replacement or deletion, while disabled and empty prompts remain unchanged.

#### Scenario: Draft stash ownership
- **WHEN** a prompt with text, cursor and image payload is stashed, restored or abandoned
- **THEN** restore rebinds the image for submission and dropping an unrestored stash deletes its staged temporary file.

#### Scenario: Composer geometry
- **WHEN** content wraps, history browse is active, chrome or info visibility changes, or a maximum height applies
- **THEN** desired height uses the actual render width, freezes browse height, and respects padding, dividers and the cap.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt paste chips, previews and clean-offset unit checks

The prompt-widget unit suite SHALL verify paste normalization, inline-versus-chip thresholds, chip labeling, preview ownership, explicit expansion, identical repaste behavior, send reconstruction and clean-to-raw offset mapping. Bare carriage returns SHALL normalize to line feeds while CRLF and trailing newlines remain intact; large or sufficiently multiline pastes SHALL become atomic paste elements with decimal size labels where applicable; the cursor-on-element match SHALL win over left adjacency and image elements SHALL suppress paste overlays; Enter or an identical repaste on the chip SHALL inline exactly one copy as one undoable edit; and command ranges after a chip SHALL map outside the atomic body. These tests do not prove bracketed-paste transport, clipboard acquisition, rendering in a live terminal, memory behavior for arbitrarily large input, or downstream request delivery.

#### Scenario: Paste classification
- **WHEN** empty, short, multiline, carriage-return or byte-threshold paste text is handled
- **THEN** empty input is ignored, line endings normalize as specified, and content remains inline or becomes one paste chip at the tested boundaries.

#### Scenario: Chip presentation
- **WHEN** a paste chip represents line-triggered, KB-sized or MB-sized content
- **THEN** its label reports the intended line or decimal-size form and preview hints reflect whether Enter would expand.

#### Scenario: Preview arbitration
- **WHEN** the cursor is on, immediately after, or between paste and image chips
- **THEN** on-chip ownership wins, post-paste preview is temporary, and image ownership suppresses the paste preview.

#### Scenario: Expansion and repaste
- **WHEN** Enter, programmatic expansion or an identical normalized repaste targets a paste chip
- **THEN** the chip becomes editable content once, one undo restores the chip, and different or distant content adds another insertion.

#### Scenario: Submission and range mapping
- **WHEN** chipped or mixed pasted text is sent or followed by a slash token
- **THEN** full paste text is reconstructed and clean offsets map to raw ranges outside atomic chip bodies.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt slash registry, completion and paste-normalization unit checks

The prompt-widget unit suite SHALL verify slash-state activation, ACP command and tool synchronization, command and argument completion, paste line-ending and threshold policy, and slash discovery beside image chips. Registry synchronization SHALL update before refresh, remain idempotent, preserve built-ins and enforce advertised-tool gates. Completion SHALL replace the current command or argument range, add a separator only for argument-taking commands, preserve existing separators and adjacent paste elements, and keep optional-argument commands sendable. Slash state SHALL close for ordinary or cleared text, while a slash followed by whitespace and an image chip remains discoverable. These tests do not prove ACP transport, the complete command registry, model-provider validity, file-backed tool discovery, or execution of an accepted slash command.

#### Scenario: Slash activation
- **WHEN** slash, ordinary, empty or cleared text is refreshed
- **THEN** the snapshot opens only for an active slash context with matches.

#### Scenario: Registry synchronization
- **WHEN** ACP commands and advertised tools are synchronized repeatedly
- **THEN** new commands appear immediately, built-ins remain, duplicates do not accumulate and tool-gated commands follow the tool set.

#### Scenario: Completion acceptance
- **WHEN** a command prefix, in-token cursor, existing argument separator, no-argument command or model argument is accepted
- **THEN** only the intended range changes, canonical ids are inserted and neighboring paste chips remain atomic.

#### Scenario: Paste policy
- **WHEN** bare CR, CRLF, mixed line endings and compact or normal line thresholds are handled
- **THEN** normalization and inline-versus-chip classification match the tested boundary rules.

#### Scenario: Image-adjacent slash context
- **WHEN** slash text and optional whitespace precede an image element
- **THEN** slash state stays active and its dropdown retains matches.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt image identity, lifecycle and bounded recovery unit checks

The prompt-widget unit suite SHALL verify image-chip insertion, path-free display, attachment identity, caps, monotonic numbering, restore pairing, undo and redo recovery, preview lookup and submission reconciliation. Each image SHALL own a distinct element id and chronological display number; deleting a chip SHALL remove only its payload while preserving the high-water counter until an explicit prompt reset; restore SHALL pair payloads to chips by display number rather than byte length or buffer order; input and undo stashes SHALL be bounded and evicted staged files cleaned; and drain SHALL return only live images. These tests do not prove decoder correctness, filesystem durability outside temporary fixtures, session serialization, terminal image protocol support beyond emitted escape inspection, content-block transmission, or resource bounds for encoded image bytes.

#### Scenario: Insertion and reset
- **WHEN** images are inserted, selected, replaced or the prompt is explicitly cleared
- **THEN** path-free chips, trailing separators, caps, payload state and reset numbering follow the tested contract.

#### Scenario: Stable identity and numbering
- **WHEN** chips are deleted, reordered, share display numbers, or new images arrive after deletion
- **THEN** element-id reconciliation preserves distinct payloads and display numbers advance monotonically within the draft.

#### Scenario: Restore pairing and bounds
- **WHEN** image state is restored from same-length or out-of-order chips or exceeds configured caps
- **THEN** payloads bind to matching fresh element ids, overflow truncates, and oldest undo entries and their staged files are evicted.

#### Scenario: Undo, redo and drain
- **WHEN** image edits are undone, redone, inlined or submitted with surrounding text
- **THEN** live payload metadata including source paths survives recoverable edits and deleted or undone images do not drain into content blocks.

#### Scenario: Preview ownership
- **WHEN** cursor, insertion state or hover identifies an image chip
- **THEN** the matching record owns the preview, post-insert preview dismisses after typing, and alternating Kitty owners retransmit placement.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt contextual hint signal unit checks

The prompt-widget unit suite SHALL verify gated one-shot contextual hints for recoverable draft wipes and planning-keyword entry. A substantial user clear SHALL fire the undo hint only when the undo gate is enabled and the erased state remains recoverable; programmatic clears, lossy image clears and completion-driven shrinkage SHALL remain silent. Typing across a planning keyword SHALL fire once only when enabled, while slash or bang commands, restore, paste, paste chords and completion-introduced keywords SHALL remain silent. These tests do not prove hint rendering, user preference persistence, keyword quality across languages, telemetry, or downstream tip scheduling.

#### Scenario: Recoverable wipe hint
- **WHEN** a sufficiently large typed draft is cleared or killed through an enabled user key path
- **THEN** the undo signal fires once only if the erased state can be restored.

#### Scenario: Silent non-user or lossy transitions
- **WHEN** set_text, image-dropping clear or completion acceptance shrinks a draft
- **THEN** no undo hint is advertised.

#### Scenario: Planning rising edge
- **WHEN** typed input crosses into a planning keyword
- **THEN** the enabled plan signal fires once and stays quiet while the keyword remains present.

#### Scenario: Command and non-typing suppression
- **WHEN** the keyword arrives through slash or bang input, restore, bracketed paste, paste chord or completion
- **THEN** the plan signal does not fire on that transition or the next edit.

#### Scenario: Independent gates
- **WHEN** contextual hints are disabled or only one gate is enabled
- **THEN** only the enabled detector runs.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt file-search drill-down and file-reference unit checks

The prompt-widget unit suite SHALL verify @-file completion pass-through, directory drill-down, hidden-mode preservation, spaced-path anchors and file-reference acceptance. Right Arrow SHALL behave as normal cursor movement without a valid selection, drill into directory results without creating an atomic file reference, and behave identically to Tab for file results. Drill anchors SHALL keep spaced directory contexts alive only while the anchored prefix remains valid and SHALL clear on Escape, leaving @ mode or reverting below the prefix. Hidden mode SHALL retain its bang prefix. These tests do not prove asynchronous filesystem search, path existence, ignore-rule evaluation, symlink handling, platform-specific path syntax, ranking, or file content attachment.

#### Scenario: Pass-through
- **WHEN** no popup or no valid selected result exists
- **THEN** Right Arrow leaves text intact and follows ordinary cursor behavior.

#### Scenario: Directory drill-down
- **WHEN** a directory is selected in ordinary or directory mode
- **THEN** Right Arrow replaces the @ path without a trailing slash, keeps the context open and creates no file-reference element.

#### Scenario: Spaced and hidden paths
- **WHEN** selected directories contain spaces or hidden mode is active
- **THEN** the drill anchor preserves the full path and bang prefix while the context remains valid.

#### Scenario: Anchor invalidation
- **WHEN** Escape closes search, the cursor leaves @ mode or text reverts below the drilled prefix
- **THEN** stale anchor state cannot reopen the spaced context.

#### Scenario: File acceptance
- **WHEN** a file is selected with Right Arrow or Tab
- **THEN** both produce the same atomic file reference and trailing space and dismiss search.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt shell ghost, token highlighting and chrome rendering unit checks

The prompt-widget unit suite SHALL verify shell ghost-text state, cursor-sensitive rendering and acceptance, wrap-aware slash-token highlighting, inline slash ghost placement, bordered title treatment and panel-background chip repainting. Shell ghost text SHALL render only for a focused end cursor with no competing slash ghost, truncate to available width, support Unicode and wrapped rows, clear on reset or explicit dismissal, and accept either all text or one word. Recognized slash ranges SHALL paint only visible token cells across wraps and scroll. Titles SHALL preserve corners, truncate or skip at narrow widths, and inline panel prompts SHALL remap paste-chip backgrounds. These buffer tests do not prove real terminal glyph widths, color fidelity, accessibility, terminal image display, interactive focus, or screenshot-level visual correctness.

#### Scenario: Ghost lifecycle
- **WHEN** shell ghost text is set, cleared, reset or replaced by a new prompt
- **THEN** visibility and generation invalidation reflect the current draft only.

#### Scenario: Ghost rendering and acceptance
- **WHEN** focus, cursor position, width, wrapping, Unicode or acceptance mode changes
- **THEN** ghost cells appear only at the eligible end cursor and accepted text is appended wholly or by one word.

#### Scenario: Slash paint
- **WHEN** a recognized slash token wraps or is partly scrolled
- **THEN** every visible token cell receives the accent while neighboring body cells remain untouched.

#### Scenario: Inline ghost and title chrome
- **WHEN** slash inline ghost or a session title is drawn on later rows or the top border
- **THEN** placement, styling, truncation and corner preservation follow the buffer assertions.

#### Scenario: Panel chip background
- **WHEN** a paste chip is drawn on an inline panel or canvas background
- **THEN** only the panel style remaps the baked chip background to the panel color.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager prompt completion splice and predicted-prompt suggestion unit checks

The prompt-widget unit suite SHALL verify safe application of completion splices and fills, stale-dropdown rejection, predicted-next-prompt gating and acceptance, and progressive shell-ghost matching. Completion edits SHALL replace only a valid current range, preserve suffix text, place the cursor after inserted content and reject stale or atomic-element-clipping ranges. A predicted prompt SHALL expose only the unmatched suffix while the gate is active, the cursor is at the end and no shell ghost competes; acceptance SHALL insert the remainder and consume the suggestion. Progressive matching SHALL shorten a compatible ghost and clear it on divergence. These tests do not prove suggestion generation, remote prediction transport, ranking, executable or path validity, concurrency of in-flight responses, or the higher-level key-routing choice that invokes these methods.

#### Scenario: Completion splice
- **WHEN** a valid token, mid-line token or whole-line range is applied
- **THEN** the range is replaced, trailing text survives and the cursor ends after the inserted replacement.

#### Scenario: Stale and atomic safety
- **WHEN** a dropdown range is stale or a completion range clips a paste chip
- **THEN** the draft and atomic element remain unchanged while an abutting range remains editable.

#### Scenario: Completion fill
- **WHEN** a common-prefix fill is applied to a valid token range
- **THEN** the prefix replaces the token and the cursor follows it.

#### Scenario: Predicted prompt visibility
- **WHEN** the gate, typed prefix, cursor or competing shell ghost changes
- **THEN** only an eligible unmatched suggestion suffix is exposed.

#### Scenario: Suggestion acceptance and progressive match
- **WHEN** a predicted prompt is accepted or typed text progressively matches a shell ghost
- **THEN** the remainder is inserted and consumed, compatible ghosts shrink, and divergence clears them.

证据：`crates/codegen/pager/src/views/prompt_widget/tests.rs`。

### Requirement: Pager session picker selection search focus and guarded-delete unit checks

The modal-routing unit suite SHALL verify that session deletion requires d followed by y for the armed row, n or another keyboard action cancels it, mouse movement preserves it, and bare y never deletes. It SHALL verify close emits fetch invalidation, Ctrl-W resolves a worktree selection while search owns focus, server-stamped content matches remain pickable without local title matching while unstamped rows remain locally filtered, and non-vim boundary arrows transfer focus to search with hidden selection until a typed query restores the highlight. These tests do not prove backend deletion, fetch cancellation, worktree creation, deep-search transport, rendered row geometry or global vim-setting isolation under parallel test execution.

#### Scenario: Guarded deletion
- **WHEN** d, y, n, unrelated keys, mouse movement or bare y are sent to the session picker
- **THEN** only an armed d-then-y sequence emits DeleteSession for the captured id and cwd.

#### Scenario: Close and worktree actions
- **WHEN** Esc closes the picker or Ctrl-W selects while search is focused
- **THEN** SessionPickerClosed or the indexed worktree-pick action is emitted.

#### Scenario: Stamped versus local search
- **WHEN** the query is or is not stamped by server results
- **THEN** an unrelated title remains selectable only for the stamped result set.

#### Scenario: Search focus boundary
- **WHEN** Up/Down crosses a non-vim list boundary and text is then entered
- **THEN** focus moves to search, hides row selection, and typing restores meaningful selection.

证据：`crates/codegen/pager/src/app/agent_view/modal_routing.rs`。

### Requirement: Pager command palette vim draft and paste-routing unit checks

The modal-routing unit suite SHALL verify that choosing external prompt editing from the minimal command palette preserves the hidden draft; vim-enabled palettes begin in input mode, clear a non-empty query on the first Escape, enter navigation mode on the next Escape, suppress ordinary typing there, and re-enter search with i or slash; non-vim palettes remain type-to-filter after Escape; and bracketed paste edits only the active query at its cursor while leaving the hidden prompt unchanged and is ignored from inactive vim search. These tests do not prove external editor launch, actual terminal bracketed-paste decoding, command execution, palette rendering or process-wide vim-cache isolation under concurrent tests.

#### Scenario: Draft preservation
- **WHEN** external editing is selected from a minimal-mode palette
- **THEN** the edit action is emitted and the existing prompt draft remains unchanged.

#### Scenario: Vim input transitions
- **WHEN** Escape, ordinary letters, i or slash are used with vim mode enabled
- **THEN** query clearing, navigation suppression and search re-entry follow the tested two-stage flow.

#### Scenario: Non-vim filtering
- **WHEN** Escape clears a non-vim query and another letter is typed
- **THEN** the palette remains in search and filters again.

#### Scenario: Palette paste ownership
- **WHEN** paste occurs in an active query or inactive vim navigation
- **THEN** it inserts at the query cursor only in the active case and never reaches the prompt.

证据：`crates/codegen/pager/src/app/agent_view/modal_routing.rs`。

### Requirement: Pager argument-picker profile geometry filtering and catalog-id unit checks

The modal-routing unit suite SHALL verify that search profiles allocate enough rows for small model lists, navigation profiles omit search chrome, behavior is wider while retaining navigation policy, effort, permission and second-phase model commands remain compact, and long descriptions cap and ellipsize. It SHALL also verify that argument pickers stay type-to-filter after arrow input even when global vim mode is enabled and that selecting a displayed model emits the underlying catalog id rather than its friendly label. These tests do not prove live catalog discovery, every command profile, Unicode display-column wrapping, terminal geometry, slash-command execution or global vim-cache isolation under concurrent tests.

#### Scenario: Selector sizing
- **WHEN** small lists use search, navigation or behavior profiles
- **THEN** chrome, visible rows, width and height follow the tested profile constraints.

#### Scenario: Description cap
- **WHEN** rich description text exceeds the line budget
- **THEN** wrapping returns at most two lines and marks remaining content with an ellipsis when required.

#### Scenario: Filtering despite vim
- **WHEN** an argument picker receives arrows and text while vim mode is globally enabled
- **THEN** its query stays active and filters the catalog.

#### Scenario: Catalog identity
- **WHEN** a model row with a friendly label is selected
- **THEN** the slash action carries its provider catalog id.

证据：`crates/codegen/pager/src/app/agent_view/modal_routing.rs`。

### Requirement: Pager focused settings memory and usage-modal input unit checks

The modal-routing unit suite SHALL verify that settings and memory paste inserts normalized text at the focused filter cursor without mutating the hidden prompt, and that the usage modal closes on Escape, switches tabs through keyboard or recorded tab hits, and maps Enter or a rendered-row click to copying the selected session-information row. It SHALL also verify click-outside dismissal after popup geometry is recorded. These tests do not prove settings persistence, memory search, clipboard writes, live usage data, terminal mouse translation, actual rendering or accessibility.

#### Scenario: Focused filter paste
- **WHEN** settings or memory owns a focused filter and paste arrives after cursor movement
- **THEN** normalized text enters that query at the cursor while the prompt remains untouched.

#### Scenario: Usage keyboard controls
- **WHEN** Escape, Tab, BackTab or Enter reaches the usage modal
- **THEN** it closes, switches tabs or emits copy for the selected loaded row.

#### Scenario: Usage mouse controls
- **WHEN** a click hits a recorded row, tab or outside-popup coordinate
- **THEN** it emits copy, switches tab or closes respectively.

证据：`crates/codegen/pager/src/app/agent_view/modal_routing.rs`。

### Requirement: Pager task badge and background-row source unit checks

内嵌单元测试 SHALL 覆盖零值及 k/M 各边界的截断 badge、truncated 加号、后台 command/description/monitor 标签分支、description 换行折叠、类型样式，以及重复构造的 id 和后台/Agent 哈希命名空间样例。这些是直接函数与内存结构检查；不运行 syntect grammar 失败、主题切换、哈希碰撞搜索或测试二进制。

#### Scenario: Badge boundaries
- **WHEN** 输入零、千、万、百万和千万级计数
- **THEN** 源码断言每个格式区间及加号。

#### Scenario: Background labels
- **WHEN** 说明存在、空白、换行、monitor 或仅 command
- **THEN** 源码断言相应单行标签与样式。

#### Scenario: Sample identities
- **WHEN** 同一后台项重复构造或与 Agent 共用字符串
- **THEN** 源码断言本次结果相同或不同。

证据：`crates/codegen/pager/src/views/tasks_pane.rs`。

### Requirement: Pager task pane render, replay and overflow source unit checks

内嵌渲染测试 SHALL 以 ratatui Buffer 检查 stdout badge 与 truncated 加号、空 stdout 不产生空 badge、replay-restored task 不自动打开而新 live task 会打开、搜索栏不被 overlay 覆盖且增加一行、非溢出长 loop 不越过 kill button，以及列表顶端/底端的居中箭头。这些测试不操作真实终端、鼠标、剪贴板、后台进程、ACP replay 或测试二进制。

#### Scenario: Output badge
- **WHEN** 缓存 stdout 行数为 42
- **THEN** buffer 含 `(42)` 或 truncated 的 `(42+)`，空输出不显示 `()`。

#### Scenario: Replay edge
- **WHEN** 先同步 restored running，再加入 live running
- **THEN** 前者不打开，后者触发打开。

#### Scenario: Overlay geometry
- **WHEN** 搜索栏、长 loop 和上下滚动分别渲染
- **THEN** buffer 中输入栏完整、按钮后无泄漏、箭头状态匹配位置。

证据：`crates/codegen/pager/src/views/tasks_pane.rs`。

### Requirement: Pager task grouping, collapse and subagent-row source unit checks

内嵌单元测试 SHALL 覆盖 running/终态过滤、Agent/Task/Monitor/Scheduled 分组顺序、共享 Watchers header 计数、header 插入、toggle 与定向折叠、空组遗忘折叠、Agent 类型字母顺序，以及类型/model/活动 suffix 的搜索与显示差异。这些测试直接访问模块私有状态，不经过应用按键路由、真实 mouse hit-test、异步子 Agent 生命周期或 test binary。

#### Scenario: Sorting and grouping
- **WHEN** 多种任务状态与类型同时同步
- **THEN** 源码断言 items 顺序、header 和默认过滤。

#### Scenario: Collapse lifecycle
- **WHEN** 折叠、重复折叠、清空及重建组
- **THEN** 源码断言返回值、header 选择与重现时展开。

#### Scenario: Subagent presentation
- **WHEN** 类型、model、活动与长说明变化
- **THEN** 源码断言搜索 label 保留完整元数据而显示活动受运行态和宽度控制。

证据：`crates/codegen/pager/src/views/tasks_pane.rs`。

### Requirement: Pager scheduled and workflow row source unit checks

内嵌单元测试 SHALL 覆盖 scheduled 未来/过去/provisional/非法时间/未知 schedule 和多字节 prompt 分支，并覆盖 workflow 默认隐藏终态、显示 active phase、排除 workflow child、只计 running roster row，以及 paused/budget-limited 等 can_stop 与 is_active 的区别。这些测试不推进可控时钟、不验证真实 scheduler fire、workflow control dispatch、重复 workflow 名称或 overlay 是否为所有 stoppable 状态提供 kill 控件。

#### Scenario: Scheduled states
- **WHEN** next fire 为未来、过去、非法或缺失
- **THEN** 源码断言 countdown、due now、interval fallback 或无 suffix。

#### Scenario: Workflow ownership
- **WHEN** active run 与其 workflow child 同时存在
- **THEN** 只计 workflow 一项。

#### Scenario: Workflow control state
- **WHEN** 状态跨 active、paused、budget_limited 和 terminal 集合
- **THEN** 源码断言 TaskEntry.stoppable 复制 can_stop，running 独立复制 is_active。

证据：`crates/codegen/pager/src/views/tasks_pane.rs`。

### Requirement: Pager permission overlay geometry MCP scope and planned arguments unit checks

The permission-view unit suite SHALL exercise squeezed-area safety, collapsed and expanded height policy, MCP scope availability and labels, Unicode-width character wrapping, preservation of span text and styles, planned JSON argument highlighting and row budgets, option visibility, and shared plain/styled labels for bash and MCP decisions. These tests do not prove key or mouse routing, response delivery, permission persistence, actual terminal accessibility, every theme, or caller-maintained layout cache invariants.

#### Scenario: Small terminal geometry
- **WHEN** rendering and height helpers receive tiny widths, heights and bottom-aligned areas
- **THEN** the tested combinations do not panic and expanded height stays within the screen while collapsed arithmetic retains its documented minimum floor.

#### Scenario: MCP arguments
- **WHEN** planned JSON is short, long, exactly at the budget, expanded or area-clipped
- **THEN** wrapping, visible content, Ctrl-F indicator, ellipsis and option reservation match the tested budget boundaries.

#### Scenario: Scope labels
- **WHEN** tool, server, missing-prefix, bash and plain options are projected
- **THEN** the expected pretty names, all-tools wording, scoped words or original option name are returned.

#### Scenario: Styled character wrapping
- **WHEN** ASCII, wide characters, empty text and alternating styles cross row boundaries
- **THEN** the counter, plain wrapper and styled wrapper agree on text and row count while styles survive their splits.

证据：`crates/codegen/pager/src/views/permission_view.rs`。

### Requirement: Pager permission bash display mapping wrapping and manual review checks

The permission-view bash suite SHALL verify raw newline and continuation preservation, newline normalization, parser-derived operator wrapping, protection of simple quoted spans and heredoc bodies, sequential token-boundary mapping, identical quote-aware wrapping for dimmed and undimmed paths, fallback dimming on a mapping miss, selection styling, and panic resistance at tiny widths. Its ignored historic-command harness SHALL, only when explicitly enabled, read the repository fixture, render configured widths and reject delimiter-only breaks or jq “.[] |” splits. Static presence of that harness does not mean it ran or that every historic command and shell grammar is covered.

#### Scenario: Source normalization
- **WHEN** raw commands contain CRLF, trailing newlines or backslash continuations
- **THEN** line endings normalize and useless trailing rows disappear while intentional physical continuation rows remain.

#### Scenario: Quote and heredoc boundaries
- **WHEN** jq filters, quoted operators or heredoc payloads contain spaces and operator text
- **THEN** the tested content is not split at those internal tokens.

#### Scenario: Selection mapping
- **WHEN** tokens span continuations and real operators or a later substring resembles the target
- **THEN** only the next valid shell-boundary token is accepted and dimmed rendering retains the same wrapping as raw rendering.

#### Scenario: Failed mapping and tiny widths
- **WHEN** raw tokens disagree or width is zero through three columns
- **THEN** fallback output keeps an explicit selected/unselected cue and the tested ASCII and Unicode paths do not panic.

#### Scenario: Opt-in historic review
- **WHEN** the ignored harness is run with PERMISSION_UI_RENDER_REVIEW enabled and optional widths
- **THEN** fixture commands are printed and automatic checks fail on delimiter-only breaks or split jq filter markers.

证据：`crates/codegen/pager/src/views/permission_view.rs`。

### Requirement: Pager scrollback dense-gap and group-range source unit checks

本文件内嵌测试 SHALL 通过 ScrollbackState 的布局接口覆盖 collapsed groupable 的零间隙、expanded/non-groupable 边界、total-height 与 virtual-y 求和、主要 block 类型的 groupability，以及 group_range_of 在全 groupable 模式和 collapsed-only 模式下的连续范围。测试源码调用 sibling layout/group 实现；不证明 ratatui 实际绘制、终端选择或 test binary 已执行。

#### Scenario: Dense gaps
- **WHEN** 连续 collapsed groupable 条目
- **THEN** 中间 gap 为零，尾部及 break 边界为一。

#### Scenario: Expanded break
- **WHEN** collapsed-only 范围中出现 Expanded
- **THEN** 该项成为 singleton 并切断两侧 run。

#### Scenario: Block taxonomy
- **WHEN** stub、prompt、agent、thinking、tool 和 durable terminal notice
- **THEN** 源码断言各自 groupable 分类。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager fold anchor, follow preservation and pin source unit checks

内嵌 harness 测试 SHALL 覆盖 page-flip preserve 下增长 fold 退出 follow、增长导致溢出仍保持 prompt anchor、非 follow 不被重启、收缩 fold 的 follow 保留、finished entry 增长、anchor 关闭分支、respect_manual_folds 关闭的旧行为、follow 关闭后 streaming 不移动 viewport、thinking/global pin 清理范围及 fold 前后 pairwise gap。测试使用内存 harness 和默认/即时布局；不执行真实终端输入、异步流或 test binary。

#### Scenario: Grow while following
- **WHEN** running 或 finished entry 展开
- **THEN** 开启手动 fold 尊重时 pin 条目并退出 follow。

#### Scenario: Shrink while following
- **WHEN** expanded 条目收缩
- **THEN** 保持进入操作前的 follow。

#### Scenario: Streaming after read intent
- **WHEN** 展开后继续追加 thinking chunks
- **THEN** follow 未恢复前 scroll offset 保持。

#### Scenario: Global controls
- **WHEN** thinking-only 与 all-entry fold 操作先后执行
- **THEN** 前者只清 thinking pin，后者清全部 pin。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager N-more truncation threshold and hidden navigation source unit checks

内嵌测试 SHALL 覆盖 group_max_visible 为零、恰为 max+1、超过 max+1 以及位于 history 起止位置的 dense groups，并验证导航跳过 height=0 成员但可停在合成 header。测试直接检查 cache height/header_count；不证明标签文案、鼠标命中、滚动性能或大历史测试二进制。

#### Scenario: Disabled
- **WHEN** max_visible 为零且有 20 项
- **THEN** 所有条目保持可见。

#### Scenario: Threshold
- **WHEN** group 长度恰为 max+1
- **THEN** 不截断；超过时首槽成为 header并隐藏前缀。

#### Scenario: Navigation
- **WHEN** 选择向前/向后越过隐藏成员
- **THEN** 落到 header 或可见尾成员。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager verb-run formation, thinking transparency and refold source unit checks

内嵌测试 SHALL 覆盖 read/subagent verb run 的 singleton 与多成员折叠、execute/edit break、隐藏/展开/运行/结束 thinking 的 transparent 或 claimed 行为、thought 不计工具数量、当前 rendered span 在 refold 前保持 group query 权威、pending input 和 hook chrome 使成员脱组并在状态变化后 refold、增量 push 在 N-more 禁用时仍形成 verb fold，以及 run 末项保留 pairwise boundary gap。测试不覆盖所有工具 verb 分类、真实 tracker/hook 生命周期、并发状态更新或最终渲染文案。

#### Scenario: Verb boundaries
- **WHEN** read run 遇 execute/edit 或 pending/hook member
- **THEN** 在边界切分，受保护成员保持可见。

#### Scenario: Thinking membership
- **WHEN** thought 位于工具前中后或正在 streaming
- **THEN** 按显示/运行状态成为 anchor、transparent 行或 claimed 隐藏成员，计数仍只含工具。

#### Scenario: Rendered authority
- **WHEN** hook 在布局后附到隐藏成员
- **THEN** 下一次 prepare 前 group_range 保留屏幕上的旧 span，refold 后才切分。

#### Scenario: Incremental fold
- **WHEN** 已有 cache 后追加 reads 且 N-more 禁用
- **THEN** verb fold 仍立即形成。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager verb-group expansion, rekey and ownership source unit checks

内嵌测试 SHALL 覆盖 singleton/多成员 verb header 展开收起、Left 从保留选择的 header 收起、page-flip preserve pin、expanded slot 路由 member zero、首项/内部成员开合后的 expansion key 迁移、verb range 排除邻接 dense 成员、claimed entry 阻断 N-more、feature flag 关闭回到普通 truncation、search reveal 解隐藏、Subagent 位于 run 首部/内部，以及 thinking expand-all 与邻接 verb fold 的 key 分离。测试直接操作 state/cache；不证明 app action router、鼠标 header 路由或远程配置持久化。

#### Scenario: Member-zero slot
- **WHEN** expanded verb header 上执行 entry fold
- **THEN** 操作首成员且 expansion key 跟随新 anchor，其他成员保持展开。

#### Scenario: Claim ownership
- **WHEN** verb run 邻接普通 dense group
- **THEN** verb claimed 成员不参加 N-more 范围或 expansion key。

#### Scenario: Search reveal
- **WHEN** 目标在 collapsed verb run 内
- **THEN** 登记当前 anchor expansion并使目标可见。

#### Scenario: Subagent member
- **WHEN** subagent-start 位于 run 首或中间
- **THEN** 可作为 anchor/成员一起展开并从成员处收起。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager generic group-header expansion and lifecycle source unit checks

内嵌测试 SHALL 覆盖普通 N-more header 的展开/收起、header 自身内容替换判定、独立可选择 collapse header、thinking 首项、隐藏 selection 修复、collapse_all/clear/remove 的 expanded_groups 清理、non-header 拒绝 toggle，以及 120 项大 group 展开后除独立 header 槽外全部成员可见。测试不覆盖持久化 expansion、EntryId 复用、并发删除或真实大历史性能。

#### Scenario: Expand and collapse
- **WHEN** 选中普通 truncation header
- **THEN** 展开时 header 独立占首槽且所有后续成员可见，收起后恢复隐藏前缀。

#### Scenario: Selection repair
- **WHEN** selection 位于即将隐藏的成员
- **THEN** 收起后移到 group header。

#### Scenario: Lifecycle cleanup
- **WHEN** collapse-all、clear 或删除 anchor entry
- **THEN** 对应 expansion state 清除。

#### Scenario: Large group
- **WHEN** 120 项 dense run 展开
- **THEN** 首槽为 collapse header，其余 119 项均可见。

证据：`crates/codegen/pager/src/scrollback/state/selection.rs`。

### Requirement: Pager extensions action target pending and fold unit checks

The agent-modal unit suite SHALL verify marketplace update and refresh pending policies, plugin update identity, target and resulting-enabled diagnostics for plugin, skill, MCP, hook and marketplace actions, plugin filter reset, group versus leaf expansion, collapsed-hook group targeting, and refusal to resolve loading data or stale MCP tool indices. These tests do not cover every ButtonAction arm, backend execution, diagnostics emission, rendered groups, asynchronous completion or result reconciliation.

#### Scenario: Update and refresh
- **WHEN** plugin or marketplace update and marketplace refresh actions are constructed
- **THEN** stable identifiers are emitted and row-level versus tab-level pending state matches operation scope.

#### Scenario: Resolved target
- **WHEN** loaded plugin, skill, MCP, hook or marketplace selection is inspected
- **THEN** the expected display target and next enabled state are returned.

#### Scenario: Fold and filter
- **WHEN** plugin group/leaf expansion or status filter cycling occurs
- **THEN** group and detail state toggle independently and filter changes reset selection.

#### Scenario: Invalid backing data
- **WHEN** data is Loading or a selected MCP tool index is stale
- **THEN** target resolution returns no target and no enabled state.

证据：`crates/codegen/pager/src/app/agent_view/modals.rs`。

### Requirement: Pager extensions search tab escape and paste ownership unit checks

The agent-modal input suite SHALL verify the staged Escape behavior for active search, retained queries and modal closure; SHALL verify Tab, BackTab and Shift-Tab switch or wrap tabs while preserving an active query; and SHALL verify bracketed paste enters an active extension form after newline normalization without mutating the hidden prompt. These tests do not cover search ranking, mouse tab routing, pending/message paste suppression, Skills slash-only search, vim global-state isolation, multiple form fields or action construction on form submit.

#### Scenario: Empty active search
- **WHEN** Escape is pressed after slash activated search
- **THEN** search deactivates while the modal stays open; a following Escape closes it.

#### Scenario: Retained query
- **WHEN** Escape is pressed with typed search text
- **THEN** search deactivates first, a later Escape clears the retained query, and only the next Escape closes.

#### Scenario: Tab navigation
- **WHEN** Tab, BackTab or shifted Tab is pressed during active search
- **THEN** the selected tab moves or wraps while search and query remain active.

#### Scenario: Form paste
- **WHEN** paste reaches an active MCP modal input while the prompt contains a draft
- **THEN** normalized text enters the field and the hidden prompt is unchanged.

证据：`crates/codegen/pager/src/app/agent_view/modals.rs`。

### Requirement: Pager destructive extension confirmation capture unit checks

The agent-modal confirmation suite SHALL verify that MCP-server removal, hook-source removal, installed-plugin uninstall, marketplace-plugin uninstall and marketplace-source removal produce a confirmation without dispatch; SHALL verify lowercase y dispatches the captured target and restores its original pending row after selection moves; SHALL preserve confirmed=false for installed-plugin uninstall; and SHALL dismiss without dispatch on Escape, n or uppercase Y. These tests do not prove managed-MCP rejection, mouse dismissal, server cascade prompts, actual deletion/uninstallation, failure recovery or authorization.

#### Scenario: Initial destructive action
- **WHEN** each tested destructive button is invoked on a valid target
- **THEN** a typed Confirmation stores the target payload and selected row while pending state is cleared and no action is emitted.

#### Scenario: Confirmed after selection moves
- **WHEN** lowercase y is pressed after another row becomes selected
- **THEN** the original target is dispatched and the pending badge remains on its captured row.

#### Scenario: Plugin server gate
- **WHEN** an installed plugin uninstall is locally confirmed
- **THEN** the emitted PluginsAction retains confirmed=false for possible server-side multi-plugin confirmation.

#### Scenario: Cancellation key
- **WHEN** Escape, lowercase n or uppercase Y reaches a confirmation
- **THEN** the prompt is dismissed and no backend action is emitted.

证据：`crates/codegen/pager/src/app/agent_view/modals.rs`。
### Requirement: Pager terminal marker stop hook stash identity folding
Stop hook batches SHALL be accepted only for the newest eligible turn-terminal marker: stamped batches require a matching prompt id, unstamped batches are tail-only, newer terminal markers stop the walk, and a repeated hook name is refused. Accepted hooks attach to the marker, force collapsed presentation unless pinned, and standalone lifecycle hooks form collapsed tool-like rows.

#### Scenario: Hook attribution
- **WHEN** a stop or stop_failure batch is stamped, unstamped, interleaved or repeated
- **THEN** only the attributable latest terminal marker accepts it and the same event name cannot be attached twice.

#### Scenario: Hook presentation
- **WHEN** accepted hooks are attached or a lifecycle hook is pushed
- **THEN** hook data is retained and the marker defaults to collapsed unless display mode is pinned.

证据：`crates/codegen/pager/src/scrollback/state/mod.rs` — `ScrollbackState::attach_hooks`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `ScrollbackState::push_lifecycle_hooks`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `ScrollbackState::latest_turn_marker_accepting`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `ScrollbackState::attach_stop_hooks_to_marker`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `tests::stop_hooks_attach_only_to_turn_terminal_markers`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `tests::stop_hooks_respect_marker_prompt_id`；`crates/codegen/pager/src/scrollback/state/mod.rs` — `tests::stop_hooks_merge_walks_past_interleaved_tail_blocks`。
