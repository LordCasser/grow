## ADDED Requirements

### Requirement: Process event identifier helpers
generate_event_id SHALL 将 session_id 与进程全局 SeqCst u64 fetch_add 序号连接；ensure_event_counter_at_least 用 fetch_max 只提高下次序号下界，供恢复方种入历史最大值后的起点。

#### Scenario: 元数据补齐
- **WHEN** ensure_event_id_meta 没有非 null eventId
- **THEN** 生成 eventId，保留其他字段，仅在 agentTimestampMs 键不存在时加入当前毫秒时间。

#### Scenario: 已有 ID
- **WHEN** eventId 任意非 null 值存在
- **THEN** 直接返回，不检查格式也不补 timestamp；计数器不自行持久化，重启和溢出边界不能由进程内计数推导全局永久唯一。

证据：`crates/codegen/shell-base/src/util/event_id.rs` — `generate_event_id`；`crates/codegen/shell-base/src/util/event_id.rs` — `ensure_event_id_meta`；`crates/codegen/shell-base/src/util/event_id.rs` — `ensure_event_counter_at_least`。



### Requirement: Shell image shadow complete coverage and durable description
图像投影 SHALL materialize当前branch及surface revision，按图像group先查内存cache，再恢复已完成Sideband，再创建冻结source引用的ImageDescription Sideband并记录attempt。成功响应先结算usage；空trim文本记Failed，非空先complete保存原文及result_ref后才缓存trim描述。辅助显式拒图尝试持久化unsupported并停止后续group；其他失败记录后继续。任何图像未获shadow则整体返回ImageDescriptionUnavailable，不提交部分永久投影，但已完成Sideband/cache仍保留。全部覆盖才聚合工具调用及assistant carrier来源并record_image_projection_and_ack，成功后发送通知。240秒预算在辅助路由建立后起算，各group开头计算remaining，用其与DESCRIBE_TIMEOUT较小值包住provider调用；磁盘恢复、Sideband记账/提交不在该timeout内且remaining未在这些操作后重算，不能称整个函数严格240秒上限。SurfaceChanged最多重建尝试3次，其他错误直接返回。

#### Scenario: One image group unresolved
- **WHEN** 部分group描述成功但仍有图像未翻译
- **THEN** 拒绝整次永久shadow提交，保留已完成Sideband供后续恢复。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `async fn project_conversation_images_for_text_model_once`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(in crate::session::actor) async fn project_conversation_images_for_text_model`。


### Requirement: Shell image description cache identity and envelope rendering
ImageDescribeCache SHALL 使用(source_revision,group_key,有序URL串长度前缀BLAKE3,带neutral-image-transcription-v1域前缀的原source_context BLAKE3)为key，保存description与Timeline result_ref；get克隆，insert同key覆盖，无容量或TTL淘汰，不含辅助模型身份。URL fingerprint仅哈希URL字符串，不下载远端内容。描述prompt请求中性转录并清理source_context；单行envelope删除全部ASCII控制字符，多行保留LF但删除其他ASCII控制，均替换尖括号为‹›，不删除所有Unicode控制符。描述block先trim_end再清理；image_files空列表None，否则按输入顺序从1编号并清理路径，不验证文件存在。持久描述恢复委托spawn_blocking存储adapter，Join失败转IO错误，不在该wrapper设timeout。

#### Scenario: Same URL changed remote content
- **WHEN** revision/group/context和URL字符串均不变但远端图片内容变化
- **THEN** 此内存key不感知远端变化，仍可命中原描述。

源码证据：
- `crates/codegen/shell/src/session/image_describe.rs` — `pub struct ImageDescribeCache`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn key_for_urls`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn scrub_for_envelope`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn scrub_envelope_body`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub async fn recover_completed_description`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn render_image_files_block`。

补充测试源码证据（未执行）：
- `crates/codegen/shell/src/session/image_describe.rs` — `fn describe_prompt_is_source_local_and_task_independent`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn describe_request_and_cache_cover_all_pdf_page_images`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn description_block_format_is_stable`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn image_files_block_numbers_paths_one_indexed`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn image_files_block_none_when_empty`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn render_image_description_block_scrubs_envelope_close_tags`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn render_image_files_block_scrubs_path_envelope_close_tags`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn scrub_for_envelope_replaces_angle_brackets_and_strips_controls`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn scrub_envelope_body_preserves_newlines_in_paragraphs`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn scrub_envelope_body_strips_other_control_chars`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn scrub_envelope_body_replaces_angle_brackets`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn scrub_envelope_body_passes_unicode_through`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn render_image_description_block_preserves_paragraph_structure`。

### Requirement: Shell image asset persistence and description request construction
persist_user_images SHALL 空输入直接空返回，否则通过ContainedDirectory打开/创建assets目录，逐图STANDARD base64解码并以UUID命名写入；扩展名按精确MIME匹配png/jpeg/jpg/gif/webp/bmp，其他回退png，不验证实际图片字节格式或在本函数限制大小。Unix/Windows委托handle-relative write_atomic，其他平台非空输入返回Unsupported；后项失败不回滚先前已写文件。成功按输入顺序返回展示路径。persist_and_prepend_image_files先完成保存再将清理后的image_files块加到原文前，空图片保持原文。build_describe_request仅生成一个User item，先prompt文本后按顺序附所有Image URL，再with_model，不在此调用provider、截断图片数或显式配置temperature/output token上限。

#### Scenario: Later attachment fails decoding
- **WHEN** 首张成功保存而第二张base64非法
- **THEN** 返回错误且首张资产保留，不返回完整路径列表或拼接消息。

源码证据（测试未执行）：
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn persist_user_images`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn mime_to_extension`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn persist_and_prepend_image_files`。
- `crates/codegen/shell/src/session/image_describe.rs` — `pub fn build_describe_request`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn persist_and_prepend_image_files_writes_assets_and_lists_paths`。

补充测试源码证据（未执行）：
- `crates/codegen/shell/src/session/image_describe.rs` — `fn persist_user_images_writes_files_and_returns_paths`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn persist_user_images_ignores_remote_uri_when_inline_bytes_exist`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn persist_user_images_rejects_symlinked_asset_directory`。
- `crates/codegen/shell/src/session/image_describe.rs` — `fn persist_user_images_empty_input_returns_empty`。


### Requirement: Shell durable image description recovery selection
持久图像描述恢复 SHALL 读取已提交parent Timeline并重建校验，再加载parent引用的全部Sideband ledger并调用统一验证；缺目录/文件或空ledger跳过，其他读取/格式错误传播，非仅检查ImageDescription候选。按parent中ImageDescription spawn逆序选择：首Request purpose及prompt精确匹配，首Result后紧接Completed且error=None，其引用attempt的manifest须image-group/version1、source_revision匹配且context/selected均含source，input_refs和evidence_refs均覆盖parent同source event。raw_output trim非空才返回描述及Result单事件引用，否则继续更早候选。此函数没有额外比较辅助模型身份或图片URL哈希，也不直接提交parent projection。 Result.source_event_seqs类型为固定[u64;2]，并经SidebandTimeline校验必须为[0,last_attempt]；因此恢复函数索引[1]不是可变数组长度风险，且Result要求已有attempt、唯一、非空raw/finish，证据范围受成功attempt输入范围约束。

#### Scenario: Result without completed terminal
- **WHEN** 匹配描述有Result但没有紧随Completed终态
- **THEN** 不恢复该结果，继续检查更早候选。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn recover_completed_image_description_from_directory`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_sideband_ledgers_from_directory`。

引用校验证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) fn validate_sideband_ledgers`。
- `crates/codegen/chat-state/src/sideband.rs` — `pub struct SidebandResult`。


### Requirement: Shell parent Sideband projection consistency validation
validate_sideband_ledgers SHALL 校验每份提供ledger存在parent spawn且非空，经SidebandTimeline及parent验证，并拒绝spawn引用外部Timeline。Compaction Summary要求完成Result的字符数匹配，存在唯一同target replacement；仅pending recovery状态允许无replacement。replacement须单个纯Text CompactionMeta User且列出的权限/goal/cwd/interrupt/prompt元数据为空，文本保留format_compact_summary_content原始摘要前缀，允许空后缀或双换行开头附加文本。ImageProjection replacement须精确等于Result.trim后的描述封装，attempt含同source的selected/context身份、同revision及输入/证据event覆盖。Generated title仅检查完成Result引用，User不需Sideband，Fallback须引用Failed或Cancelled且error=Some的终态；该层不比较生成title字符串与Result文本。

#### Scenario: Tampered image replacement
- **WHEN** 图像shadow文字与所引用完成描述重新封装结果不同
- **THEN** 跨账本校验返回InvalidData。

源码证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) fn validate_sideband_ledgers`。


### Requirement: Shell Sideband completion boundary and interrupted recovery
completed_sideband_result SHALL 要求指定seq可转usize且指向Result，下一条恰为Completed/error=None并为ledger最后一条；缺ledger、类型不符、溢出或无证明均InvalidData。recover_interrupted_sidebands对提供的每份ledger重建状态机，已结束跳过，未结束prepare Cancelled/error=process ended并逐份Durable追加，不将已有Result自动升级Completed。加载调用仅recover_sidebands=true执行该修复，普通读取不应终结活跃Sideband；批次后项失败会返回错误但不回滚已追加终态。

#### Scenario: Result persisted before process exit
- **WHEN** 恢复的新writer读取到Result但未有End的Sideband
- **THEN** 追加Cancelled，不把该Result视为已完成可消费结果。

源码证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `fn completed_sideband_result`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn recover_interrupted_sidebands`。


### Requirement: Shell light session load and summary reconciliation
新writer轻量加载 SHALL 先ensure_writer_lease再load_light_data(true)；读取session Summary与Timeline，重建Timeline、验证prompt blobs及Sideband后才处理恢复。recover_sidebands只控制中断Sideband修复，随后标题和模型投影协调仍执行，再提取control/signals/announcement及workflow runs。标题无canonical事件而Summary有任一标题字段则InvalidData；完全相等直接返回，同seq或更新seq但内容冲突拒绝，落后投影调用repair且要求实际推进。模型无selection则保持Summary，有selection且不同则更新current_model_id/reasoning，agent参数None。materialize_timeline基于同一验证路径，拒绝空Timeline并输出完整0..last输入引用、surface/revision及权限/control上下文。

#### Scenario: Conflicting title at same sequence
- **WHEN** Summary标题seq等于canonical seq但标题或source不相同
- **THEN** 拒绝加载，不以普通落后投影修复覆盖冲突。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) async fn load_session_for_write_without_updates`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn load_light_data`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn reconcile_session_title_projection`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn reconcile_model_projection`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn materialize_timeline`。


### Requirement: Shell session writer lease acquisition and stale cleanup eligibility
writer lease SHALL 按session id加NUL加cwd缓存句柄，命中直接成功；否则从session parent打开锁文件并try_lock_exclusive，WouldBlock返回false，其他错误传播，成功持有句柄。ensure将false转active writer错误。锁名为.id.writer.lock，超过255组件长度（Windows UTF16单位，其他字节）改为.writer-BLAKE3.lock。stale cleanup按last_active_at否则updated_at严格早于UTC cutoff判断，精确展示路径匹配skip；busy lease跳过，获锁才委托整实体删除。单项失败计数继续，scan失败传播，返回饱和deleted/errors计数。

#### Scenario: Stale active session
- **WHEN** 过期实体仍被其他writer锁定
- **THEN** 跳过且不计删除错误。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn try_acquire_writer_lease`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn writer_lease_name`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn cleanup_stale_sessions_sync`。


### Requirement: Shell session deletion quarantine and identity boundary
delete_session_by_id_sync SHALL 查找id，缺失或可选cwd不匹配返回None；删除先取得writer lease，再将parent下session id目录no-replace重命名为UUIDv7 deleting隔离名，parent sync失败只warn继续。隔离目录打开或identity检查报错时尽力no-replace恢复原名并返回原错；identity不同要求恢复成功后返回InvalidData，恢复失败单独报错。只有与原OpenedSession相同实体才递归删除内容并删空隔离目录；中途失败不回滚已删除内容。完成目录删除后依序释放lease缓存、删除锁文件（NotFound容忍）、清opened及prefix缓存；后续清理错误可能在实体已删后返回。

#### Scenario: Session leaf replaced before quarantine
- **WHEN** 隔离后的目录与原打开实体identity不同
- **THEN** 恢复原名后拒绝删除；恢复失败报告错误，不继续递归删除替换实体。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn delete_session_by_id_sync`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn delete_opened_session`。


### Requirement: Shell id only session lookup and physical identity
id-only查询 SHALL 仅支持FromRoot，显式目录模式Unsupported；遍历sessions非点前缀cwd目录，cwd目录打开任意错误跳过，目标session或Summary缺失跳过而其他错误传播。物理身份要求session目录名等于Summary id，cwd目录名等于encode_cwd_dirname结果；若编码结果不同于普通URL encoding，还须有受限读取的.cwd字节精确等于Summary cwd。发现第二份有效同id实体返回InvalidData，不任取其一。普通查询将成功目录句柄按id/NUL/cwd缓存，shared_read使用共享读取句柄且不入writer缓存。bound_session_directory命中已绑定缓存直接返回，不重读Summary；未绑定才走open_session。 普通open_session即使目录缓存命中仍重新读取Summary并校验请求id/cwd；只有bound_session_directory命中路径省略重读。首次open_session在Summary校验成功后才缓存目录，缓存的是句柄而非Summary。

#### Scenario: Duplicate valid id
- **WHEN** 两个cwd目录各有身份校验通过的同id会话
- **THEN** 返回duplicate canonical session id错误。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn open_session_by_id_inner`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn validate_physical_session_identity`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn bound_session_directory`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn open_session`。


### Requirement: Shell committed update streaming and tolerant collection
CommittedJsonlLines SHALL 仅返回换行结束记录，未提交尾部返回None且committed_position不前移；超限行以max+1受限读取并排空至换行后返回InvalidData并推进位置，超限未结束尾部None。open_at直接seek指定offset不校验行边界，stream_position返回最后完整消费位置而非reader已读尾部。UpdatesIterator对完整行trim ASCII并跳过空白，UTF8或envelope错误返回Err；collect_updates逐项错误跳过、首项及汇总warning并保留成功记录，缺updates文件返回空。此容错用于显示updates，不将坏Timeline权威记录同样跳过。

#### Scenario: Incomplete update tail
- **WHEN** 读取到尚无换行的追加尾部
- **THEN** 不返回记录，保存的committed_position仍在上一完整记录之后。

源码证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) struct CommittedJsonlLines`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `impl Iterator for CommittedJsonlLines`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `impl Iterator for UpdatesIterator`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn collect_updates`。


### Requirement: Shell session update wire and disk envelopes
SessionUpdate序列化 SHALL 仅输出method/params，ACP方法为session/update，Grow为_grow/session/update；磁盘SessionUpdateEnvelope额外保存当前Unix秒timestamp，时钟早于epoch回退0。反向转换只接受这两个精确method，未知返回InvalidData类JSON错误，并将params解析为对应notification；timestamp不进入SessionUpdate。from_value要求timestamp如存在可解析为u64，缺省0；from_str借用method与RawValue params，不声明timestamp字段，因此忽略其内容类型及其他未知字段，只校验method/params及notification。两种解析路径对无效timestamp不等价，时间戳不提供事件权威顺序。

#### Scenario: Unknown update method
- **WHEN** envelope的method不属于ACP或Grow session update
- **THEN** 返回错误，不尝试根据params猜测类型。

源码证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) struct SessionUpdateEnvelope`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `impl SessionUpdateEnvelope`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `fn invalid_update_envelope`。


### Requirement: Shell update append bookkeeping and torn tail handling
append_update_with_bookkeeping SHALL 先取得writer lease，再序列化带timestamp及换行的update并spawn_blocking追加到已绑定目录，成功后Summary记录activity并messages加1；lease/append错误标NotCommitted，Summary错误标Committed。底层追加加锁、打开读写文件，若非空尾字节不是换行则在本次line前补换行，不截除旧碎片；随后write_all/flush，Durable额外同步文件及目录，unlock错误忽略。NotCommitted是调用层分类，不证明磁盘未变化：write/flush/sync阶段失败可能已写入部分或完整字节；不可将其视为可靠事务回滚。 追加锁名为目标文件名加.lock，目录相对打开后以try_lock_exclusive循环尝试；WouldBlock每10ms睡眠，5秒deadline后TimedOut，其他错误立即传播。5秒仅覆盖锁竞争循环，不覆盖锁文件打开、写入或同步。

#### Scenario: Summary fails after append
- **WHEN** update完整追加成功但后续Summary patch失败
- **THEN** 返回Committed错误，不能重放为尚未追加的update。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_update_with_bookkeeping`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(super) async fn append_update_to_file`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn append_jsonl_line_in_directory_sync`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn lock_append_contained`。


### Requirement: Shell staged session publication and adapter binding
会话发布 SHALL 在已打开parent下创建独立UUIDv7 staging目录，build成功后sync_tree，再publish_child_no_replace到目标名；发布成功后parent.sync失败仅warning仍返回成功及published句柄，避免以未提交错误重复创建。create失败也尽力清理staging名，build/sync_tree失败关闭staging后清理，发布失败同样尝试清理，清理非NotFound错误只warn并保留原错误。build_publish_and_cache在持opened_sessions缓存mutex期间执行事务，已有id/NUL/cwd key则AlreadyExists；成功缓存返回的published句柄，目标名取session_dir最后组件。该包装器不提供跨实体事务或对build闭包外部副作用的回滚。

#### Scenario: Parent sync fails after publish
- **WHEN** no-replace发布已成功但父目录同步失败
- **THEN** 记录warning并返回成功，不将实体作为未发布重试。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn build_and_publish_session_opened`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn build_publish_and_cache`。


### Requirement: Shell versioned JSONL read admission
版本化JSONL读取 SHALL 先严格读取所有换行提交的RawValue记录，再逐项要求version可解析为u64且精确等于传入current_version，之后反序列化目标类型；缺失或非法版本返回InvalidData，版本不匹配调用session_version_mismatch，不进行迁移或跳过坏记录。底层完整记录解析失败使整个读取失败，不返回已读前缀；未换行尾部遵循CommittedJsonlLines规则。版本化directory读取传播NotFound，read_timeline路径包装器将其转为mandatory Timeline缺失的InvalidData，Sideband加载调用方允许缺失实体或ledger并跳过空ledger。普通read_jsonl及read_jsonl_from_directory仅将文件打开的NotFound转换为空集合，其他错误传播；read_jsonl打开父目录失败不享有此转换。版本检查错误中的index+1是RawValue集合序号，不保证对应含空行文件的物理行号。

#### Scenario: Missing version in committed record
- **WHEN** 已提交JSON对象缺失version或version不是u64
- **THEN** 读取返回InvalidData，不返回其前缀或尝试推断版本。

#### Scenario: Missing ledger is caller specific
- **WHEN** 版本化目录读取遇到不存在的ledger
- **THEN** 底层传播NotFound，由Timeline路径包装器转为InvalidData，或由Sideband加载方跳过。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_versioned_jsonl_from_directory`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_jsonl<T:`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_timeline(`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `fn read_committed_jsonl_from_file`。


### Requirement: Shell full session load and deferred rewind records
完整load_session SHALL 打开会话，验证Timeline重建、prompt blobs和Sideband ledgers，然后协调summary标题及模型投影，再读取Timeline控制快照、updates、signals、announcement、Workflow恢复项及rewind_points。updates沿容错收集路径，rewind_points沿严格完整JSONL记录读取路径；缺失rewind文件为空集合，完整坏记录使整个load失败。返回PersistedData包含updates和rewind_points；load_session_without_updates调用load_light_data(info,false)，省略二者。完整加载不调用interrupted Sideband恢复写入；此前summary协调可能已修改磁盘，后续读取错误不会回滚这些修改。append_rewind_point取得writer lease后序列化一条带换行记录，以Buffered durability追加；load_rewind_points独立spawn_blocking严格读取，replace_rewind_points取得lease后序列化全量记录并调用write_atomic替换。追加不等同durable同步，完整加载也不保证纯只读或跨文件快照原子性。

#### Scenario: Rewind corruption after summary repair
- **WHEN** summary协调成功后完整rewind记录解码失败
- **THEN** 完整load返回错误，先前summary修复不会回滚。

#### Scenario: Deferred rewind load
- **WHEN** 调用load_session_without_updates
- **THEN** 走light加载且不读取updates和rewind_points，rewind可通过独立接口加载。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn load_session(`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn validated_timeline`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn load_session_without_updates`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_rewind_point`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn load_rewind_points`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn replace_rewind_points`。


- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `async fn replace_rewind_points_round_trips_file_snapshots`。

### Requirement: Shell rewind intent codec and acknowledged storage
RewindTransaction SHALL 使用拒绝未知字段的version/target_prompt_index/pre_prompt_index/mode结构；validate要求版本1，非FilesOnly模式要求target严格小于pre，FilesOnly豁免此顺序条件。JSONL写入先取得writer lease，再validate及紧凑JSON序列化，超过16KiB返回InvalidInput，否则通过spawn_blocking对rewind-transaction.json调用write_atomic；clear同样取得lease并删除文件，NotFound视为成功。actor读取从绑定session目录有界读取16KiB，文件缺失返回None，空文件/JSON结构错误/版本或索引不合法传播错误，不静默忽略。读取先typed反序列化后validate，因此坏类型或缺字段可先于版本不兼容被报告。写入和清除消息均由persistence actor把storage结果发送oneshot回执，调用方等待并传播发送失败、回执丢失及storage错误，此等待函数没有本地timeout。此契约只覆盖intent存取，不证明后续文件恢复步骤的幂等性。

#### Scenario: Files only index ordering
- **WHEN** FilesOnly事务target大于或等于pre且版本有效
- **THEN** validate不因索引顺序拒绝该事务。

#### Scenario: Malformed persisted intent
- **WHEN** rewind-transaction.json存在但为空或结构非法
- **THEN** 加载返回错误，不当作不存在。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `impl RewindTransaction`。
- `crates/codegen/shell/src/session/persistence.rs` — `PersistenceMsg::WriteRewindTransactionAndAck {`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn write_rewind_transaction`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn clear_rewind_transaction`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `fn load_rewind_transaction`。


- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `async fn rewind_transaction_is_durable_and_clear_is_idempotent`。

- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn pending_rewind_version_mismatch_remains_classifiable_during_recovery`。

### Requirement: Shell pending rewind recovery sequencing and file compensation
待恢复rewind SHALL 在无intent时返回；涉及conversation且当前prompt等于target时加载rewind points后清除intent并返回，不重放文件或调用mark_reverted；当前既非target也非pre则报错。FilesOnly不检查当前prompt分支。重放文件时按rewind points现有顺序遍历prompt_index>=target的快照，每路径首次出现的content作为desired；先读取所有desired路径的当前内容，再按BTreeMap路径顺序应用。内容相同不写；文件写删失败时按逆序补偿此前成功修改的路径，汇总补偿失败，不包括本次失败路径，也不承诺失败写入无部分副作用。文件模式仅保留prompt_index<target的rewind points，ConversationOnly调用merge_rewind_points_from；先持久化next_points，再按需rewind_durably Timeline，然后替换内存points、清除intent、mark_reverted。文件应用成功后的后续持久化错误直接传播，本函数不回滚已写文件。intent重放的全局崩溃一致性不能仅由此步骤顺序推定。 get_rewind_points会将内存集合按prompt_index升序返回，因此文件desired选择最早prompt快照。其ensure_historical_loaded失败只告警并保留lazy_source后返回，get_rewind_points仍返回当前内存集合且无Result错误信号；本恢复调用链不能据返回成功认定完整历史已加载。replace_rewind_points会清除lazy_source，因此后续恢复写入及替换可消耗这个重试机会。

#### Scenario: Already at target
- **WHEN** 涉及conversation且当前prompt已等于target
- **THEN** 加载points并清除intent后返回，不重放文件或标记reverted。

#### Scenario: Later persistence fails
- **WHEN** 文件重放成功，但next_points持久化失败
- **THEN** 传播错误，不在此函数中回滚文件，intent未被清除。

#### Scenario: File operation fails
- **WHEN** 某路径写删失败且此前已有成功修改路径
- **THEN** 逆序尝试补偿此前路径，并保留原错误及补偿失败信息。

源码证据：
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn recover_pending_rewind`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `async fn apply_file_rewind`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `async fn rollback_rewind_files`。

- `crates/codegen/workspace/src/session/file_state.rs` — `async fn ensure_historical_loaded`。
- `crates/codegen/workspace/src/session/file_state.rs` — `pub async fn get_rewind_points`。
- `crates/codegen/workspace/src/session/file_state.rs` — `pub async fn replace_rewind_points`。
- `crates/codegen/workspace/src/session/file_state.rs` — `pub fn merge_rewind_points_from`。


- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn pending_rewind_transaction_rolls_forward_before_session_use`。

### Requirement: Shell resident history repair admission
resident会话历史修复 SHALL 使用session_turn_active而非跨会话共享的tool_context turn标记，先SeqCst读取并在活跃turn时返回TurnActive，再将同一标记随dry_run发送chat-state actor；actor处理命令时再次检查标记，活跃则拒绝，否则执行repair_history并返回结果。shell将无actor回复转换为chat-state actor unavailable错误，将修复错误传播；只有report.changed且非dry_run才告警记录session_id、duplicates_removed、stripped_tool_result_ids和synthetic_results_inserted。dry_run也受活跃turn拒绝约束；shell入口不独立实现Timeline修复算法。

#### Scenario: Dry run while session turn active
- **WHEN** 请求dry_run但本会话turn标记为true
- **THEN** 返回TurnActive，不因只预览而绕过准入。

#### Scenario: Turn starts after fast path
- **WHEN** shell初次检查为false而actor处理时同一标记为true
- **THEN** actor返回TurnActive，不执行修复。

源码证据：
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn handle_repair_history`。
- `crates/codegen/chat-state/src/actor/mod.rs` — `ChatStateCommand::RepairHistory {`。
- `crates/codegen/chat-state/src/handle.rs` — `pub async fn repair_history`。


### Requirement: Shell rewind checkpoint picker projection
rewind checkpoint列表 SHALL 先读取FileStateTracker元数据，再取chat-state snapshot，以0..current_prompt_index生成每个prompt的条目；snapshot不可用时视为current=0并返回空列表，不用文件快照索引补出会话checkpoint。文件元数据缺失时num_file_snapshots为0、created_at为空字符串，has_file_changes仅由数量大于0决定。预览按prompt_record.text提取user query，取trim后首个非空行；空则None，超过60个chars时调用truncate(first_line,57)加三个点。rewind_file_counts独立从元数据映射prompt_index到数量，不以当前会话prompt范围筛选。close_rewind_window仅将actor state.rewindable设false。

#### Scenario: Unavailable chat snapshot
- **WHEN** 存在文件元数据但chat-state snapshot返回None
- **THEN** checkpoint列表为空，不从文件元数据推断会话prompt范围。

#### Scenario: Prompt without file metadata
- **WHEN** 某prompt位于当前范围内但没有对应文件元数据
- **THEN** 仍显示checkpoint，数量0、has_file_changes=false、created_at为空。

源码证据：
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn get_rewind_points`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn rewind_file_counts`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn close_rewind_window`。


- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn run_file_counts_scenario`。

### Requirement: Shell rewind preview and commit compensation
handle_rewind SHALL 在任何Goal状态存在时返回success=false并要求clear；非FilesOnly的target必须小于当前prompt。文件模式按target及以后最早before快照选择目标内容，用全部points中最后一个after快照与当前文件比较，缺失after快照与content=None均视为None；不等时分类deleted_externally/created_externally/modified_externally。force=false返回success=false的预览，即使无冲突也不提交；可能延迟加载内存历史，但不写intent或修改目标文件。force=true仍先读取并计算冲突，然后取消后台compaction、持久化intent、应用文件、持久化next_points，再按需提交Timeline。next_points写失败补偿已改文件；Timeline失败尝试恢复旧points及文件，汇总补偿错误，intent仍保留。Timeline成功后记录pending prompt，除SUPPRESS_UNTIL_SUCCESS外清除compaction suppression，再持久化UI RewindMarker；marker失败返回明确已提交Timeline的错误，不回滚分支。最后替换内存points、清除intent并mark_reverted。成功响应reverted_files包含目标集合中原内容已相同的路径，clean_files为空，conflicts仍保留预览结果；force不保证这些文件在读取后未被外部再次修改。

#### Scenario: Clean preview
- **WHEN** force=false且无冲突
- **THEN** 返回success=false且error=None，不执行提交。

#### Scenario: Timeline committed marker fails
- **WHEN** 分支已持久化但UI RewindMarker写入失败
- **THEN** 返回说明Timeline已提交的错误，保留intent且不回滚分支。

源码证据：
- `crates/codegen/shell/src/session/actor/rewind.rs` — `pub(super) async fn handle_rewind`。
- `crates/codegen/shell/src/session/actor/rewind.rs` — `async fn persist_rewind_points`。

- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn rewind_persistence_failure_rolls_back_files_and_keeps_tracker`。

- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn run_rewind_scenario`。
- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn run_files_only_bound_scenario`。
- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn seed_compacted_timeline`。


### Requirement: Shell JSONL session initialization admission
JSONL init_session SHALL 在已有会话可打开时验证Timeline、prompt blobs与Sideband并返回已有summary，不按本次传入model_id改写已有模型；仅open_session返回NotFound时创建Summary并设置当前configured sandbox profile，以空surface、空prompt blobs和空initial facts调用init_session_with_summary，其他错误直接传播。prepared初始化先确保parent，在staging写初始prompt blobs，再Timeline::from_seed并依序record initial_facts，写Timeline后写summary，交由build_publish_and_cache发布及缓存。ensure_writer_lease在发布成功之后执行，若此步失败则返回错误，但本函数不撤销已发布会话及缓存；不能将初始化任何错误解释为目标不存在。传入prepared summary不在本包装器中按Timeline重新协调标题或模型。

#### Scenario: Existing session model argument
- **WHEN** 已存在有效会话且本次传入不同model_id
- **THEN** 验证已有事实并返回原summary，不以该参数修改模型。

#### Scenario: Lease fails after publication
- **WHEN** prepared会话已发布缓存后writer lease取得失败
- **THEN** 返回错误，已发布目录不由本函数回滚。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn init_session(`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) async fn init_session_with_summary`。


### Requirement: Shell summary patch field merge rules
Summary patch SHALL 仅更新指定字段：activity取existing与now较大值，messages Increment饱和加而Set可降低，format_version取max。cwd bookkeeping仅接受更大generation并清除generation不大于它的pending reminder；相等generation不执行清除。model_id总替换，agent_name=None保留旧值，reasoning_effort外层None保留、Some(None)清除；git commit/branch包括None均覆盖。title仅无既有seq或新seq严格更大时替换title/source/seq，相等冲突静默忽略；返回bool仅表示title是否更新，不表示其他字段是否改变。每次apply均设置updated_at=now，不取max，因此它不保证单调。Unix/Windows锁定summary sidecar后读取新鲜summary、apply并写回，即使title未更新仍写summary；lock_exclusive无本地timeout，unlock错误忽略，其他平台Unsupported。路径包装要求summary与lock的parent相同，空summary返回InvalidData，读取使用summary字节上限。 写回先serialize_summary验证当前format、pretty JSON及MAX_SESSION_SUMMARY_BYTES上限，再以durable=false、replace=true调用目录write_atomic；不能将summary patch成功等同于强制持久化同步。 lineage patch覆盖session_kind、fork_context_source、parent_session_id及fork_parent_prompt_id，parent_prompt_id=None会清除旧值；context_source不等于字面new且forked_at为空时以独立Utc::now设置forked_at，否则保留。apply_to不写subagent_seed，也不重置已有forked_at；因此forked_at不保证与apply_patch传入now一致。

#### Scenario: Equal title sequence with changed content
- **WHEN** patch title seq等于现存seq但title不同
- **THEN** 保留现存title且返回false，仍更新updated_at并由锁包装写回。

#### Scenario: Clear reasoning but preserve agent
- **WHEN** model patch agent_name=None且reasoning_effort=Some(None)
- **THEN** 保留原agent_name并清除reasoning_effort。

源码证据：
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `pub(crate) fn apply_patch(`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `pub(crate) fn apply_patch_locked_in_directory`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `fn read_modify_write`。

- `crates/codegen/shell/src/session/storage/summary_write.rs` — `fn write_summary_atomic`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `async fn concurrent_writes_do_not_lose_updates`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) fn serialize_summary`。


- `crates/codegen/shell/src/session/storage/summary_write.rs` — `async fn summary_patch_rejects_symlinked_summary_and_lock_targets`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `async fn canonical_title_projection_advances_by_event_seq`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `async fn stale_title_projection_is_ignored`。
- `crates/codegen/shell/src/session/storage/summary_write.rs` — `async fn concurrent_title_projection_converges_to_highest_seq`。

- `crates/codegen/shell/src/session/persistence.rs` — `impl SessionLineage`。

### Requirement: Shell contained atomic file publication durability
ContainedDirectory原子文件写入 SHALL 创建带PID和UUIDv7的独占临时文件，先write_all，再按durable选择文件同步，之后发布目标；发布前失败尽力清理临时文件，不保证清理成功。Unix预检查目标以O_NOFOLLOW打开且须为普通文件，临时文件权限0600；replace使用renameat，no-replace使用linkat后unlink临时名，成功link后的unlink错误被忽略，因此成功不保证无临时名残留。Windows临时文件请求读写及DELETE权限、共享读写删除且不跟随symlink，replace关闭文件后目录rename，no-replace使用打开句柄rename helper。两平台durable发布成功后的目录sync失败仅告警仍返回成功，不回滚目标也不返回普通失败诱发重复发布；durable=false跳过文件及目录同步。不能将成功等同于所有目录同步成功，或将平台实现推定为完全相同的目标预检查。

#### Scenario: Directory sync fails after publish
- **WHEN** durable写入已发布目标但目录sync失败
- **THEN** 告警并返回成功，目标保持已发布。

#### Scenario: Unix temporary unlink fails after link
- **WHEN** Unix no-replace linkat成功但临时名unlink失败
- **THEN** 函数不因该unlink失败报错，可能留下临时名。

源码证据：
- `crates/codegen/shell/src/session/storage/mod.rs` — `pub(crate) fn write_atomic(`。


### Requirement: Shell staged fork copy authority boundaries
copy_session_data_sync SHALL 验证源Timeline及引用文件，在目标staging从源当前surface创建新Timeline lineage。可选target先过滤rewind updates并截断updates及surface；fork_filter移除synthetic User并截至最后完整turn边界，同时清空updates，否则移除源绑定投影updates。所有保留User清除prompt_index和permission_evidence，再按配置转换cwd、复制并验证引用prompt blobs、按需strip_reasoning。summary清空title及其来源/seq、pending cwd reminder、git root/remotes，模型用显式new_model否则源模型，session_kind默认fork，parent只使用options，保留源agent/sandbox/reasoning/head/last_active等显式字段。inherit_control时取源完整Timeline最新control而非截断目标时刻，清除goal/applied_control，将Plan/Workflow/Goal行为归Normal，revision饱和加1并记录新控制事实；announcement去重状态独立于inherit_control复制为新事实。最后写新Timeline及转换session ID后的updates，不复制源事件身份、Workflow ledger或rewind points；返回surface/update/blob计数和控制事实是否seeded。 updates的target截断按计数的User chunk run而非promptIndex数值直接比较：首个marker前未标记连续run计一turn，见marker后只计带marker的run；marker变化或非User边界分run，hostTurn User被当作非User边界不计数。首次超过target+1个计数turn时截断，否则保留全部。session ID转换只替换通知顶层session_id，不递归改写payload中的ID。源绑定过滤明确只包含Grow WorkflowUpdated、GoalUpdated、ControlStateUpdate及SubagentSpawned/Progress/Finished。 cwd转换是字面子串replace：覆盖System文本、User的Text部分、Assistant文本及tool arguments原始字符串、ToolResult文本和Reasoning文本；BackendToolCall与其他User内容部分不变，不做路径组件匹配、JSON解析重编码或通用元数据遍历。surface截断调用conversation_truncate_for_prompt(target+1)，其选择无marker的legacy、未标记前缀计数与首marker不一致时markers-only，否则progressive路径；不得把updates的run计数规则直接视为surface截断算法。 surface legacy计数忽略首个非synthetic User作为preamble，此后非synthetic User及starts_prompt_turn的synthetic各计一个索引，其他synthetic不计；progressive在首marker前沿用该规则，marker出现后只用显式prompt_index，忽略后续未标记User。markers-only只找首个显式idx>=阈值的User；三分支未找到截断点都返回完整长度，不验证marker连续或单调。

#### Scenario: Fork inherited control
- **WHEN** 启用inherit_control且源最新行为为Goal
- **THEN** 子control清除goal与applied_control，行为转Normal并记录新事实。

#### Scenario: Filtered fork updates
- **WHEN** fork_filter=true
- **THEN** 过滤surface至完整turn并清空updates，不复制父会话显示历史。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub fn copy_session_data_sync`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn fork_filter_surface`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn copy_referenced_prompt_blobs`。

- `crates/codegen/shell/src/session/storage/mod.rs` — `pub fn updates_truncate_for_prompt`。
- `crates/codegen/shell/src/session/storage/mod.rs` — `impl UserRunTurnTracker`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn transform_session_id_in_update`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn is_source_bound_projection_update`。

- `crates/codegen/sampling-types/src/conversation.rs` — `pub fn transform_conversation_cwd`。
- `crates/codegen/sampling-types/src/conversation.rs` — `pub fn conversation_truncate_for_prompt`。

- `crates/codegen/sampling-types/src/conversation.rs` — `fn conversation_truncate_legacy`。
- `crates/codegen/sampling-types/src/conversation.rs` — `fn count_legacy_turns_until_marker`。
- `crates/codegen/sampling-types/src/conversation.rs` — `fn conversation_truncate_progressive`。
- `crates/codegen/sampling-types/src/conversation.rs` — `fn conversation_truncate_markers_only`。


### Requirement: Shell storage authority handle reuse
JSONL adapter SHALL 通过Arc OnceLock共享首次打开的authority目录句柄，后续authority调用try_clone该句柄，不重新按路径打开；竞争初始化保留OnceLock胜者。FromRoot仅在create_root=true且未缓存authority时ensure_storage_root，再打开root；Explicit使用配置session_dir的parent，缺parent返回InvalidInput，不因create_root自动创建它。session_parent在FromRoot下打开sessions/encoded_cwd，非直接URL编码名称要求.cwd字节匹配请求cwd，创建路径调用ensure_cwd_marker；Explicit直接返回authority。adapter Clone共享authority、opened_sessions、writer_leases和timeline_prefixes，新构造adapter初始化独立缓存。updates_snapshot_len打开绑定会话updates普通文件读取metadata.len，缺失文件传播错误而非返回0。 ensure_cwd_marker先限制cwd原始字节长度不超过MAX_SESSION_SUMMARY_BYTES；Unix/Windows以durable=true、replace=false发布.cwd，仅AlreadyExists时有界读回并逐字节比较，相同成功、不同InvalidData，其余错误直接传播。不trim、canonicalize或覆盖已有冲突标记，其他平台Unsupported。session_directory以session_dir最后组件在parent句柄下打开，Explicit的目录名来自配置路径而非info.id。

#### Scenario: Cached authority reused
- **WHEN** authority已成功缓存后再次请求create_root=true
- **THEN** 克隆缓存句柄，不再次按root路径创建或打开。

#### Scenario: Missing updates length query
- **WHEN** 有效会话的updates文件不存在
- **THEN** updates_snapshot_len返回打开错误，不把它当作零字节。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn authority(`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn session_parent(`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn ensure_storage_root`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub fn updates_snapshot_len`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn ensure_cwd_marker`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn session_directory(`。


### Requirement: Shell session enumeration admission and activity ordering
会话枚举 SHALL 仅支持FromRoot，Explicit返回空集合。打开authority错误传播，sessions子目录NotFound为空；跳过点开头cwd/session名，cwd或session目录打开失败和候选summary读取失败均跳过。候选须通过物理身份和当前format检查、非hidden、匹配可选cwd精确字符串；在筛选后的候选中重复canonical id使整个枚举报InvalidData。通过候选按id/NUL/cwd获取或插入句柄缓存，再从缓存句柄重读summary并校验id/cwd，此阶段错误传播而非跳过；没有再次执行hidden筛选。目录list_names错误传播，不将扫描描述为所有错误均容错。list_sessions_sync按last_active_at缺省updated_at降序、id字符串升序排序，recent先完整扫描排序再truncate(limit)，limit=0也不跳过扫描。

#### Scenario: Duplicate admitted identity
- **WHEN** 两个通过筛选的候选具有相同canonical id
- **THEN** 整个枚举返回InvalidData，不选择其中一个。

#### Scenario: Recent limit zero
- **WHEN** 调用list_sessions_recent(0)
- **THEN** 仍先扫描并可能返回扫描错误，成功后截为空集合。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn scan_opened_sessions`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn list_sessions_sync`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub async fn list_sessions_recent`。


### Requirement: Shell Timeline append derived summary refresh
Timeline追加bookkeeping SHALL 先取得writer lease、绑定目录及prefix state并完成指定durability的事件追加；追加失败直接返回。之后仅Messages、Notification Consumed且input=Some、Subagent/SubagentSeed/SubagentResult更新activity及format版本，Messages额外取items最大cwd switch generation；SessionTitle构造带事件seq的标题投影，其他事件直接成功不写summary。该路径不增加num_messages。追加成功后的summary patch失败只告警并仍返回成功，不能将派生投影失败当作Timeline未提交。事件JSON序列化并加换行后在spawn_blocking进入底层序列感知追加，不能将本包装器本身描述为独立验证全部Timeline内容。 append_session_title_durable先读取已有Timeline事件并重建，prepare source=User的SessionTitle，再调用Durable bookkeeping并返回准备的事件；prepare错误转InvalidInput，读取/重建错误传播。此入口不加载完整validated_timeline的prompt blobs/Sideband验证链，不单独写第二份标题存储，summary失败仍遵循追加成功后告警规则。

#### Scenario: Projection refresh fails
- **WHEN** Timeline事件追加成功但summary patch失败
- **THEN** 告警并返回成功，不因投影失败重试事件。

#### Scenario: Consumed without input
- **WHEN** 追加Notification Consumed且input=None
- **THEN** 不触发本路径summary activity刷新。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_timeline_event_with_bookkeeping`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_timeline_event_with_durability`。


- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) async fn append_session_title_durable`。

### Requirement: Shell Timeline append prefix guard and tail retry
Timeline序列追加 SHALL 检查行大小并取得append锁，读取完整尾部后先截断未提交尾字节，再检查prefix缓存。完整长度及文件stamp都匹配时复用hasher；长度相同而stamp变化时重载prefix并要求hash一致，长度改变直接拒绝；无缓存时重载prefix。末行seq等于输入seq只在不含换行的序列化字节完全一致时允许重试，不按JSON语义比较；否则要求输入seq为末行checked_add(1)，空ledger要求0。新追加write_all/flush后更新prefix长度/hash/stamp，再按Durable同步文件和目录；这些同步错误传播，即使字节和prefix已更新。相同尾行的Durable重试同样重新同步文件及目录。该追加与write_atomic发布后目录sync仅告警的语义不同；不完整尾截断发生在后续prefix/seq拒绝之前，因此错误不代表没有修改文件。 load_timeline_prefix从文件0读取至complete_len，以最多MAX_JSONL_ENTRY_BYTES+1的单记录buffer读取，每条必须通过大小检查、以换行结束、反序列化TimelineEvent并由Timeline.accept接纳；不跳过空行或空白行。BLAKE3包含每条原始字节及换行，不是规范化JSON哈希；consumed不等于complete_len返回UnexpectedEof。该fold保留Timeline事件，因此有界单行buffer不等于总内存恒定；返回hash和结束时metadata stamp，不单独验证prompt blobs或独立Sideband文件。 共享read_timeline_tail以最后换行确定complete_len，空文件或完全无换行返回无末行；使用8KiB固定buffer向前寻找换行，扫描距离无总字节或时间上限。最后完整行在分配前要求内容长度加换行不超过MAX_JSONL_ENTRY_BYTES，再按usize转换及read_exact读取；返回末行不含换行，空末行仍Some(empty)并由调用方解析拒绝。通用行大小检查仅拒绝len>上限，允许正好上限，换行计入追加行预算。 LedgerFileStamp包含len及metadata.modified().ok()，Unix另含device/inode/ctime秒和纳秒，Windows另含creation_time/last_write_time；modified读取失败记None。缓存命中依据这些元数据相等，不在命中路径重算内容hash，不能把注释中外部写必改stamp视为任意文件系统上的内容完整性证明。 adapter的prefix state按id/NUL/cwd存入共享BTreeMap，每项为Arc Mutex Option<LedgerPrefix>，首次默认None，缓存锁poison传播错误。测试辅助append_timeline_line_sync每次新建None prefix，不保留跨调用writer epoch缓存，因此只经该helper的测试不能证明stamp命中或历史变化检测分支。

#### Scenario: Same sequence different bytes
- **WHEN** 输入seq等于末行但序列化字节不同
- **THEN** 返回InvalidData，不将其当作幂等重试。

#### Scenario: Durable sync fails after append
- **WHEN** 新记录已write/flush且prefix更新后同步失败
- **THEN** 返回错误，记录可能已存在；相同尾行重试可以再次同步。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn append_timeline_line_in_directory_sync`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn load_timeline_prefix`。


- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_timeline_tail`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn find_previous_newline`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn validate_jsonl_line_size`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `impl LedgerFileStamp`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn timeline_prefix_state`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn append_timeline_line_sync(`。

### Requirement: Shell Sideband tail append admission
Sideband追加 SHALL 在公开durable adapter入口先取得writer lease并验证sideband_id，再序列化带换行事件，于spawn_blocking创建或打开sidebands/id目录。底层检查行大小、取得append锁、读完整尾记录并截断未提交尾部；相同seq仅允许不含换行的原始JSON字节完全匹配，Durable重试重新同步文件和目录。不同seq须等于末行checked_add(1)，空ledger须0，冲突/跳号/溢出拒绝。新行write_all/flush后按Durable同步，文件或目录同步错误传播；unlock错误忽略。该函数仅校验尾部序列与字节，不执行父Timeline的prefix hash缓存校验，也不在此处折叠完整Sideband生命周期或验证parent引用；读取恢复阶段的独立ledger验证不能归为本追加函数行为。 共享read_timeline_tail以最后换行确定complete_len，空文件或完全无换行返回无末行；使用8KiB固定buffer向前寻找换行，扫描距离无总字节或时间上限。最后完整行在分配前要求内容长度加换行不超过MAX_JSONL_ENTRY_BYTES，再按usize转换及read_exact读取；返回末行不含换行，空末行仍Some(empty)并由调用方解析拒绝。通用行大小检查仅拒绝len>上限，允许正好上限，换行计入追加行预算。

#### Scenario: Same Sideband tail retried
- **WHEN** 传入seq和末行相同且序列化字节相同
- **THEN** 不重复追加，Durable模式仍同步文件及目录。

#### Scenario: Interior prefix differs
- **WHEN** Sideband较早记录被改动但末行仍可解析
- **THEN** 本函数没有完整prefix哈希检查，不能据追加成功证明历史完整性。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn append_sideband_line_sync`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_sideband_event_with_durability`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn append_sideband_event_durable`。

- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn read_timeline_tail`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn find_previous_newline`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn validate_jsonl_line_size`。


### Requirement: Shell strict update envelope export
OpenedSession.update_envelopes SHALL 从绑定目录打开updates，NotFound返回空集合，其他打开错误传播；从offset0迭代换行提交记录，每条trim_ascii后解析JSON Value并以SessionUpdateEnvelope::from_value校验，任一完整记录读取/JSON/typed envelope错误使整个导出失败，不像replay跳过坏记录。返回原始解析Value而非重新序列化typed update，保留Value中通过校验的字段；JSON格式空白和原始字节不保留，诊断index是从0起的迭代条目序号。timeline_events将NotFound转mandatory ledger缺失InvalidData，保留可识别session_version_mismatch原错误，其他InvalidData添加expected schema上下文，其余错误原样传播。

#### Scenario: Corrupt committed update during export
- **WHEN** updates中某条完整行无法通过envelope校验
- **THEN** 整个update_envelopes失败，不返回跳过该行的镜像。

#### Scenario: Missing updates export
- **WHEN** updates文件不存在
- **THEN** 返回空集合。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn update_envelopes`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `pub(crate) fn timeline_events`。


### Requirement: Shell summary format title and visibility validation
Summary.validate_current_format SHALL 首先要求session format等于当前6；title/title_source/title_event_seq须全部None或全部Some，title trim后非空且原字符串chars不超过160。Generated/Fallback来源须sideband_id合法且result_seq/terminal_seq非0，User无额外来源检查；本函数不读取ledger验证引用存在，也不校验cwd generation或路径一致性。is_hidden优先使用显式hidden布尔值，None时仅session_kind以大小写敏感subagent前缀开头则隐藏。display_title返回trim后字符串或空，display_title_opt对空返回None，manual_title_opt仅User来源且trim非空返回文本。last_change_unix_ms选择last_active_at否则updated_at，不取两者最大值。 decode_summary先解析JSON Value并要求session_format_version为u64，异版本在typed body反序列化前返回session_version_mismatch；当前版本才从原始bytes再次反序列化Summary并validate_current_format。decoder本身不限制bytes长度，read_summary_in_session_dir经绑定目录read_bounded施加MAX_SESSION_SUMMARY_BYTES。路径读取包装保留可识别版本错误，其余解码错误添加summary路径上下文；文件打开/读取错误在此映射前直接传播。 Summary serde拒绝未知字段，info/created_at/updated_at/num_messages/current_model_id/session_format_version无字段default；cwd_generation与cwd_switch_bookkeeping_generation缺省0且0省略，git_remotes缺省空且空省略，其余Option元数据缺省None且None省略。Some(false)的hidden及Some(0)的title_event_seq仍序列化。session_kind/context_source等字符串无此结构层枚举约束，source_workspace_dir也无仅worktree可用的结构校验；注释所述使用约定不等于decoder强制条件。 Summary::new同步解析cwd的Git元数据及worktree label，克隆Info并直接接纳传入model_id；cwd两类generation与消息计数为0，title、lineage、activity、agent、sandbox及effort均None。created_at和updated_at分别调用Utc::now，不保证完全相等。grow_home只在路径为有效UTF-8时存字符串，否则None；default_model_id返回空字符串ModelId，不查询模型目录。new函数当前直接Ok构造，不在自身执行validate_current_format或验证模型可用性。

#### Scenario: Explicit visible subagent
- **WHEN** session_kind以subagent开头但hidden=Some(false)
- **THEN** is_hidden返回false，显式设置覆盖默认。

#### Scenario: Partial title projection
- **WHEN** title存在但source或event_seq缺失
- **THEN** validate_current_format返回InvalidData。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn validate_current_format`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn is_hidden`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn manual_title_opt`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn last_change_unix_ms`。

- `crates/codegen/shell/src/session/persistence.rs` — `fn hidden_for_all_subagent_kinds`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn not_hidden_for_regular_sessions`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn explicit_hidden_overrides_session_kind`。

- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn decode_summary`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn read_summary_in_session_dir`。


- `crates/codegen/shell/src/session/persistence.rs` — `pub struct Summary {`。

- `crates/codegen/shell/src/session/persistence.rs` — `mod agent_name_persistence_tests`（测试源码审阅，未运行）。

- `crates/codegen/shell/src/session/persistence.rs` — `mod title_projection_tests`（测试源码审阅，未运行）。
- `crates/codegen/shell/src/session/persistence.rs` — `fn summary_relocation_metadata_round_trips`（测试源码审阅，未运行）。

- `crates/codegen/shell/src/session/persistence.rs` — `pub fn new(info: &Info, model_id:`。

### Requirement: Shell resumed sandbox profile local selection
resumed_session_sandbox_profile SHALL 优先使用非空session_id跨cwd本地查找并返回其sandbox_profile；该ID找不到、读取失败或profile=None时直接None，不回退cwd。仅ID缺失或空字符串才按cwd选择活动排序最前的本地summary，再取profile，不继续搜索更旧的有profile会话。ID只检查is_empty不trim。best-effort查找将adapter构造和列表/查找错误降为None；local_summaries_for_cwd_sync则保留Result并传播列表级错误，仍沿adapter规则跳过单个坏候选。sessions_root helper取其parent建立FromRoot adapter，不将任意末级名称作为可自定义sessions目录。上述入口只返回持久化profile字符串，不在此处应用OS sandbox或验证profile配置存在。

#### Scenario: Explicit id has no profile
- **WHEN** 非空ID匹配会话但其sandbox_profile=None，同时提供cwd
- **THEN** 返回None，不改查cwd或更旧会话。

#### Scenario: Newest cwd session lacks profile
- **WHEN** cwd最新会话无profile但更旧会话有profile
- **THEN** 返回None，不扫描寻找非空profile。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `fn resumed_session_sandbox_profile_in_root`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn most_recent_local_summary_for_cwd_in_root`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn local_summaries_for_cwd_sync_in_root`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn storage_for_sessions_root`。


- `crates/codegen/shell/src/session/persistence.rs` — `mod resumed_sandbox_profile_tests`（测试源码审阅，未运行）。

### Requirement: Shell prompt blob reference extraction and freezing
prompt blob引用提取 SHALL 遍历各ConversationItem.text_content的lines，仅匹配行首artifact:prompt:blake3:，去除行尾CR后要求剩余hash正好64个小写十六进制字符；匹配前缀但hash非法使整体失败，不trim空白，不扫描任意行内子串。BTreeSet去重排序。验证读取打开绑定session的prompts目录，对hash.txt有界读取64MiB，再比较原始字节BLAKE3与hash；缺文件保留NotFound并添加引用路径说明，hash不符InvalidData，不额外要求UTF-8。freeze返回hash到完整bytes的BTreeMap，任一读取错误整体失败，不返回部分集合；单blob限额不等于总集合内存限额。路径版freeze逐hash重新打开session目录，directory版复用传入句柄，因此不将路径版描述为跨多blob固定单一目录身份。 初始blob写入先要求map keys与surface引用集合完全相等，缺失或多余均InvalidData；再依BTreeMap顺序逐项验证hash并写prompts/hash.txt，后项失败不由该函数回滚先前写入，staging事务的清理由外层负责。verify_timeline_prompt_blobs先调用sampling_evidence::verify，再收集完整Timeline所有Messages事件的引用并验证，不仅验证当前selected surface，返回去重hash数量。 通用write_immutable_blob_to_directory要求content不超过64MiB，提取relative的parent及file_name，创建父目录后以durable=true、replace=false发布。AlreadyExists时有界读既有文件并与content逐字节比较，不同InvalidData，相同则调用parent.sync并传播其错误；其他发布错误直接传播，其他平台Unsupported。该通用函数不自行验证文件名是content hash，也不设置readonly，内容寻址校验由prompt调用方承担；新发布后的目录sync失败可被write_atomic告警吞掉，而已存在相同内容分支的parent.sync失败会返回错误。 get_prompt_blob_path与get_prompt_blob_ref均对输入content原始UTF-8字节计算BLAKE3，不trim或规范化换行；前者仅拼session_dir/prompts/<hex>.txt，后者仅返回artifact:prompt:blake3:<hex>字符串。这两个构造helper不创建目录、不写入或验证blob，也不施加64MiB写入限额。

#### Scenario: Invalid prefixed line
- **WHEN** 文本某行以artifact:prompt:blake3:开始但hash含大写字符或尾空白
- **THEN** 引用提取返回InvalidData。

#### Scenario: Duplicate references
- **WHEN** 多个文本行引用同一合法hash
- **THEN** 只保留一个集合项并冻结一份bytes。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn referenced_prompt_blob_hashes`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn verified_prompt_blob_bytes_from_directory`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn freeze_prompt_blobs(`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn freeze_prompt_blobs_from_directory`。

- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn write_initial_prompt_blobs_to_directory`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn verify_timeline_prompt_blobs_from_directory`。


- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn write_immutable_blob_to_directory`。

- `crates/codegen/shell/src/session/persistence.rs` — `pub fn get_prompt_blob_path`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn get_prompt_blob_ref`。

### Requirement: Shell request scoped prompt blob materialization
directory版prompt引用物化 SHALL 在无引用时返回count0且无临时目录；有引用时创建grow-prompt-export-临时目录，逐hash验证源blob并create_new导出hash.txt，Unix创建mode0400，write_all/sync_all后设readonly，再以std::fs::read读回验证hash，成功才在传入items上字面替换引用为导出路径。返回PromptBlobExport持有TempDir，count是去重引用数，is_request_scoped依据目录Some。函数中途失败不回滚已替换的items，局部TempDir按其生命周期清理；不能据此宣称调用者仍可使用失败后items中的路径。路径版materialize不导出临时文件，仅验证源后替换成session_dir/prompts/hash.txt。两版都使用to_string_lossy和文本replace，不是通用URI解析；导出读回std::fs::read本身无额外字节上限，不能泛称所有读取均有界。

#### Scenario: No prompt references
- **WHEN** items中无合法行首blob引用
- **THEN** directory版返回count0和is_request_scoped=false，不创建目录。

#### Scenario: Later blob fails
- **WHEN** 前一个引用已替换而后一个blob验证或写入失败
- **THEN** 函数返回错误，items可能已部分改写，不回滚前一替换。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn materialize_prompt_blob_refs(`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn materialize_prompt_blob_refs_from_directory`。
- `crates/codegen/shell/src/session/persistence.rs` — `impl PromptBlobExport`。


### Requirement: Shell local session resolution candidate semantics
session_exists_for_cwd SHALL 构造指定id/cwd的Info并以open_session成功判定存在，所有错误转false；resolve_local_session成功返回原ID字符串。repo resolver按调用者candidate_cwds顺序逐项检查同一ID，首项命中标ExactCwd，其余标SameRepoDifferentCwd；不在函数内验证仓库归属，不搜索parent lineage或restored children，空候选None。跨cwd Result入口传播open_session_by_id错误并返回summary cwd或绑定目录display_path；Option包装将错误降为None，无法区分缺失与损坏/重复身份。ResolvedLocalSession字段及resolution kind使用camelCase serde名称。

#### Scenario: Candidate ordering defines resolution kind
- **WHEN** 只有candidate_cwds第二项包含指定ID
- **THEN** 返回SameRepoDifferentCwd，不自行核验是否同一仓库。

#### Scenario: Lookup error through optional wrapper
- **WHEN** 跨cwd查找返回身份冲突错误
- **THEN** Option包装返回None，Result入口保留错误。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `fn session_exists_for_cwd_in_root`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn resolve_local_session_for_repo_in_root`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn resolve_local_session_any_cwd(`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn resolve_local_session_any_cwd_in_root`。


- `crates/codegen/shell/src/session/persistence.rs` — `mod session_exists_tests`（测试源码审阅，未运行）。
- `crates/codegen/shell/src/session/persistence.rs` — `mod find_summary_by_session_id_tests`（测试源码审阅，未运行）。

- `crates/codegen/shell/src/session/persistence.rs` — `mod session_exists_for_cwd_tests`（测试源码审阅，未运行）。
- `crates/codegen/shell/src/session/persistence.rs` — `mod repo_wide_resolution_tests`（测试源码审阅，未运行）。

### Requirement: Shell durable model selection continuity and receipt projection
模型选择恢复 SHALL 仅折叠scope=model、name=changed的Observation，拒绝turn或step、非对象data以及不恰好包含10个规定字段的数据。from/to模型ID、provider名称和reason须trim后非空但保留原字符串；reasoning effort按Option类型解码，传输身份须通过is_valid，非空control_intent须validate。后续匹配事实的from模型、effort、provider、transport必须与上条to逐项一致；首条from不与summary或外部目录核对，无匹配返回None。生成器仅组装这10个字段，不自行执行上述验证。summary reconciliation在存在最新选择且model或effort不同才写投影，保留agent_name，错误传播。durable_model_control_receipts独立提取匹配Observation的control_intent；None不产生回执，Some验证intent及to模型/effort并按事件顺序返回Sampling回执，不自行执行完整10字段、turn/step或连续性校验，不去重。恢复入口将这些回执与控制快照回执合并，以Applied、ui_terminal_durable=false交给admission恢复；该处错误转InvalidConfig。

#### Scenario: Discontinuous model facts
- **WHEN** 第二条匹配事实的from transport与上一条to transport不同
- **THEN** latest_model_selection返回带event seq的InvalidData，不用summary掩盖错误。

#### Scenario: Receipt extraction without intent
- **WHEN** 匹配Observation具有对象data且control_intent为null
- **THEN** 回执提取跳过该事件；这不证明其满足完整模型选择事实校验。

#### Scenario: No durable model selection
- **WHEN** Timeline不存在匹配的model.changed Observation
- **THEN** 选择折叠返回None，模型投影协调保留原summary。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn model_change_event`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn latest_model_selection`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn durable_model_control_receipts`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn reconcile_model_projection`。
- `crates/codegen/shell/src/session/actor/spawn.rs` — `durable_model_control_receipts`。
- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `model_change_repairs_selection_summary_from_timeline`（测试源码已审阅，本轮未运行）。
- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `malformed_model_change_bricks_session_load`（测试源码已审阅，本轮未运行）。
- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `model_change_fold_rejects_transport_discontinuity`（测试源码已审阅，本轮未运行）。
- `crates/codegen/shell/src/session/storage/jsonl/tests.rs` — `model_change_folds_its_atomic_sampling_receipt`（测试源码已审阅，本轮未运行）。


### Requirement: Shell persistence error classification for ACP
io_error_to_acp SHALL 统一返回ACP InternalError，并优先识别类型化SessionVersionMismatch，输出SESSION_VERSION_INCOMPATIBLE及component、persistedVersion、currentVersion，提示新建会话。识别器沿错误链逐层downcast，io::Error优先get_ref，否则使用Error::source，不按错误文本匹配。其余错误按外层kind和raw_os_error分类：InvalidData输出FS_INVALID_DATA和原始detail；Unix ENOSPC/EDQUOT、Windows 112或StorageFull输出FS_DISK_QUOTA_EXCEEDED；NotFound和PermissionDenied分别输出FS_NOT_FOUND及FS_PERMISSION_DENIED，其余告警并输出FS_OTHER。除类型化版本分支外均包含e.to_string的detail；普通磁盘和路径分类不递归查找内层OS错误。仅包含版本字样的InvalidData文本仍为FS_INVALID_DATA，不能冒充类型化版本错误。

#### Scenario: Typed mismatch survives wrapping
- **WHEN** io::Error::other包装类型化Rewind transaction版本不匹配
- **THEN** 返回SESSION_VERSION_INCOMPATIBLE，保留组件和两端版本。

#### Scenario: Text is not a typed mismatch
- **WHEN** 普通InvalidData消息含unsupported timeline schema version
- **THEN** 返回FS_INVALID_DATA并保留具体原因。

#### Scenario: Storage full kind
- **WHEN** 输入ErrorKind::StorageFull
- **THEN** 返回FS_DISK_QUOTA_EXCEEDED及No space left on device消息。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn io_error_to_acp`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) fn session_version_mismatch_from_error`。
- `crates/codegen/shell/src/session/persistence.rs` — `mod io_error_to_acp_tests`。


### Requirement: Shell light resume pinned rewind source and writer handle
load_light SHALL 根据claim_writer选择带writer claim的轻量加载或普通轻量加载，成功后执行worktree touch，再取得绑定session_directory。它立即在该目录句柄下打开rewind_points.jsonl普通文件，存在则保存文件句柄和诊断路径组成PinnedRewindSource，NotFound为None，其余错误终止；此时不解析rewind快照，也不物化客户端updates。返回summary、Timeline events、control、signals、announcement和workflow恢复项。claim_writer=false仍完成上述加载及句柄打开，但返回PersistenceHandle::noop且不启动持久化actor；noop句柄无session_directory、接收端已丢弃。claim_writer=true仅在所有这些步骤成功后创建无界actor channel，句柄持有同一目录Arc并启动带gateway的SessionPersistence。PinnedRewindSource固定所打开文件，不能将命名空间路径固定等同于文件内容不可变或提前验证完整ledger。

#### Scenario: Observational resume
- **WHEN** claim_writer=false且rewind ledger存在
- **THEN** 返回已打开的延迟rewind来源和noop handle，不启动新持久化actor。

#### Scenario: Rewind source admission failure
- **WHEN** 打开rewind ledger返回PermissionDenied
- **THEN** 整个load_light返回错误，不将该错误当作空历史。

#### Scenario: Missing rewind ledger
- **WHEN** 绑定目录中不存在rewind_points.jsonl
- **THEN** 返回rewind_points_source=None，继续按claim_writer决定actor创建。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) async fn load_light`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) struct PersistedInfoLight`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn actor_channel`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn noop()`。


- `crates/codegen/shell/src/session/persistence.rs` — `mod actor_lifetime_tests`（测试源码审阅，未运行）。

### Requirement: Shell session deletion result and cleanup entry policy
delete_session_history SHALL 在spawn_blocking中调用按ID及可选cwd删除的adapter入口，任务JoinError和删除I/O错误均包装DeleteSessionError::Local。未找到实体返回local_removed=false；实际返回被删Info才通知搜索索引更新并返回true，any_removed等同local_removed。索引通知为enqueue，不等待索引删除确认。cleanup_stale_sessions使用进程级Once，只在首次调用中读取配置并执行同步adapter清理；adapter返回错误仅记录并返回，仍消费该次Once机会，后续调用不重试也不采用新的skip路径。TTL从ConfigLayers的effective_config_disk_only读取storage.cleanup_ttl_days，仅接纳正整数，配置失败或其他值回退30天；正整数直接as u32转换，无上界检查，因此超出u32的值按转换截断。入口本身不spawn_blocking，调用方负责避免阻塞异步运行时。 ACP initialize以独立spawn_blocking调用cleanup_stale_sessions(None)，load_session在建立SessionInfo和current_session_dir后、查询resident actor前再次spawn_blocking并传Some(current_session_dir)，两处均不await清理任务。进程Once由实际执行顺序决定首次参数，恢复调用的skip不能覆盖已开始或完成的初始化清理。adapter对skip使用display_path精确路径相等，并以last_active_at否则updated_at与cutoff比较；活动时间等于cutoff保留，旧实体仍须取得writer lease才删除。

#### Scenario: Missing deletion target
- **WHEN** adapter删除查找返回None
- **THEN** 结果local_removed=false且不发送索引通知。

#### Scenario: Cleanup failure consumes once
- **WHEN** 首次cleanup调用的adapter返回错误后再次调用
- **THEN** 首次记录错误，第二次不执行清理或重新读取配置。

#### Scenario: Oversized positive TTL
- **WHEN** 配置cleanup_ttl_days为4294967296
- **THEN** 正整数通过检查但as u32得到0，不回退默认30。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub async fn delete_session_history`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub fn cleanup_stale_sessions`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn resolve_cleanup_ttl_days`。
- `crates/codegen/shell/src/session/persistence.rs` — `impl SessionDeletion`。
- `crates/codegen/shell/src/session/storage/search.rs` — `pub fn notify_session_updated`。
- `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs` — `cleanup_stale_sessions`。


### Requirement: Shell durable append handle acknowledgement classification
PersistenceHandle::append_update_durably SHALL 对noop返回NotCommitted(Unsupported)，向无界channel发送失败返回NotCommitted(BrokenPipe)，发送成功但oneshot关闭返回AcknowledgementLost(BrokenPipe)；actor返回的AppendUpdateError按Committed/NotCommitted原样映射。等待没有本地timeout。append_timeline_event_durably对noop返回Unsupported，发送或确认通道关闭均返回普通BrokenPipe，storage结果按io::Result传播，不提供相同的提交状态枚举。DurableAppendError::retry_exact对三种变体均依据内部io错误的persistence_error_is_permanent取反，不能仅凭变体判断是否重试；此方法本身不执行重试。DurableAppendError的Display委托内部错误，但Error实现没有覆写source，因此不能依靠Error::source遍历取得内部io错误。session_directory只克隆已绑定Arc，is_noop只返回标记。 NotificationSender的durable update采用相同发送失败NotCommitted、确认丢失AcknowledgementLost和storage提交分类映射；durable Sideband则把所有storage io错误包装NotCommitted，不提供Committed分类。两个方法均不读取gateway_enabled、不向gateway转发，也没有noop标记检查或本地timeout。Sideband的NotCommitted在此仅是包装结果，不能证明底层未写入。

#### Scenario: Acknowledgement channel closes
- **WHEN** update成功入队后actor未返回确认便关闭oneshot
- **THEN** 调用方收到AcknowledgementLost，不能据此认定未写入。

#### Scenario: Noop durable append
- **WHEN** 在noop handle请求durable update或Timeline追加
- **THEN** 两者均拒绝Unsupported，update额外包装NotCommitted。

#### Scenario: Timeline acknowledgement loss
- **WHEN** Timeline请求成功发送但确认丢失
- **THEN** 返回普通BrokenPipe，没有DurableAppendError提交分类。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `pub async fn append_update_durably`。
- `crates/codegen/shell/src/session/persistence.rs` — `pub(crate) async fn append_timeline_event_durably`。
- `crates/codegen/shell/src/session/persistence.rs` — `impl DurableAppendError`。
- `crates/codegen/shell/src/session/persistence.rs` — `impl std::error::Error for DurableAppendError`。


- `crates/codegen/shell/src/session/persistence_tests.rs` — `noop_handle_rejects_durable_append`（测试源码已读，未运行）。

- `crates/codegen/shell/src/session/notifications.rs` — `impl NotificationSender`。

### Requirement: Shell persistence notification merge and pending drain
持久化通知合并 SHALL 仅合并相邻同类AgentMessageChunk或AgentThoughtChunk，要求两侧chunk meta均None且内容均Text、两侧Text annotations/meta均None；拼接文本后采用incoming的session_id及notification meta，不检查notification meta相等或session_id相等。空chunk判定只检查上述两类的Text为空及chunk meta=None，不检查Text annotations/meta或notification meta；命中则直接忽略incoming并保留原pending。无pending则暂存incoming；不满足合并条件时暂存incoming并返回旧pending供写入，无本地合并长度上限。drain_pending先取出pending并调用append_update_commit_aware，成功或Committed错误不恢复pending，NotCommitted错误恢复原通知并传播。handle_durable_append先drain_pending，任何错误阻止新durable update发送；flush_pending只记录drain错误，不返回失败。此层依赖storage提交分类，不额外验证磁盘状态或执行独立同步。 普通Update::Acp发生不可合并切换时，旧pending已被incoming替换；旧通知write_update返回NotCommitted仅告警，不重新放回，Committed错误直接忽略。Update::Grow直接写入，不先drain已有ACP pending，错误仅告警。FlushAndAck在flush_pending返回后发送单位值，即使drain失败也发送成功形式的确认，不能视为持久化成功证明。SidebandDurablyAndAck先执行同样只告警的flush_pending，再追加Sideband，确认仅反映Sideband结果；TimelineDurablyAndAck直接追加Timeline，不先drain ACP pending。消息接收顺序因此不保证跨这些投影的实际落盘顺序。 CurrentModel、GitHead和普通RewindPoint直接调用各自storage更新，错误只告警，不排空ACP pending、不向发送方确认。Workflow及rewind带Ack分支发送对应storage Result，发送确认失败被忽略，不停止actor；普通workflow写入/删除仅告警。channel自然关闭后仅调用一次flush_pending再退出，若NotCommitted使pending恢复也不再次循环重试，退出丢弃该内存pending；任务abort不保证执行此尾部flush。

#### Scenario: Empty annotated text
- **WHEN** AgentMessageChunk文本为空且chunk meta=None，但Text annotations存在
- **THEN** 仍被空chunk判定丢弃，不改变已有pending。

#### Scenario: Notification metadata changes
- **WHEN** 两个可合并同类Text chunk的顶层notification meta不同
- **THEN** 文本合并并采用incoming顶层meta，不因顶层meta差异拆分。

#### Scenario: Pending write not committed
- **WHEN** drain_pending收到NotCommitted错误
- **THEN** 恢复旧pending并返回错误，后续durable append不执行。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `fn try_merge_text`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn is_empty_chunk`。
- `crates/codegen/shell/src/session/persistence.rs` — `fn maybe_merge_notification`。
- `crates/codegen/shell/src/session/persistence.rs` — `async fn drain_pending`。
- `crates/codegen/shell/src/session/persistence.rs` — `async fn handle_durable_append`。


- `crates/codegen/shell/src/session/persistence_tests.rs` — `committed_error_returns_sync_disposition`（测试源码已读，未运行）。
- `crates/codegen/shell/src/session/persistence_tests.rs` — `uncommitted_error_returns_restore_disposition`（测试源码已读，未运行）。
- `crates/codegen/shell/src/session/persistence_tests.rs` — `failed_pending_drain_retains_record_and_skips_durable_update`（测试源码已读，未运行）。
- `crates/codegen/shell/src/session/persistence_tests.rs` — `durable_append_drains_pending_update_in_fifo_order`（测试源码已读，未运行）。

### Requirement: Shell durable Timeline post append notifications
持久化actor处理TimelineDurablyAndAck SHALL 先调用storage durable append，仅Result成功时对Messages、Consumed且input=Some、Turn Started、SessionTitle、Subagent和SubagentResult通知搜索索引更新；其他事件不由此分支刷新，包括SubagentSeed。SessionTitle成功还调用notify_client，再发送storage结果确认。索引为enqueue，客户端为forward_fire_and_forget，确认不等待索引完成或客户端接收；gateway=None直接跳过标题通知。SessionInfoUpdate携带title和内层meta的grow/titleEventSeq及grow/titleSource=user/generated/fallback，不设置updatedAt，也不携带来源sideband引用细节。存储追加错误时不执行这些通知，即使底层磁盘可能已写入；成功重试也不由该分支去重通知。

#### Scenario: No gateway title append
- **WHEN** SessionTitle durable append成功且gateway=None
- **THEN** 仍可通知搜索索引并确认storage成功，但不发送客户端标题通知。

#### Scenario: Failed append
- **WHEN** durable append返回错误
- **THEN** 不触发此分支的索引或标题通知，错误返回等待方。

#### Scenario: Title metadata
- **WHEN** 构造Generated标题的SessionInfoUpdate
- **THEN** meta包含事件序号及generated，不设置updatedAt。

源码证据：
- `crates/codegen/shell/src/session/persistence.rs` — `async fn run(mut self)`。
- `crates/codegen/shell/src/session/actor/summary.rs` — `pub(crate) fn notify_client`。
- `crates/codegen/shell/src/session/actor/summary.rs` — `pub(crate) fn session_info_update`。


### Requirement: Shell session title generation adoption sequencing
标题生成调度 SHALL 对trim后空user_text直接返回，否则take一次性route；缺route不生成。materialize失败恢复route，成功冻结最后一个Timeline事件范围；activity无法启动则返回且不恢复已take的route，成功以spawn_local运行。生成先begin SessionTitle Sideband并记录attempt，再在SESSION_TITLE_TIMEOUT内调用provider。begin、attempt、provider admission、usage settlement、Sideband结果/失败终态持久化出错会恢复route并返回，不在此函数自动重试。provider失败或输出解析失败先持久化Failed终态，超时先持久化Cancelled终态，成功后才以其引用采纳fallback标题。provider响应先结算usage再解析标题；有效输出先complete Sideband保存原文、结构化session_title、usage及finish，再追加Generated SessionTitle。Generated或Fallback标题采纳失败仅告警，不恢复route；Sideband成功不代表标题已采纳。commit_session_title仅通过chat_state_handle记录durable SessionTitle并将错误转String。 生产调度并非仅普通prompt：turn admission对User origin调用schedule，直接命令入口在push_user_message_durably成功后也调用，标题源只拼接原prompt_blocks的Text而非命令执行输出。SetSessionTitle先take并丢弃route再尝试User标题durable commit，commit失败也不恢复route；该分支没有取消已启动的生成任务。不能把一次性route描述为调用入口仅执行一次。

#### Scenario: Activity admission fails
- **WHEN** route已take且Timeline已物化，但try_start返回None
- **THEN** 不启动生成任务，也不恢复route。

#### Scenario: Generated title adoption fails
- **WHEN** Sideband complete成功但durable SessionTitle追加失败
- **THEN** 仅告警，已有Sideband结果保留且route不恢复。

#### Scenario: Provider timeout
- **WHEN** provider调用超过标题timeout且Cancelled终态写入成功
- **THEN** 以终态引用构造Fallback来源并尝试采纳。

源码证据：
- `crates/codegen/shell/src/session/actor/summary.rs` — `pub(crate) async fn schedule_session_title`。
- `crates/codegen/shell/src/session/actor/summary.rs` — `async fn generate_session_title`。
- `crates/codegen/shell/src/session/actor/summary.rs` — `async fn persist_title_fallback`。
- `crates/codegen/shell/src/session/actor/summary.rs` — `pub(crate) async fn commit_session_title`。


- `crates/codegen/shell/src/session/actor/tool/dispatch.rs` — `self.schedule_session_title(title_source)`。
- `crates/codegen/shell/src/session/actor/turn/admission.rs` — `self.schedule_session_title(original_prompt_text.clone())`。
- `crates/codegen/shell/src/session/actor/run_loop.rs` — `SessionCommand::SetSessionTitle`。

### Requirement: Shell title request normalization and fallback bounds
标题helper SHALL 先按字面标记移除system-reminder块，未闭合块丢弃余下文本；若剥离结果为空则重新使用原始user_message，再尝试skill display提取，最后在UTF-8字符边界截取最多8000字节。该上限作用于处理后输出，不限制前序分配或扫描。请求包含固定system提示及user_query标签包裹的文本，标签内容不额外转义，使用指定model及backend portable JSON格式；timeout常量30秒。输出schema仅要求唯一session_title字符串，不含160字符或5–10词限制；本地parser拒绝未知字段，将所有空白折叠为空格，拒绝空结果及超过160个Unicode字符，不强制词数。fallback沿相同source处理取前10个空白分词并拼空格，空则New session；无独立160字符限制，单个长词可超出canonical标题上限。

#### Scenario: Reminder only input
- **WHEN** 移除reminder后没有剩余文本
- **THEN** title_source_text回退原始输入再提取和截断，不能保证提醒文本始终消失。

#### Scenario: Long fallback word
- **WHEN** source包含一个超过160字符且不超过8000字节的词
- **THEN** fallback返回该长词，不在helper内截到160字符。

#### Scenario: Strict output normalization
- **WHEN** JSON含合法session_title和未知extra字段
- **THEN** parser返回InvalidJson，而非忽略extra。

源码证据：
- `crates/codegen/shell/src/session/helpers/session_title.rs` — `pub(crate) fn title_source_text`。
- `crates/codegen/shell/src/session/helpers/session_title.rs` — `pub(crate) fn title_fallback_from_user_text`。
- `crates/codegen/shell/src/session/helpers/session_title.rs` — `pub(crate) fn build_session_title_request`。
- `crates/codegen/shell/src/session/helpers/session_title.rs` — `pub(crate) fn parse_session_title_output`。
- `crates/codegen/shell/src/session/helpers/session_title.rs` — `mod tests`。


### Requirement: Shell channel Timeline persistence bridge
ChannelTimelinePersistence SHALL 克隆传入TimelineEvent并以TimelineDurablyAndAck发送到持久化无界channel，立即返回oneshot Receiver，不在桥接层等待或设置timeout。发送失败时创建另一oneshot并预填BrokenPipe错误，返回可正常接收到Err(io)的receiver；发送成功后响应sender丢弃则沿oneshot取消语义体现，两种失败不同。flush仅发送PersistenceMsg::Flush并忽略发送失败，不等待确认，也不在桥接层执行文件同步、事件验证、永久错误分类或重试。

#### Scenario: Dispatch failure
- **WHEN** 持久化channel接收端已关闭
- **THEN** 返回receiver携带BrokenPipe结果，而不是仅让原确认通道取消。

#### Scenario: Flush forwarding
- **WHEN** 调用flush且channel可接收
- **THEN** 只入队Flush，调用不等待磁盘操作。

源码证据：
- `crates/codegen/shell/src/session/timeline_persistence.rs` — `impl TimelinePersistence for ChannelTimelinePersistence`。
- `crates/codegen/shell/src/session/timeline_persistence.rs` — `mod tests`。


### Requirement: Shell turn completion terminal construction
build_turn_completed SHALL 原样传递prompt_id、可选TurnIdentity及PromptUsage，并将stop_reason的JSON字符串取内文、其他JSON值序列化为文本。agent_result为JSON null时映射None，其他值按相同文本转换映射Some；字符串null仍是Some字符串。该纯构造器不校验stop reason枚举、不推断续行、不发送或持久化事件，也不补充缺失identity/usage；调用成功不证明终态已送达或可重放。

#### Scenario: Non string terminal fields
- **WHEN** stop_reason为42且agent_result为对象
- **THEN** 构造stop_reason=42文本及对象JSON文本，不丢弃终态。

#### Scenario: Null result distinction
- **WHEN** agent_result为JSON null
- **THEN** agent_result字段为None，而非字符串null。

源码证据：
- `crates/codegen/shell/src/session/turn_completion.rs` — `pub(crate) fn build_turn_completed`。
- `crates/codegen/shell/src/session/turn_completion.rs` — `fn json_to_string`。
- `crates/codegen/shell/src/session/turn_completion.rs` — `mod tests`。


### Requirement: Shell replay wire discriminant derivation
Replay wire tag helper SHALL 以LazyLock首次使用时序列化代表性enum值，读取sessionUpdate字符串，派生UserMessageChunk、AvailableCommandsUpdate、RewindMarker、TaskBackgrounded和TaskCompleted的标签；序列化失败或标签缺失/非字符串触发expect panic，不返回可恢复错误。AVAILABLE_COMMANDS_UPDATE_PREFIX拼接带左花括号的紧凑JSON标签前缀，USER_MESSAGE_CHUNK_PREFIX仅拼键值对；这些字符串是匹配辅助，不自行解析记录、验证对象路径或接受任意空白/字段顺序的等价JSON。当前测试固定五个snake_case标签及两个前缀，不能由标签派生推断全部回放匹配器已验证。

#### Scenario: Missing discriminant
- **WHEN** 代表性序列化结果不含字符串sessionUpdate
- **THEN** 惰性初始化panic，不静默使用空标签。

#### Scenario: Prefix construction
- **WHEN** 初始化AvailableCommandsUpdate前缀
- **THEN** 生成以左花括号和sessionUpdate键开始的紧凑字符串。

源码证据：
- `crates/codegen/shell/src/session/wire_tags.rs` — `fn tagged_discriminant`。
- `crates/codegen/shell/src/session/wire_tags.rs` — `pub(crate) static AVAILABLE_COMMANDS_UPDATE_PREFIX`。
- `crates/codegen/shell/src/session/wire_tags.rs` — `fn derived_discriminants_match_wire_format`。


### Requirement: Shell replay event timestamp window and flush request
Replay SessionNotification SHALL 区分ACP/Grow并返回内层session ID；streaming分类仅ACP AgentMessageChunk/AgentThoughtChunk与Grow ToolCallDeltaChunk。agentTimestampMs仅从通知顶层meta读取u64；任一缺失或类型不符时timestamp window返回true。两者有效时仅检查incoming<=previous+max_duration，不检查下界，因此倒序时间也可通过；加法为普通u64加法，非饱和。此窗口helper不检查会话ID、通知类型或内容可合并性。flush_replay_actor创建带oneshot的FlushReplay事件并发送，发送失败或ack取消均EventChannelClosed，入队后最多等待5秒，超时Timeout；超时不撤销已入队事件。它是请求actor处理的屏障，不在此执行回放或磁盘同步。

#### Scenario: Missing timestamp
- **WHEN** 任一通知meta没有有效u64时间戳
- **THEN** 窗口判断返回true。

#### Scenario: Earlier incoming timestamp
- **WHEN** incoming时间早于previous且加法不溢出
- **THEN** 仍满足上界判断，helper不拒绝倒序。

#### Scenario: Flush acknowledgement timeout
- **WHEN** FlushReplay已发送但5秒内无ack
- **THEN** 返回Timeout，事件可能之后才被actor处理。

源码证据：
- `crates/codegen/shell/src/session/replay_events.rs` — `impl SessionNotification`。
- `crates/codegen/shell/src/session/replay_events.rs` — `pub(crate) async fn flush_replay_actor`。
- `crates/codegen/shell/src/session/replay_events.rs` — `mod tests`。


### Requirement: Shell replay buffer protocol merge and thresholds
ReplayBuffer SHALL 在settings=None时直接返回incoming；启用时不同session返回旧/新两条并清零，越窗则替换pending返回旧条但保留旧计数。正常路径按是否有旧pending计算饱和count+1并重算合并后payload字节，force或count>=max_items或bytes>=max_bytes时返回且清空，否则暂存；flush取走pending并清零。ACP只合并同类message/thought Text且两侧无annotations，保留旧Text/chunk meta；顶层meta两者存在时仅合并chunkId范围，其余保留旧键。Grow仅合并ToolCallDeltaChunk：两ID都有值按ID相等判定，否则按index，拼arguments并优先旧ID/name、保留旧index，合并后meta=None。不同协议或不可合并返回两条。字节估算仅ACP文本或Grow arguments+name，不计meta、ID、JSON封装；阈值在合并分配后检查，不是峰值内存上限。BufferingSettings派生Default字段全0，而serde缺字段默认100项/2048字节/10ms；本模块不安装定时器。 ACP initialize读取meta.bufferingSettings并serde解码，缺失为None；非法值告警后也转None，不拒绝initialize。空对象使用serde默认100/2048/10而非派生Default的零值；合法零阈值原样接纳。解析结果写入agent.buffering_settings，actor从session保存值构造ReplayBuffer。 run_session在启用buffering时spawn_local周期任务，周期为max(20, max_duration_ms*2)毫秒，乘法为普通u64乘法；零duration因此使用20ms周期。任务每tick入队无ack FlushReplay，直到event channel发送失败退出，不根据pending内容重置周期。actor处理FlushReplay时先flush，若有通知则await emit_buffered，随后即使缓冲为空也发送可选ack；不是每个chunk独立deadline，队列处理延迟不受该周期保证。 emit_buffered的ACP分支调用emit_notification_direct：补event ID后除AvailableCommandsUpdate外尝试普通持久化入队，再按gateway_enabled Relaxed读取决定fire-and-forget转发，入队失败被忽略。Grow分支仅日志及gate开启时编码转发，不持久化、不补event ID、不派发hook，编码失败静默跳过。flush ack因此不证明磁盘写入或客户端接收；日志记录也可能发生在gate关闭时。 flush_to_disk先await flush_replay_actor，失败仅告警并继续；随后发送FlushAndAck，入队成功才无timeout等待，发送失败及ack取消忽略，返回单位值而非持久化结果。

#### Scenario: Same tool ID different indices
- **WHEN** 两个Grow delta均有相同ID但index不同
- **THEN** 允许合并并保留旧index，合并结果meta=None。

#### Scenario: Threshold equality
- **WHEN** 正常合并后估算字节等于max_bytes
- **THEN** 立即返回结果并清空缓冲。

#### Scenario: Window rollover
- **WHEN** incoming超出时间窗口且已有pending
- **THEN** 返回旧pending、替换为incoming，但不重建计数。

源码证据：
- `crates/codegen/shell/src/agent/update_chunk_merge.rs` — `impl ReplayBuffer`。
- `crates/codegen/shell/src/agent/update_chunk_merge.rs` — `fn merge_grow_chunks`。
- `crates/codegen/shell/src/agent/update_chunk_merge.rs` — `fn estimate_payload_bytes`。
- `crates/codegen/shell/src/agent/update_chunk_merge.rs` — `fn merge_meta`。

- `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs` — `Failed to parse buffering settings from init meta`。

- `crates/codegen/shell/src/session/actor/run_loop.rs` — `event_tx_for_flush_timer`。

- `crates/codegen/shell/src/agent/update_chunk_merge.rs` — `mod tests`（源码审阅，未运行）。

- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn emit_buffered`。

- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn flush_to_disk`。

### Requirement: Shell subagent usage fold attribution and acknowledgement

record_subagent_usage SHALL 仅在parent_prompt_id为Some且等于当前prompt时归属prompt，否则返回SessionOnly；subagent_id参数不参与此函数判定或去重。by_model非空或incomplete=true才await chat-state记录，返回false则Err；空数据且完整跳过调用但仍返回归属分类。actor命令在AttributedToPrompt时发送unit ack，在SessionOnly时先尝试标记stamped prompt的report sticky再ack，sticky结果被忽略；Err分支不发送ack，oneshot随分支结束丢弃。mark_apply_miss_incomplete先尝试sticky，再按pin判定prompt ledger标记：匹配live或无pin但有live才标prompt，session始终标记，返回sticky或ledger任一成功；对应命令仅true才ack。finalize_usage_from_outcome仅fail_closed时尝试标记prompt和session ledger，忽略标记结果，再按report_incomplete取得snapshot，之后清指定prompt sticky，即使snapshot为None也清理。上述ack不证明每个sticky与ledger操作均成功，也不代表磁盘提交。

#### Scenario: Mismatched prompt pin
- **WHEN** parent pin与当前prompt不同
- **THEN** usage按session-only提交，命令尝试report sticky后ack。

#### Scenario: Empty complete fold
- **WHEN** by_model为空且incomplete为false
- **THEN** 跳过chat-state记录并返回按pin算出的分类。

#### Scenario: Partial incomplete marking success
- **WHEN** sticky失败但ledger标记成功
- **THEN** mark_apply_miss_incomplete返回true，命令可ack。

源码证据：
- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn record_subagent_usage`。
- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn mark_apply_miss_incomplete`。
- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn finalize_usage_from_outcome`。
- `crates/codegen/shell/src/session/actor/run_loop.rs` — `SessionCommand::RecordSubagentUsage`。
- `crates/codegen/shell/src/session/actor/tests/subagent_usage_fold_tests.rs` — `async fn subagent_usage_fold_attribution_gate`。

### Requirement: Shell prompt usage drain query and snapshot boundaries

freeze_prompt_usage SHALL 默认传120秒预算给drain；drain先await outstanding查询，仅当live_ids非空才检查deadline，未到期则sleep 50ms继续。查询None返回fail_closed，live_ids空直接返回background/sticky状态，即使此时已过deadline。outstanding查询无subagent通道时返回默认空reply，发送失败或oneshot取消返回None，成功发送后await reply没有本地timeout，因此预算不是整个调用硬上限。snapshot先以Relaxed swap(false)消费actor与shared后台未归属标记，再读取prompt ledger并合并incomplete；读取失败投影None ledger且incomplete=true，不恢复已消费标记。report incomplete是fail_closed、background_live、sticky_report的逻辑或。clear sticky仅发送按session/prompt定位的清理事件，无ack且忽略发送失败。

#### Scenario: No subagent coordinator
- **WHEN** 没有subagent_event_tx
- **THEN** outstanding返回默认空reply，不按查询失败处理。

#### Scenario: Pending query beyond budget
- **WHEN** Outstanding消息已发送但reply保持pending
- **THEN** 本地deadline不会中断oneshot等待。

#### Scenario: No remaining live child
- **WHEN** 查询返回live_ids为空
- **THEN** 直接返回reply的background和sticky标志，不再检查deadline。

#### Scenario: Snapshot read fails
- **WHEN** 后台标记已swap清除且ledger读取失败
- **THEN** 请求生成incomplete报告，不恢复后台标记。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/mod.rs` — `pub(super) async fn drain_subagent_usage_for_prompt_bounded`。
- `crates/codegen/shell/src/session/actor/turn/mod.rs` — `pub(super) async fn snapshot_prompt_usage_marked`。
- `crates/codegen/shell/src/session/actor/turn/settlement.rs` — `pub(in crate::session::actor) async fn outstanding_reply_for_prompt`。
- `crates/codegen/shell/src/session/actor/turn/settlement.rs` — `pub(in crate::session::actor) fn clear_subagent_usage_not_applied`。
### Requirement: Pager fork slash leading flag and directive parsing

parse_fork_args SHALL 只在参数开头反复识别`--worktree`与`--no-worktree`，分别设置Some(true/false)；两者组合或同一flag重复返回明确错误。开头遇到`--at`立即返回本版本不支持，是否带turn值不影响；遇到任何其他token即停止flag解析并把从该token起的全部余文作为directive，因此未知`--foo`是prompt而非错误，其后的已知flag也不再解析。函数对整体及每个已识别flag后的余文只做trim_start，不做trim_end，所以directive保留尾随空白；无余文为None。ForkCommand标记session_scoped、参数可选，run不检查session，只把解析结果包装Action::Fork；worktree提问、placeholder、目标turn、实际fork及首prompt发送均由dispatch/Shell负责。

#### Scenario: Unknown flag begins directive
- **WHEN** 参数为`--foo bar --worktree`
- **THEN** worktree_override保持None，完整文本成为directive。

#### Scenario: Trailing directive whitespace
- **WHEN** 参数在有效flag及directive后带空格
- **THEN** 前导分隔空白被移除，但directive尾随空白保留。

证据：`crates/codegen/pager/src/slash/commands/fork.rs` — `ForkArgs / parse_fork_args / ForkCommand::run`。
### Requirement: Pager viewer monotonic turn anchor from wall-clock timestamp

The viewer turn-anchor helper SHALL sample the current monotonic instant, return it when the shell timestamp is absent or not earlier than the current UTC millisecond time, and otherwise subtract the saturating wall-clock delta. If checked monotonic subtraction cannot represent that delta, it falls back to the sampled current instant. The conversion does not preserve an absolute timestamp, compensate for wall-clock changes between shell and pager, or report invalid and out-of-range inputs.

#### Scenario: No timestamp
- **WHEN** turn_start_ms is absent
- **THEN** the sampled current Instant is returned.

#### Scenario: Past timestamp
- **WHEN** UTC now is later than the supplied milliseconds and subtraction is representable
- **THEN** the Instant is back-dated by that delta.

#### Scenario: Future timestamp
- **WHEN** the supplied time is current or future
- **THEN** the sampled current Instant is returned.

#### Scenario: Unrepresentable delta
- **WHEN** checked_sub fails
- **THEN** the sampled current Instant is returned without an error.

证据：`crates/codegen/pager/src/app/acp_handler/prompt_origin.rs` — `viewer_turn_anchor`。

### Requirement: Pager subagent mutable activity projection and waiting label

The subagent activity helpers SHALL mutate only the matching `subagent_sessions` entity projection and leave immutable lifecycle rows untouched. A child view's resolved turn activity is formatted through the shared subagent label formatter; when no activity resolves but the child session is busy, the display label is `Waiting`, and an idle child with no activity has no label. An unknown child key is a silent no-op.

#### Scenario: Known child
- **WHEN** sync receives a child_key present in subagent_sessions
- **THEN** its activity_label is replaced by the supplied optional value.

#### Scenario: Unknown child
- **WHEN** the child_key is absent
- **THEN** no state changes and no error is returned.

#### Scenario: Resolved activity
- **WHEN** the child view resolves current turn activity
- **THEN** the shared formatted activity label is returned.

#### Scenario: Busy without activity
- **WHEN** the child has no resolved activity and its state is busy
- **THEN** Waiting is returned; otherwise the label is absent.

证据：`crates/codegen/pager/src/app/acp_handler/subagent_activity.rs` — `sync_subagent_activity`、`subagent_activity_label`。

### Requirement: Pager orphaned subagent synthetic terminal projection

The killed-subagent finalizer SHALL operate only when the supplied session resolves to a root agent and an unfinished child record matches the supplied subagent id. It serializes and re-enters the canonical `grow/session/update` handler with `SubagentFinished`, preserving the child session id and supplied terminal status while setting error and output absent and tool calls, turns, duration and tokens to zero. Missing roots, missing agents, already-finished or unknown children, serialization failure, or downstream rejection return false. The synthetic record does not recover the original failure, output, counters or runtime.

#### Scenario: Eligible orphan
- **WHEN** a root owns an unfinished matching subagent
- **THEN** a canonical SubagentFinished notification is synthesized and dispatched.

#### Scenario: Synthetic fields
- **WHEN** the terminal update is built
- **THEN** status and child identity are preserved while unavailable detail is empty or zero.

#### Scenario: Idempotence
- **WHEN** the matching child is already finished
- **THEN** the helper returns false without another update.

#### Scenario: Invalid owner
- **WHEN** the session is missing or resolves to a child
- **THEN** the helper returns false.

证据：`crates/codegen/pager/src/app/acp_handler/subagent_activity.rs` — `finalize_killed_subagent`。
### Requirement: Pager late automatic recap admission predicate

Late-recap admission SHALL drop a recap only when all three conditions hold: it is automatic, it is not history replay, and the agent is not idle. Manual recap and replay always remain admissible, as does an automatic recap received while idle. The predicate does not inspect event identity, turn id, recap age or current scrollback ordering.

#### Scenario: Late live auto
- **WHEN** auto is true, replay false and agent idle false
- **THEN** the recap is dropped.

#### Scenario: Replay
- **WHEN** the recap is replayed
- **THEN** it is retained regardless of auto or busy state.

#### Scenario: Manual
- **WHEN** auto is false
- **THEN** it is retained regardless of busy state.

#### Scenario: Idle auto
- **WHEN** the live automatic recap arrives while idle
- **THEN** it is retained.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `should_drop_late_auto_recap`。

### Requirement: Pager recap immutable block commit and manual live-feedback clearing

Recap application SHALL clear the shared `recap` live-feedback entry for manual recaps and then append the supplied RenderBlock to scrollback. Automatic recaps append without that clear. The helper does not deduplicate, validate recap type, check lateness, distinguish retained from native scrollback or report whether the clear changed state.

#### Scenario: Manual recap
- **WHEN** auto is false
- **THEN** recap live feedback is cleared before the block is pushed.

#### Scenario: Automatic recap
- **WHEN** auto is true
- **THEN** the block is pushed without clearing manual progress.

#### Scenario: Duplicate
- **WHEN** the same block is applied twice
- **THEN** this helper appends twice because it has no identity gate.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `apply_recap_block`。
### Requirement: Pager recursive subagent control projection synchronization

Control projection by session id SHALL depth-first search every top-level AgentView and nested subagent view, stop at the first parent containing the child key, and invoke sync_child_control_projection there; no match returns false. Full-tree synchronization SHALL snapshot each view's direct child ids, recursively synchronize each child subtree first, then synchronize that child's projection into its parent, producing post-order propagation without holding overlapping map borrows. Duplicate child ids in different branches are resolved by iteration order in the by-id helper.

#### Scenario: By id
- **WHEN** a nested parent contains the child session key
- **THEN** that parent-child projection is synchronized and true returned.

#### Scenario: Missing
- **WHEN** no subtree contains the key
- **THEN** false is returned.

#### Scenario: Full tree
- **WHEN** an agent has nested descendants
- **THEN** descendants synchronize before their projection is copied upward.

#### Scenario: Duplicate id
- **WHEN** multiple branches contain the same child key
- **THEN** the first branch visited wins for by-id synchronization.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `sync_child_control_projection_by_session_id`、`sync_all_subagent_control_projections`。

### Requirement: Pager root session live event highwater and metadata admission

For a root SessionNotification, Pager SHALL parse NotificationMeta and drop a non-replay event whose event sequence is at or below the session highwater; an admitted non-replay sequence advances that highwater before further routing. Replay is exempt and never seeds it. Unexpected replay is then dropped through the load/reconnect guard. For admitted live deltas with a prompt id differing from current, attached_as_viewer is recomputed from whether Pager originated that prompt. Only non-deduped updates may confirm total tokens or replace turn_start_ms and its prompt binding, preventing stale metadata regression. The highwater assumes live delivery order and cannot distinguish a valid lower sequence arriving late.

#### Scenario: Live duplicate
- **WHEN** event_seq is at or below last_applied_event_seq
- **THEN** render and token/timing mutation are dropped.

#### Scenario: Live advance
- **WHEN** a higher event sequence arrives
- **THEN** the highwater advances before update dispatch.

#### Scenario: Replay
- **WHEN** meta marks replay
- **THEN** highwater comparison and seeding are skipped.

#### Scenario: Viewer derivation
- **WHEN** an admitted live prompt id differs from current
- **THEN** viewer status becomes the inverse of self-originated ownership.

#### Scenario: Fresh metadata
- **WHEN** the event is not deduped
- **THEN** total tokens and turn-start fields may update.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。

### Requirement: Pager root special session info plan and background stdout updates

Before generic tracking, a root SessionInfoUpdate SHALL decode title entities, set generated title for Value, additionally set display_name only for `grow/titleSource=user`, clear both on Null, and treat Undefined as mutation only when context-pressure meta is true; it advances reconnect cursor. A Plan update refreshes watchdog time only for live activity whose prompt id equals current, converts every entry to todo, marks reload todo update, advances reconnect cursor and reports mutation only outside replay/loading. A background ToolCallUpdate consumed by background stdout routing likewise advances reconnect cursor and reports live non-loading mutation without keeping the foreground watchdog alive.

#### Scenario: Generated title
- **WHEN** SessionInfo title is a value
- **THEN** HTML entities decode into generated title and user source also becomes display name.

#### Scenario: Title clear
- **WHEN** title is Null
- **THEN** generated and display names clear.

#### Scenario: Plan
- **WHEN** a plan update arrives
- **THEN** entries replace todo state and reload-todo is marked.

#### Scenario: Plan watchdog
- **WHEN** a live plan carries the current prompt id
- **THEN** last_prompt_event_at is refreshed.

#### Scenario: Background stdout
- **WHEN** the background router consumes a tool update
- **THEN** generic tracker is bypassed and reconnect cursor advances.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`、`todo_item_from_plan_entry`。

### Requirement: Pager root prompt mismatch guard and viewer turn adoption

A live root update whose prompt id differs from current SHALL be dropped before generic tracking when the agent is not attached as viewer; the return mutation remains true only when replay loading is not active. A viewer instead may adopt the mismatching prompt id, clear visible follow-ups while retaining the seen ring, and flush pending follow-ups for the adopted id. Replay never drives adoption. This gate relies on opaque prompt identity and self-originated history; updates without a prompt id bypass the mismatch branch.

#### Scenario: Driver stale prompt
- **WHEN** a non-viewer receives a live different prompt id
- **THEN** the update is not passed to session.handle_update.

#### Scenario: Viewer adoption
- **WHEN** a viewer receives a live different prompt id
- **THEN** current prompt changes and matching buffered follow-ups flush.

#### Scenario: Replay
- **WHEN** a replay update has a different prompt id
- **THEN** this live adoption branch does not run.

#### Scenario: No prompt id
- **WHEN** metadata omits prompt identity
- **THEN** the mismatch guard cannot reject it.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。

### Requirement: Pager root generic tracker side-channel drain and first activity log

An admitted generic root update SHALL optionally detect behavior/plan-mode state, stamp live last_prompt_event_at, pass the update to AgentSession.handle_update, and when tracker activity transitions from absent to present clear in_flight_prompt and emit one `turn.first_activity` unified log per turn-start Instant with elapsed milliseconds and activity label. It drains pending ACP commands as the sole generation-bump site, refreshes workflow-run management and conditionally queues a workflows modal refresh when workflow command fingerprints changed. It also drains workflow definitions, diagnostics, tools and edit-highlight entry ids into their owning views and workers. Repeated activity does not log first activity again for the same start Instant.

#### Scenario: First activity
- **WHEN** tracker activity changes None to Some
- **THEN** rewind stash clears and one TTFA log is emitted for the turn.

#### Scenario: Commands
- **WHEN** pending commands exist
- **THEN** catalog replaces, generation increments once and workflow capabilities refresh.

#### Scenario: Workflow metadata
- **WHEN** pending definitions or diagnostics exist
- **THEN** the workflow view projections replace.

#### Scenario: Tools
- **WHEN** pending ACP tools exist
- **THEN** session available_tools becomes a collected set.

#### Scenario: Edit highlights
- **WHEN** pending entry ids exist
- **THEN** each is submitted to the highlight worker.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。

### Requirement: Pager viewer running chrome and behavior admission drain

After an admitted live generic delta, an attached viewer outside replay/loading SHALL enter TurnRunning without resetting the tracker when its current prompt is neither terminal-replayed nor rewound and state is not already TurnRunning. It stamps observed status time and back-dates turn_started_at from turn_start_ms. Behavior control resolution is applied after reconnect cursor advancement; when a resolution changes in-flight behavior, deferred authoritative controls retry. An Applied resolution matching deferred_session_mode clears that admission latch, may drain the prompt queue, records page-flip state and appends resulting effects. Settings modal refresh and workflow modal refresh are processed after the agent borrow.

#### Scenario: Viewer live turn
- **WHEN** an eligible viewer receives any applied live delta
- **THEN** state becomes TurnRunning and elapsed anchor is established.

#### Scenario: Terminal or rewound
- **WHEN** the current prompt is terminal-replayed or rewound
- **THEN** viewer running chrome is not armed.

#### Scenario: Behavior resolution
- **WHEN** a live update resolves in-flight behavior
- **THEN** deferred authoritative controls are retried.

#### Scenario: Admission release
- **WHEN** Applied matches deferred_session_mode
- **THEN** the latch clears and queue drain effects/page flip are propagated.

#### Scenario: Modal refresh
- **WHEN** plan/command or workflow fingerprints require refresh
- **THEN** the corresponding app-level refresh is issued after mutation.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。

### Requirement: Pager child session update projection and parent activity sync

A Child SessionNotification SHALL locate the direct child from the matched parent, confirm total tokens, replace turn_start_ms without prompt binding, detect plan mode, and pass every update directly to the child session tracker without the root live-event highwater, unexpected-replay guard, prompt mismatch/adoption logic, reconnect cursor or command/workflow/tool side-channel drains in this branch. Edit highlights and behavior resolution/deferred controls are handled; a matching Applied deferred mode may drain the child's queue into parent-scoped effects/page flip. The child's resolved live activity label is then copied to parent subagent info. The return value is parent-agent active status regardless of replay, loading or actual mutation.

#### Scenario: Child update
- **WHEN** a matched direct child exists
- **THEN** its session and scrollback consume the update.

#### Scenario: Metadata
- **WHEN** child meta has tokens or turn start
- **THEN** context and timestamp update without root prompt binding.

#### Scenario: Behavior drain
- **WHEN** an Applied behavior matches the child's deferred mode
- **THEN** the latch clears and effects propagate through the parent app.

#### Scenario: Activity projection
- **WHEN** child handling completes
- **THEN** the parent subagent activity label is synchronized.

#### Scenario: Return
- **WHEN** the parent is active
- **THEN** true is returned even if the child update was replay or made no visible mutation.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。
### Requirement: Pager replayed subagent descriptor merge preserves live lifecycle

Replay merge SHALL replace durable child identity, description and type, apply optional descriptors only when present, and always replace context_normalized while preserving newer lifecycle, counters, activity and kill state.

#### Scenario: Durable fields
- **WHEN** replayed spawn merges into an entity
- **THEN** ids, description and type replace existing values.

#### Scenario: Optional omission
- **WHEN** replay omits an optional descriptor
- **THEN** its live value remains.

#### Scenario: Lifecycle
- **WHEN** terminal/live fields are newer
- **THEN** replay does not regress them.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `merge_replayed_subagent`。

### Requirement: Pager replay admission incident accounting and reconnect cursor consumption

Replay SHALL be accepted only during loading and mark the reload seen; otherwise it is dropped with one warning then debug logs and a saturating incident count. Applied cursor advancement consumes event_id and passes replay status; absent or already-taken ids do nothing.

#### Scenario: Expected
- **WHEN** replay arrives during loading
- **THEN** it is accepted and the reload is marked.

#### Scenario: Unexpected
- **WHEN** replay arrives outside loading
- **THEN** it is dropped and the incident counter increments.

#### Scenario: Applied id
- **WHEN** metadata carries event_id
- **THEN** it is taken into the reconnect cursor.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `drop_unexpected_replay`、`advance_reconnect_cursor`。

### Requirement: Pager Grow session notification routing highwater and forced descendant owner

Grow session routing SHALL forward dedicated task/schedule variants first, otherwise use normal session matching or a forced owner that directly contains the descendant. Child handled updates and nested lifecycle use child highwaters; ordinary root live non-workflow updates use the root Grow highwater. WorkflowUpdated bypasses root highwater. Highwater/cursor advance only on the corresponding handled paths, and control resolution may drain queues.

#### Scenario: Forced owner
- **WHEN** descendant replay names an owner containing the child
- **THEN** global lookup is bypassed.

#### Scenario: Child duplicate
- **WHEN** the child sequence is stale
- **THEN** the handled event is dropped.

#### Scenario: Root duplicate
- **WHEN** an ordinary root Grow sequence is stale
- **THEN** it is dropped.

#### Scenario: Workflow
- **WHEN** WorkflowUpdated arrives
- **THEN** root Grow highwater is neither checked nor seeded.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `handle_session_notification_inner`、`handle_descendant_state_replay`。

### Requirement: Pager durable turn terminal replay seal and live finalization

TurnCompleted during replay loading SHALL seal permission grouping and record the prompt terminal without live finalization. A live terminal SHALL create a deferred TerminalOutcome from prompt, stop reason and optional result, applied only after common cursor/control cleanup.

#### Scenario: Replay
- **WHEN** loading_replay is true
- **THEN** grouping seals and the prompt enters replayed_terminal_prompts.

#### Scenario: Live
- **WHEN** the terminal is live
- **THEN** durable finalization produces a deferred outcome.

#### Scenario: Ordering
- **WHEN** an outcome exists
- **THEN** common cleanup runs before app-level completion effects.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `TurnCompleted`。

### Requirement: Pager subagent spawn replay merge view hydration and lifecycle row

SubagentSpawned SHALL reject duplicate live ids, derive background/model/cwd/worktree/permission/control state, merge replay descriptors without regressing lifecycle, reuse an exact child view or create one with inherited appearance and slash visibility, replay inherited updates once, and insert event-id-deduped lifecycle rows only for non-workflow children. A terminal row observed before spawn remains authoritative.

#### Scenario: Duplicate live
- **WHEN** the child id already exists
- **THEN** a non-replay spawn is ignored.

#### Scenario: Reconnect
- **WHEN** replay repeats an existing child
- **THEN** its concrete view and pending control generations are retained.

#### Scenario: New child
- **WHEN** no view exists
- **THEN** a Vim scrollback child view is created with inherited UI capability.

#### Scenario: Terminal first
- **WHEN** a finished row predates replayed spawn
- **THEN** terminal entity state is recovered without a reversed Started row.

#### Scenario: Workflow
- **WHEN** workflow_run_id exists
- **THEN** generic subagent lifecycle rows are suppressed.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `SubagentSpawned`、`merge_replayed_subagent`。

### Requirement: Pager subagent progress metrics and context projection

SubagentProgress SHALL replace metrics on an existing entity, override a matching child model window only when positive, and synchronize live activity. Unknown entities are not created but the arm still reports change.

#### Scenario: Known
- **WHEN** a tracked child reports progress
- **THEN** metrics, tools and progress time replace its projection.

#### Scenario: Positive window
- **WHEN** a child view exists and context_window_tokens is positive
- **THEN** its model window is overridden.

#### Scenario: Unknown
- **WHEN** no child entity exists
- **THEN** no entity is created although the arm returns true.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `SubagentProgress`。

### Requirement: Pager subagent finish interaction cleanup terminal row and child idle

SubagentFinished SHALL clear child transport interactions/activity, append a deduplicated completed/cancelled/failed row only for an unfinished non-workflow entity, finalize existing metrics and kill state, and set a matching child view Idle. Live child finalization is skipped while parent replay loads; unknown metadata can still yield a generic row without creating an entity.

#### Scenario: Ordinary finish
- **WHEN** an unfinished non-workflow child terminates
- **THEN** one status-mapped row and finalized metrics are produced.

#### Scenario: Existing terminal
- **WHEN** metadata is already finished
- **THEN** no second generic terminal row is added.

#### Scenario: Workflow
- **WHEN** the child belongs to a workflow run
- **THEN** generic terminal rows remain suppressed.

#### Scenario: Child view
- **WHEN** the concrete child exists
- **THEN** it becomes Idle.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `SubagentFinished`。
### Requirement: Pager session recap admission focus marker and unavailable toast

SessionRecap SHALL drop only busy live auto recap; accepted recap marks focus tracking and appends Recap, with manual recap clearing live feedback first. Replay unavailable is ignored; live unavailable clears feedback and shows a toast based on whether user messages exist.

#### Scenario: Busy auto
- **WHEN** live automatic recap arrives while busy
- **THEN** it is dropped.

#### Scenario: Accepted
- **WHEN** manual, replay or idle-auto recap arrives
- **THEN** recap-shown is marked and the immutable event is appended.

#### Scenario: Unavailable
- **WHEN** live unavailability arrives
- **THEN** feedback clears and a history-sensitive toast appears.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `SessionRecap`、`SessionRecapUnavailable`。

### Requirement: Pager child Grow update variant projection and control resync

Child Grow dispatch SHALL support notice, control, model, Agent, interaction, compaction/retry and memory lifecycle variants. Compact completion refreshes child context and parent entity usage. Unsupported or missing-child variants return false, but child control projection sync is attempted after dispatch.

#### Scenario: Supported
- **WHEN** a supported update targets an existing child
- **THEN** its concrete view applies the domain behavior.

#### Scenario: Compaction
- **WHEN** child compaction completes
- **THEN** context and parent usage projection refresh.

#### Scenario: Unsupported
- **WHEN** another variant arrives
- **THEN** changed is false and control sync is still attempted.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `handle_child_session_notification`。

### Requirement: Pager session creation loading worktree restoration and fork effects

The pager root executor SHALL create or load ACP sessions with the selected cwd, discovered MCP servers and SessionFlags metadata, and SHALL preflight explicit session ids before creating them. Worktree creation SHALL use grow/git/worktree/create_from_worktree_sync followed by NewSession; worktree resume SHALL use grow/git/worktree/resume_session and project its session id, paths and restore metadata; direct fork SHALL use grow/session/fork and require a returned newSessionId. LoadSession SHALL add grow/restore_code only for Some(true), parse restore and foreground metadata and preserve the requested session id in its TaskResult. This file does not prove worktree filesystem isolation, Git copy semantics, session-id preflight atomicity, MCP server validity or cleanup of a worktree after session creation fails.

#### Scenario: Explicit id collision
- **WHEN** an explicit new session id fails the availability preflight
- **THEN** no NewSession request is sent and a sanitized session failure is returned.

#### Scenario: Resume existing worktree session
- **WHEN** load_session_id is present
- **THEN** resume_session receives source cwd, copy mode, worktree type, optional restoreCode and gitRef, and its result is mapped to WorktreeForked.

#### Scenario: Create worktree session
- **WHEN** no load_session_id is present
- **THEN** a worktree is created first and NewSession is issued from the derived session cwd.

#### Scenario: Load response
- **WHEN** ACP LoadSession succeeds
- **THEN** models, restoration summary/degree and foreground metadata are returned; ACP failure becomes SessionLoadFailed.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Pager session discovery mutation and information effects

The pager root executor SHALL request session picker, roster and dashboard data through grow/session/list or grow/sessions/list, debounce picker search, and load a persisted title independently. Rename and delete SHALL pass session id plus cwd and treat a non-null response error as failure. Session info SHALL use the typed grow/session/info envelope for agent name, context and formatted display; session usage SHALL parse the bare grow/session/usage response and translate method-not-found to an unsupported-version message. The formatted info SHALL include title when present, shell version, session, cwd, model, optional fingerprint/backend/sandbox, turn and context. This file does not prove picker parsing, persistence freshness, title uniqueness, delete authorization or the accuracy of the hard-coded authentication label.

#### Scenario: Unfiltered picker
- **WHEN** FetchSessionList has no query
- **THEN** the request includes limit 30 and allowRelax true; a query instead supplies query and omits allowRelax.

#### Scenario: Malformed dashboard result
- **WHEN** the dashboard list response contains an error or ACP fails
- **THEN** DashboardSessionsLoaded carries an empty session list.

#### Scenario: Session mutation error
- **WHEN** rename or delete returns a non-null error member
- **THEN** the corresponding Failed TaskResult carries its string or JSON representation.

#### Scenario: Usage unsupported
- **WHEN** grow/session/usage returns method-not-found
- **THEN** SessionUsageFailed reports that the agent version does not support it.

#### Scenario: Title hydration
- **WHEN** a local summary has a manual title or display title
- **THEN** SessionTitleFromDisk returns the title and whether it was manual; read/join failures become None.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/acp_conversion.rs session timeline and state model contract

crates/codegen/shell/src/session/acp_conversion.rs SHALL 维护 session timeline and state model 的入口 PathRewriter, new, rewrite, rewrite_path, rewrite_json, maybe_rewrite, maybe_rewrite_path, raw_output_json, acp_tool_update, rather, acp_plan_update, coordination_tools_publish_full_standard_acp_results_and_terminal_status, test_acp_tool_update_read_file_success, test_acp_tool_update_todo_returns_completed, test_turn_end_plan_cleanup_preserves_semantics_and_priority, test_acp_tool_update_todo_duplicate_id_returns_failed, test_acp_plan_update_todo, test_acp_plan_update_todo_duplicate_id_returns_none (plus 21 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PathRewriter, new, rewrite, rewrite_path, rewrite_json, maybe_rewrite, maybe_rewrite_path, raw_output_json, acp_tool_update, rather, acp_plan_update, coordination_tools_publish_full_standard_acp_results_and_terminal_status, test_acp_tool_update_read_file_success, test_acp_tool_update_todo_returns_completed, test_turn_end_plan_cleanup_preserves_semantics_and_priority, test_acp_tool_update_todo_duplicate_id_returns_failed, test_acp_plan_update_todo, test_acp_plan_update_todo_duplicate_id_returns_none (plus 21 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/acp_conversion.rs`。

### Requirement: Shell crates/codegen/shell/src/session/acp_types.rs session timeline and state model contract

crates/codegen/shell/src/session/acp_types.rs SHALL 维护 session timeline and state model 的入口 CompactConversationRequest, CompactConversationStatus, CompactConversationResponse, Citation, CommentRequest, CommentResponse, CommentDeleteRequest, CommentDeleteResponse, RewindMode, RewindRequest, RewindResponse, RewindConflictInfo, RewindPointsRequest, RewindPointsResponse, RewindPointInfo, TokenUsageCategory, skills_listing, mcp_servers (plus 27 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CompactConversationRequest, CompactConversationStatus, CompactConversationResponse, Citation, CommentRequest, CommentResponse, CommentDeleteRequest, CommentDeleteResponse, RewindMode, RewindRequest, RewindResponse, RewindConflictInfo, RewindPointsRequest, RewindPointsResponse, RewindPointInfo, TokenUsageCategory, skills_listing, mcp_servers (plus 27 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/acp_types.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/completion_delivery.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/completion_delivery.rs SHALL 维护 session actor lifecycle and notifications 的入口 DeliveryState, DeliveryEntry, DeliveryInner, CompletionDeliveryTracker, begin_wait, finish_wait, consume, defer_wait, defer_turn_waits, consume_turn_waits, complete, generation, wait_generation_change, has_ready, ready_ids, contains, wait_task_ids, drain_deferred_completions (plus 8 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** DeliveryState, DeliveryEntry, DeliveryInner, CompletionDeliveryTracker, begin_wait, finish_wait, consume, defer_wait, defer_turn_waits, consume_turn_waits, complete, generation, wait_generation_change, has_ready, ready_ids, contains, wait_task_ids, drain_deferred_completions (plus 8 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** DeliveryState, DeliveryEntry, DeliveryInner, CompletionDeliveryTracker, begin_wait, finish_wait, consume, defer_wait, defer_turn_waits, consume_turn_waits, complete, generation, wait_generation_change, has_ready, ready_ids, contains, wait_task_ids, drain_deferred_completions (plus 8 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/completion_delivery.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/context_recall.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/context_recall.rs SHALL 维护 session actor lifecycle and notifications 的入口 CONTEXT_RECALL_TIMEOUT, MAX_ARCHIVE_ITEM_CHARS, MAX_ARCHIVE_BUDGET_TOKENS, MAX_NEED_CONTEXT_TOKENS, MAX_RECALL_OUTPUT_TOKENS, MIN_RECALL_OUTPUT_TOKENS, MIN_RECALL_ARCHIVE_TOKENS, MAX_RECALL_SYNTHESIS_ATTEMPTS, MAX_RECALLED_TOPIC_CHARS, CONTEXT_RECALL_SYSTEM_PROMPT, ContextRecallRequest, ContextRecallReceiver, ShellContextRecallBackend, context_recall_channel, recall, serve_context_recall, run_context_recall, context_recall_result_wrapper_tokens (plus 62 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CONTEXT_RECALL_TIMEOUT, MAX_ARCHIVE_ITEM_CHARS, MAX_ARCHIVE_BUDGET_TOKENS, MAX_NEED_CONTEXT_TOKENS, MAX_RECALL_OUTPUT_TOKENS, MIN_RECALL_OUTPUT_TOKENS, MIN_RECALL_ARCHIVE_TOKENS, MAX_RECALL_SYNTHESIS_ATTEMPTS, MAX_RECALLED_TOPIC_CHARS, CONTEXT_RECALL_SYSTEM_PROMPT, ContextRecallRequest, ContextRecallReceiver, ShellContextRecallBackend, context_recall_channel, recall, serve_context_recall, run_context_recall, context_recall_result_wrapper_tokens (plus 62 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** CONTEXT_RECALL_TIMEOUT, MAX_ARCHIVE_ITEM_CHARS, MAX_ARCHIVE_BUDGET_TOKENS, MAX_NEED_CONTEXT_TOKENS, MAX_RECALL_OUTPUT_TOKENS, MIN_RECALL_OUTPUT_TOKENS, MIN_RECALL_ARCHIVE_TOKENS, MAX_RECALL_SYNTHESIS_ATTEMPTS, MAX_RECALLED_TOPIC_CHARS, CONTEXT_RECALL_SYSTEM_PROMPT, ContextRecallRequest, ContextRecallReceiver, ShellContextRecallBackend, context_recall_channel, recall, serve_context_recall, run_context_recall, context_recall_result_wrapper_tokens (plus 62 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** CONTEXT_RECALL_TIMEOUT, MAX_ARCHIVE_ITEM_CHARS, MAX_ARCHIVE_BUDGET_TOKENS, MAX_NEED_CONTEXT_TOKENS, MAX_RECALL_OUTPUT_TOKENS, MIN_RECALL_OUTPUT_TOKENS, MIN_RECALL_ARCHIVE_TOKENS, MAX_RECALL_SYNTHESIS_ATTEMPTS, MAX_RECALLED_TOPIC_CHARS, CONTEXT_RECALL_SYSTEM_PROMPT, ContextRecallRequest, ContextRecallReceiver, ShellContextRecallBackend, context_recall_channel, recall, serve_context_recall, run_context_recall, context_recall_result_wrapper_tokens (plus 62 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/context_recall.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/coordination.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/coordination.rs SHALL 维护 session actor lifecycle and notifications 的入口 QueuedCoordinationInquiry, enqueue_coordination_inquiry, run_coordination_inquiry_queue, handle_coordination_inquiry, complete_rejected_coordination_inquiry_with_audit, persist_coordination_inquiry, record_incoming_coordination_notice, record_coordination_approval_notice, record_coordination_terminal_notice, pending_coordination_notices, recover_interrupted_coordination_inquiries, publish_coordination_state, request_coordination_approval, CoordinationApproval, reject_inquiry, failed, inquiry, inquiry_restore_closes_only_open_timeline_receipts_without_ui_replay (plus 5 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** QueuedCoordinationInquiry, enqueue_coordination_inquiry, run_coordination_inquiry_queue, handle_coordination_inquiry, complete_rejected_coordination_inquiry_with_audit, persist_coordination_inquiry, record_incoming_coordination_notice, record_coordination_approval_notice, record_coordination_terminal_notice, pending_coordination_notices, recover_interrupted_coordination_inquiries, publish_coordination_state, request_coordination_approval, CoordinationApproval, reject_inquiry, failed, inquiry, inquiry_restore_closes_only_open_timeline_receipts_without_ui_replay (plus 5 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** QueuedCoordinationInquiry, enqueue_coordination_inquiry, run_coordination_inquiry_queue, handle_coordination_inquiry, complete_rejected_coordination_inquiry_with_audit, persist_coordination_inquiry, record_incoming_coordination_notice, record_coordination_approval_notice, record_coordination_terminal_notice, pending_coordination_notices, recover_interrupted_coordination_inquiries, publish_coordination_state, request_coordination_approval, CoordinationApproval, reject_inquiry, failed, inquiry, inquiry_restore_closes_only_open_timeline_receipts_without_ui_replay (plus 5 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** QueuedCoordinationInquiry, enqueue_coordination_inquiry, run_coordination_inquiry_queue, handle_coordination_inquiry, complete_rejected_coordination_inquiry_with_audit, persist_coordination_inquiry, record_incoming_coordination_notice, record_coordination_approval_notice, record_coordination_terminal_notice, pending_coordination_notices, recover_interrupted_coordination_inquiries, publish_coordination_state, request_coordination_approval, CoordinationApproval, reject_inquiry, failed, inquiry, inquiry_restore_closes_only_open_timeline_receipts_without_ui_replay (plus 5 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/coordination.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/extensions/idle_prompt.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/extensions/idle_prompt.rs SHALL 维护 session actor lifecycle and notifications 的入口 DEFAULT_IDLE_NOTIFICATION_DELAY, idle_notification_delay, resolve_idle_notification_delay, IdlePromptExtension, new, on_turn_start, on_turn_done, on_turn_failed, shutdown, on_session_idle, delay_defaults_to_sixty_seconds, delay_override_is_milliseconds, malformed_delay_uses_default。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、hook dispatch or hook source boundary、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** DEFAULT_IDLE_NOTIFICATION_DELAY, idle_notification_delay, resolve_idle_notification_delay, IdlePromptExtension, new, on_turn_start, on_turn_done, on_turn_failed, shutdown, on_session_idle, delay_defaults_to_sixty_seconds, delay_override_is_milliseconds, malformed_delay_uses_default 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/extensions/idle_prompt.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/extensions.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/extensions.rs SHALL 维护 session actor lifecycle and notifications 的入口 the file module entrypoint。实现显示该边界包含 session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/actor/extensions.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/goal.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/goal.rs SHALL 维护 session actor lifecycle and notifications 的入口 goal_view_from_snapshot, active_goal_edit_rebinds_context_and_tool_authority_at_the_next_step, edit_that_reactivates_goal_forces_the_current_behavior_turn_to_end, out_of_band_goal_entry_commits_before_normal_foreground_cancellation, normal_active_turn_can_create_and_activate_a_goal_atomically, goal_cannot_overtake_queued_model_and_agent_controls, goal_admission_rechecks_agent_after_an_in_flight_step_control, stale_goal_definition_cannot_admit_a_continuation, queued_route_control_blocks_goal_continuation_admission, stale_goal_mutation_authority_cannot_complete_revised_goal, goal_bookkeeping_checkpoint_allows_completion_and_stops_the_loop, stale_create_authority_cannot_recreate_goal_after_control_change, goal_definition_context, schedule_goal_on_idle, restore_goal_snapshot, commit_goal_stop_or_restore, publish_goal_lifecycle_transition, commit_goal_activation_or_restore (plus 16 additional private symbols)。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** goal_view_from_snapshot, active_goal_edit_rebinds_context_and_tool_authority_at_the_next_step, edit_that_reactivates_goal_forces_the_current_behavior_turn_to_end, out_of_band_goal_entry_commits_before_normal_foreground_cancellation, normal_active_turn_can_create_and_activate_a_goal_atomically, goal_cannot_overtake_queued_model_and_agent_controls, goal_admission_rechecks_agent_after_an_in_flight_step_control, stale_goal_definition_cannot_admit_a_continuation, queued_route_control_blocks_goal_continuation_admission, stale_goal_mutation_authority_cannot_complete_revised_goal, goal_bookkeeping_checkpoint_allows_completion_and_stops_the_loop, stale_create_authority_cannot_recreate_goal_after_control_change, goal_definition_context, schedule_goal_on_idle, restore_goal_snapshot, commit_goal_stop_or_restore, publish_goal_lifecycle_transition, commit_goal_activation_or_restore (plus 16 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** goal_view_from_snapshot, active_goal_edit_rebinds_context_and_tool_authority_at_the_next_step, edit_that_reactivates_goal_forces_the_current_behavior_turn_to_end, out_of_band_goal_entry_commits_before_normal_foreground_cancellation, normal_active_turn_can_create_and_activate_a_goal_atomically, goal_cannot_overtake_queued_model_and_agent_controls, goal_admission_rechecks_agent_after_an_in_flight_step_control, stale_goal_definition_cannot_admit_a_continuation, queued_route_control_blocks_goal_continuation_admission, stale_goal_mutation_authority_cannot_complete_revised_goal, goal_bookkeeping_checkpoint_allows_completion_and_stops_the_loop, stale_create_authority_cannot_recreate_goal_after_control_change, goal_definition_context, schedule_goal_on_idle, restore_goal_snapshot, commit_goal_stop_or_restore, publish_goal_lifecycle_transition, commit_goal_activation_or_restore (plus 16 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/goal.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/goal_support.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/goal_support.rs SHALL 维护 session actor lifecycle and notifications 的入口 GoalUsageWindow, GoalUsageWindowState, has_unsettled_owner_attempt, GoalProviderWindow, GoalUsageIncompleteApply, applied, GoalUsageAttemptOwner, new, sync, sync_with_goal_state, active_goal_id, provider_admission_closed, usage_incomplete_goal_id, close_goal_admission, owner_epoch, advance_owner_epoch, begin_model_attempt, begin_model_attempt_with_background (plus 78 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** GoalUsageWindow, GoalUsageWindowState, has_unsettled_owner_attempt, GoalProviderWindow, GoalUsageIncompleteApply, applied, GoalUsageAttemptOwner, new, sync, sync_with_goal_state, active_goal_id, provider_admission_closed, usage_incomplete_goal_id, close_goal_admission, owner_epoch, advance_owner_epoch, begin_model_attempt, begin_model_attempt_with_background (plus 78 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** GoalUsageWindow, GoalUsageWindowState, has_unsettled_owner_attempt, GoalProviderWindow, GoalUsageIncompleteApply, applied, GoalUsageAttemptOwner, new, sync, sync_with_goal_state, active_goal_id, provider_admission_closed, usage_incomplete_goal_id, close_goal_admission, owner_epoch, advance_owner_epoch, begin_model_attempt, begin_model_attempt_with_background (plus 78 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/goal_support.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/hook_dispatch.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/hook_dispatch.rs SHALL 维护 session actor lifecycle and notifications 的入口 HookAggregate, HookDispatchPolicy, into_tool_decision, into_stop_result, PlannedHandler, runtime_name, OccurrencePlanEntry, timeline_event, timeline_gate, timeline_skip, timeline_kind, timeline_provenance, frozen_handler_source_allows_execution, result_outcome, result_elapsed_ms, typed_failure_reason, str, admission_failure_block_reason (plus 36 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** HookAggregate, HookDispatchPolicy, into_tool_decision, into_stop_result, PlannedHandler, runtime_name, OccurrencePlanEntry, timeline_event, timeline_gate, timeline_skip, timeline_kind, timeline_provenance, frozen_handler_source_allows_execution, result_outcome, result_elapsed_ms, typed_failure_reason, str, admission_failure_block_reason (plus 36 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** HookAggregate, HookDispatchPolicy, into_tool_decision, into_stop_result, PlannedHandler, runtime_name, OccurrencePlanEntry, timeline_event, timeline_gate, timeline_skip, timeline_kind, timeline_provenance, frozen_handler_source_allows_execution, result_outcome, result_elapsed_ms, typed_failure_reason, str, admission_failure_block_reason (plus 36 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/hook_dispatch.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/hooks.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/hooks.rs SHALL 维护 session actor lifecycle and notifications 的入口 HOOK_EVENT_METHOD, HOOK_RUN_METHOD, CLIENT_HOOK_TIMEOUT, CLIENT_STOP_GATE_TIMEOUT, next_hook_config_generation, ReverseOutcome, PlannedClientHook, classify, matching_callback_ids, matching_gate_callbacks, dispatch_params, hook_config_generation, advance_hook_config_generation, replace_hook_registry, replace_client_hooks, mark_hook_registry_changed, plan_client_hooks, make_hook_envelope (plus 16 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** HOOK_EVENT_METHOD, HOOK_RUN_METHOD, CLIENT_HOOK_TIMEOUT, CLIENT_STOP_GATE_TIMEOUT, next_hook_config_generation, ReverseOutcome, PlannedClientHook, classify, matching_callback_ids, matching_gate_callbacks, dispatch_params, hook_config_generation, advance_hook_config_generation, replace_hook_registry, replace_client_hooks, mark_hook_registry_changed, plan_client_hooks, make_hook_envelope (plus 16 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** HOOK_EVENT_METHOD, HOOK_RUN_METHOD, CLIENT_HOOK_TIMEOUT, CLIENT_STOP_GATE_TIMEOUT, next_hook_config_generation, ReverseOutcome, PlannedClientHook, classify, matching_callback_ids, matching_gate_callbacks, dispatch_params, hook_config_generation, advance_hook_config_generation, replace_hook_registry, replace_client_hooks, mark_hook_registry_changed, plan_client_hooks, make_hook_envelope (plus 16 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/hooks.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/hooks_plugins.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/hooks_plugins.rs SHALL 维护 session actor lifecycle and notifications 的入口 do_hooks_trust_project, do_hooks_untrust_project, reseed_mcp_output_cap, resolve_path, handle_hooks_action, handle_plugins_action, reload_hooks_impl, reload_plugins_impl, report_plugin_path_change, session_plugin_dirs, preserve_session_plugin_dirs, apply_plugin_registry_snapshot。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** do_hooks_trust_project, do_hooks_untrust_project, reseed_mcp_output_cap, resolve_path, handle_hooks_action, handle_plugins_action, reload_hooks_impl, reload_plugins_impl, report_plugin_path_change, session_plugin_dirs, preserve_session_plugin_dirs, apply_plugin_registry_snapshot 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/hooks_plugins.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/idle_arbitration.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/idle_arbitration.rs SHALL 维护 session actor lifecycle and notifications 的入口 spawn_manual_compaction, maybe_start_pending_manual_compaction, arbitrate_idle_wake, admit_manual_compaction, terminating_idle_wake_does_not_admit_any_work, restored_notification_wins_an_idle_permit_before_goal_continuation, active_goal_continues_after_manual_compaction_releases_the_foreground, user_input_arriving_before_async_goal_admission_keeps_priority。实现显示该边界包含 explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** spawn_manual_compaction, maybe_start_pending_manual_compaction, arbitrate_idle_wake, admit_manual_compaction, terminating_idle_wake_does_not_admit_any_work, restored_notification_wins_an_idle_permit_before_goal_continuation, active_goal_continues_after_manual_compaction_releases_the_foreground, user_input_arriving_before_async_goal_admission_keeps_priority 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** spawn_manual_compaction, maybe_start_pending_manual_compaction, arbitrate_idle_wake, admit_manual_compaction, terminating_idle_wake_does_not_admit_any_work, restored_notification_wins_an_idle_permit_before_goal_continuation, active_goal_continues_after_manual_compaction_releases_the_foreground, user_input_arriving_before_async_goal_admission_keeps_priority 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/idle_arbitration.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/input_admission.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/input_admission.rs SHALL 维护 session actor lifecycle and notifications 的入口 AdmittedInput, admit_image, large_input_recovery_reuses_admission_and_snapshot, missing_input_attachment_is_invalidated_without_blocking_other_inputs, write_failure_retry_has_one_submission_and_gc_keeps_consumed_images, admit_human_input, run_human_input_admission_hook, resolve_human_input_admission, reroute_input, reroute_input_ids, consume_steer_inputs, consume_fifo_inputs, complete_unmodeled_fifo_inputs, dismiss_input_ids, record_input_event, restore_pending_human_inputs, reconcile_input_payloads。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** AdmittedInput, admit_image, large_input_recovery_reuses_admission_and_snapshot, missing_input_attachment_is_invalidated_without_blocking_other_inputs, write_failure_retry_has_one_submission_and_gc_keeps_consumed_images, admit_human_input, run_human_input_admission_hook, resolve_human_input_admission, reroute_input, reroute_input_ids, consume_steer_inputs, consume_fifo_inputs, complete_unmodeled_fifo_inputs, dismiss_input_ids, record_input_event, restore_pending_human_inputs, reconcile_input_payloads 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** AdmittedInput, admit_image, large_input_recovery_reuses_admission_and_snapshot, missing_input_attachment_is_invalidated_without_blocking_other_inputs, write_failure_retry_has_one_submission_and_gc_keeps_consumed_images, admit_human_input, run_human_input_admission_hook, resolve_human_input_admission, reroute_input, reroute_input_ids, consume_steer_inputs, consume_fifo_inputs, complete_unmodeled_fifo_inputs, dismiss_input_ids, record_input_event, restore_pending_human_inputs, reconcile_input_payloads 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** AdmittedInput, admit_image, large_input_recovery_reuses_admission_and_snapshot, missing_input_attachment_is_invalidated_without_blocking_other_inputs, write_failure_retry_has_one_submission_and_gc_keeps_consumed_images, admit_human_input, run_human_input_admission_hook, resolve_human_input_admission, reroute_input, reroute_input_ids, consume_steer_inputs, consume_fifo_inputs, complete_unmodeled_fifo_inputs, dismiss_input_ids, record_input_event, restore_pending_human_inputs, reconcile_input_payloads 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/input_admission.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/interjection.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/interjection.rs SHALL 维护 session actor lifecycle and notifications 的入口 ResidualInterjectionRequeue, PendingInterjection, InterjectionBuffer, admit_human_steer, queue_auto_promoted_follow_up, discard_residual_interjections_at_turn_end, requeue_auto_promoted, prepare_interjection_images, broadcast_interjection, inject_synthetic_user_message, publish_synthetic_user_message, interjection_skill_information, drain_pending_interjections, inject_pending_interjections, close_steering_and_drain, reopen_steering, final_steering_fence_injects_accepted_input_and_rejects_late_input。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ResidualInterjectionRequeue, PendingInterjection, InterjectionBuffer, admit_human_steer, queue_auto_promoted_follow_up, discard_residual_interjections_at_turn_end, requeue_auto_promoted, prepare_interjection_images, broadcast_interjection, inject_synthetic_user_message, publish_synthetic_user_message, interjection_skill_information, drain_pending_interjections, inject_pending_interjections, close_steering_and_drain, reopen_steering, final_steering_fence_injects_accepted_input_and_rejects_late_input 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** ResidualInterjectionRequeue, PendingInterjection, InterjectionBuffer, admit_human_steer, queue_auto_promoted_follow_up, discard_residual_interjections_at_turn_end, requeue_auto_promoted, prepare_interjection_images, broadcast_interjection, inject_synthetic_user_message, publish_synthetic_user_message, interjection_skill_information, drain_pending_interjections, inject_pending_interjections, close_steering_and_drain, reopen_steering, final_steering_fence_injects_accepted_input_and_rejects_late_input 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** ResidualInterjectionRequeue, PendingInterjection, InterjectionBuffer, admit_human_steer, queue_auto_promoted_follow_up, discard_residual_interjections_at_turn_end, requeue_auto_promoted, prepare_interjection_images, broadcast_interjection, inject_synthetic_user_message, publish_synthetic_user_message, interjection_skill_information, drain_pending_interjections, inject_pending_interjections, close_steering_and_drain, reopen_steering, final_steering_fence_injects_accepted_input_and_rejects_late_input 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/interjection.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/laziness.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/laziness.rs SHALL 维护 session actor lifecycle and notifications 的入口 LazinessFireMeta, LazinessFireOutcome, LazinessSuppressReason, build_laziness_debug_line, classify_debug_decision, DebugDecision, DebugClassifierOutput, from, DebugTodoSnapshot, str, LazinessDebugLogLine, append_laziness_debug_log_line, laziness_abort_snapshot, laziness_abort_check, set, emit_laziness_abort, handle_model_switch_for_laziness, maybe_fire_laziness_check (plus 3 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LazinessFireMeta, LazinessFireOutcome, LazinessSuppressReason, build_laziness_debug_line, classify_debug_decision, DebugDecision, DebugClassifierOutput, from, DebugTodoSnapshot, str, LazinessDebugLogLine, append_laziness_debug_log_line, laziness_abort_snapshot, laziness_abort_check, set, emit_laziness_abort, handle_model_switch_for_laziness, maybe_fire_laziness_check (plus 3 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LazinessFireMeta, LazinessFireOutcome, LazinessSuppressReason, build_laziness_debug_line, classify_debug_decision, DebugDecision, DebugClassifierOutput, from, DebugTodoSnapshot, str, LazinessDebugLogLine, append_laziness_debug_log_line, laziness_abort_snapshot, laziness_abort_check, set, emit_laziness_abort, handle_model_switch_for_laziness, maybe_fire_laziness_check (plus 3 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** LazinessFireMeta, LazinessFireOutcome, LazinessSuppressReason, build_laziness_debug_line, classify_debug_decision, DebugDecision, DebugClassifierOutput, from, DebugTodoSnapshot, str, LazinessDebugLogLine, append_laziness_debug_log_line, laziness_abort_snapshot, laziness_abort_check, set, emit_laziness_abort, handle_model_switch_for_laziness, maybe_fire_laziness_check (plus 3 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/laziness.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/laziness_classifier.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/laziness_classifier.rs SHALL 维护 session actor lifecycle and notifications 的入口 LAZINESS_DEFAULT_IDLE_THRESHOLD_MS, LAZINESS_DEFAULT_MIN_CONFIDENCE, LAZINESS_CONTEXT_ITEM_LIMIT, LAZINESS_MIN_USER_TURNS, LAZINESS_MIN_ASSISTANT_TURNS, LAZINESS_CLASSIFIER_TIMEOUT_MS, LAZINESS_ABORT_POLL_INTERVAL_MS, as_const_str, str, all, fn, LAZINESS_USER_PREAMBLE, LAZINESS_CLASSIFIER_PROMPT, LAZINESS_INCLUDE_REASONING, turn_elapsed_seconds_from_start_ms, format_runtime_state_line, flatten_transcript_for_classifier, MAX_FIELD_LEN (plus 18 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LAZINESS_DEFAULT_IDLE_THRESHOLD_MS, LAZINESS_DEFAULT_MIN_CONFIDENCE, LAZINESS_CONTEXT_ITEM_LIMIT, LAZINESS_MIN_USER_TURNS, LAZINESS_MIN_ASSISTANT_TURNS, LAZINESS_CLASSIFIER_TIMEOUT_MS, LAZINESS_ABORT_POLL_INTERVAL_MS, as_const_str, str, all, fn, LAZINESS_USER_PREAMBLE, LAZINESS_CLASSIFIER_PROMPT, LAZINESS_INCLUDE_REASONING, turn_elapsed_seconds_from_start_ms, format_runtime_state_line, flatten_transcript_for_classifier, MAX_FIELD_LEN (plus 18 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LAZINESS_DEFAULT_IDLE_THRESHOLD_MS, LAZINESS_DEFAULT_MIN_CONFIDENCE, LAZINESS_CONTEXT_ITEM_LIMIT, LAZINESS_MIN_USER_TURNS, LAZINESS_MIN_ASSISTANT_TURNS, LAZINESS_CLASSIFIER_TIMEOUT_MS, LAZINESS_ABORT_POLL_INTERVAL_MS, as_const_str, str, all, fn, LAZINESS_USER_PREAMBLE, LAZINESS_CLASSIFIER_PROMPT, LAZINESS_INCLUDE_REASONING, turn_elapsed_seconds_from_start_ms, format_runtime_state_line, flatten_transcript_for_classifier, MAX_FIELD_LEN (plus 18 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/actor/laziness_classifier.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/memory_dream.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/memory_dream.rs SHALL 维护 session actor lifecycle and notifications 的入口 MemoryFlushSnapshot, build_initial_injection_backend_params, register_memory_tools, emit_memory_session_summary, reindex_and_embed, dream_context, maybe_run_dream, DreamActivityGuard, drop, run_dream_slash_command, run_dream_inner, run_dream_model_call, run_memory_flush, FlushActivityGuard, snapshot_memory_flush_state, handle_rewrite_memory_note, MAX_INPUT_BYTES。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MemoryFlushSnapshot, build_initial_injection_backend_params, register_memory_tools, emit_memory_session_summary, reindex_and_embed, dream_context, maybe_run_dream, DreamActivityGuard, drop, run_dream_slash_command, run_dream_inner, run_dream_model_call, run_memory_flush, FlushActivityGuard, snapshot_memory_flush_state, handle_rewrite_memory_note, MAX_INPUT_BYTES 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MemoryFlushSnapshot, build_initial_injection_backend_params, register_memory_tools, emit_memory_session_summary, reindex_and_embed, dream_context, maybe_run_dream, DreamActivityGuard, drop, run_dream_slash_command, run_dream_inner, run_dream_model_call, run_memory_flush, FlushActivityGuard, snapshot_memory_flush_state, handle_rewrite_memory_note, MAX_INPUT_BYTES 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/memory_dream.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/mod.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/mod.rs SHALL 维护 session actor lifecycle and notifications 的入口 SESSION_LOG, InputItem, TerminationState, is_open, request, AdmissionState, restore_terminal_control_intent, admit_control_intent, mark_control_intent_terminal, mark_control_terminal_ui_durable, ControlIntentLifecycle, ControlIntentReceipt, ControlIntentAdmission, ControlIntentTerminal, PendingModelReload, PendingModelSelection, PendingAgentSelection, PendingBehaviorSelection (plus 90 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SESSION_LOG, InputItem, TerminationState, is_open, request, AdmissionState, restore_terminal_control_intent, admit_control_intent, mark_control_intent_terminal, mark_control_terminal_ui_durable, ControlIntentLifecycle, ControlIntentReceipt, ControlIntentAdmission, ControlIntentTerminal, PendingModelReload, PendingModelSelection, PendingAgentSelection, PendingBehaviorSelection (plus 90 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SESSION_LOG, InputItem, TerminationState, is_open, request, AdmissionState, restore_terminal_control_intent, admit_control_intent, mark_control_intent_terminal, mark_control_terminal_ui_durable, ControlIntentLifecycle, ControlIntentReceipt, ControlIntentAdmission, ControlIntentTerminal, PendingModelReload, PendingModelSelection, PendingAgentSelection, PendingBehaviorSelection (plus 90 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** SESSION_LOG, InputItem, TerminationState, is_open, request, AdmissionState, restore_terminal_control_intent, admit_control_intent, mark_control_intent_terminal, mark_control_terminal_ui_durable, ControlIntentLifecycle, ControlIntentReceipt, ControlIntentAdmission, ControlIntentTerminal, PendingModelReload, PendingModelSelection, PendingAgentSelection, PendingBehaviorSelection (plus 90 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/model_switch.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/model_switch.rs SHALL 维护 session actor lifecycle and notifications 的入口 PendingControlSettlement, settle_fatal, control_intent, terminal_result, settle, current_control_target, publish_control_projection, recover_missing_terminal_projection, control_terminal_message, publish_control_state_snapshot, selection_route_for_test, published_catalog_for_test, commit_model_change, apply_model_config_reload, apply_published_model_catalog, admit_model_catalog_reload, apply_pending_step_controls_if_idle, apply_user_model_selection (plus 43 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PendingControlSettlement, settle_fatal, control_intent, terminal_result, settle, current_control_target, publish_control_projection, recover_missing_terminal_projection, control_terminal_message, publish_control_state_snapshot, selection_route_for_test, published_catalog_for_test, commit_model_change, apply_model_config_reload, apply_published_model_catalog, admit_model_catalog_reload, apply_pending_step_controls_if_idle, apply_user_model_selection (plus 43 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** PendingControlSettlement, settle_fatal, control_intent, terminal_result, settle, current_control_target, publish_control_projection, recover_missing_terminal_projection, control_terminal_message, publish_control_state_snapshot, selection_route_for_test, published_catalog_for_test, commit_model_change, apply_model_config_reload, apply_published_model_catalog, admit_model_catalog_reload, apply_pending_step_controls_if_idle, apply_user_model_selection (plus 43 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/model_switch.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/notification_drain.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/notification_drain.rs SHALL 维护 session actor lifecycle and notifications 的入口 reconcile_notification_payloads, cleanup_notification_payloads_under_gate, consume_notifications_durably, consume_live_plan_handoff_for_next_step, dismiss_notifications_durably, receive_notification, maybe_start_running_task, drain_active_notifications, drain_active_notifications_excluding, maybe_drain_notifications, emit_session_idle_if_idle, read_notification_payloads, str, notification_owned_by_goal, notification_consumable_by, goal_notification_evidence, notification_autostarts, coalesce_running_task_checkpoints (plus 20 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** reconcile_notification_payloads, cleanup_notification_payloads_under_gate, consume_notifications_durably, consume_live_plan_handoff_for_next_step, dismiss_notifications_durably, receive_notification, maybe_start_running_task, drain_active_notifications, drain_active_notifications_excluding, maybe_drain_notifications, emit_session_idle_if_idle, read_notification_payloads, str, notification_owned_by_goal, notification_consumable_by, goal_notification_evidence, notification_autostarts, coalesce_running_task_checkpoints (plus 20 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** reconcile_notification_payloads, cleanup_notification_payloads_under_gate, consume_notifications_durably, consume_live_plan_handoff_for_next_step, dismiss_notifications_durably, receive_notification, maybe_start_running_task, drain_active_notifications, drain_active_notifications_excluding, maybe_drain_notifications, emit_session_idle_if_idle, read_notification_payloads, str, notification_owned_by_goal, notification_consumable_by, goal_notification_evidence, notification_autostarts, coalesce_running_task_checkpoints (plus 20 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** reconcile_notification_payloads, cleanup_notification_payloads_under_gate, consume_notifications_durably, consume_live_plan_handoff_for_next_step, dismiss_notifications_durably, receive_notification, maybe_start_running_task, drain_active_notifications, drain_active_notifications_excluding, maybe_drain_notifications, emit_session_idle_if_idle, read_notification_payloads, str, notification_owned_by_goal, notification_consumable_by, goal_notification_evidence, notification_autostarts, coalesce_running_task_checkpoints (plus 20 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/notification_drain.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/recap.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/recap.rs SHALL 维护 session actor lifecycle and notifications 的入口 side_question_retry_policy, should_retry_side_question, handle_side_question, handle_recap, RECAP_MIN_IDLE_MS, cancel_pending_recap_for_new_prompt, recap_was_cancelled, try_commit_recap, drop_recap_after_cancel, emit_recap_unavailable, handle_ai_suggest, handle_suggest_prompt, api, side_question_retries_overload_only, side_question_retry_wiring_caps_attempts_and_bounds_backoff。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** side_question_retry_policy, should_retry_side_question, handle_side_question, handle_recap, RECAP_MIN_IDLE_MS, cancel_pending_recap_for_new_prompt, recap_was_cancelled, try_commit_recap, drop_recap_after_cancel, emit_recap_unavailable, handle_ai_suggest, handle_suggest_prompt, api, side_question_retries_overload_only, side_question_retry_wiring_caps_attempts_and_bounds_backoff 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** side_question_retry_policy, should_retry_side_question, handle_side_question, handle_recap, RECAP_MIN_IDLE_MS, cancel_pending_recap_for_new_prompt, recap_was_cancelled, try_commit_recap, drop_recap_after_cancel, emit_recap_unavailable, handle_ai_suggest, handle_suggest_prompt, api, side_question_retries_overload_only, side_question_retry_wiring_caps_attempts_and_bounds_backoff 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/recap.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/reminders.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/reminders.rs SHALL 维护 session actor lifecycle and notifications 的入口 date_rollover_reminder, INTERRUPT_REMINDER, WORKFLOW_RESULT_SUMMARY_REMINDER_CAP, WORKFLOW_OBJECTIVE_REMINDER_CAP, workflow_completion_detail, push_workflow_launch_reminder, inject_workflow_status_reminder, workflow_handoff_notification, workflow_report_path, format_workflow_status_reminder, format_workflow_elapsed, workflow_report_markdown_link, format_workflow_completion_notification, format_running_task_checkpoint_notification, running_task_notification_owner, maybe_inject_date_rollover_reminder, maybe_inject_interrupt_reminder, push_system_reminder (plus 9 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** date_rollover_reminder, INTERRUPT_REMINDER, WORKFLOW_RESULT_SUMMARY_REMINDER_CAP, WORKFLOW_OBJECTIVE_REMINDER_CAP, workflow_completion_detail, push_workflow_launch_reminder, inject_workflow_status_reminder, workflow_handoff_notification, workflow_report_path, format_workflow_status_reminder, format_workflow_elapsed, workflow_report_markdown_link, format_workflow_completion_notification, format_running_task_checkpoint_notification, running_task_notification_owner, maybe_inject_date_rollover_reminder, maybe_inject_interrupt_reminder, push_system_reminder (plus 9 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/reminders.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/session_setup.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/session_setup.rs SHALL 维护 session actor lifecycle and notifications 的入口 slash_skills_for_resolve, to_acp_error, initialize_fresh_context_durably, build_prefix_background, ensure_prefix_ready, WAIT_TIMEOUT, reload_skills_from_disk, send_available_commands_update, wrap_skill_reminder, apply_skill_update_effects, flush_pending_system_reminders, record_api_request_time, handle_model_metadata_update, inject_deny_read_globs, build_session_info, usage_categories。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** slash_skills_for_resolve, to_acp_error, initialize_fresh_context_durably, build_prefix_background, ensure_prefix_ready, WAIT_TIMEOUT, reload_skills_from_disk, send_available_commands_update, wrap_skill_reminder, apply_skill_update_effects, flush_pending_system_reminders, record_api_request_time, handle_model_metadata_update, inject_deny_read_globs, build_session_info, usage_categories 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/session_setup.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/sideband.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/sideband.rs SHALL 维护 session actor lifecycle and notifications 的入口 SidebandSource, SidebandRunError, SidebandRun, fail_stop_sideband_admission, begin_sideband, begin_finalizer_sideband, begin_sideband_in_epoch, finish_evidence, usage_after_evidence_gate, response_usage, settle_goal_attempt, claim_goal_attempt, accept_goal_attempt_settlement, attempt_all_sources, attempt_selected, run_provider, set_background, provider_attempt_started (plus 38 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SidebandSource, SidebandRunError, SidebandRun, fail_stop_sideband_admission, begin_sideband, begin_finalizer_sideband, begin_sideband_in_epoch, finish_evidence, usage_after_evidence_gate, response_usage, settle_goal_attempt, claim_goal_attempt, accept_goal_attempt_settlement, attempt_all_sources, attempt_selected, run_provider, set_background, provider_attempt_started (plus 38 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** SidebandSource, SidebandRunError, SidebandRun, fail_stop_sideband_admission, begin_sideband, begin_finalizer_sideband, begin_sideband_in_epoch, finish_evidence, usage_after_evidence_gate, response_usage, settle_goal_attempt, claim_goal_attempt, accept_goal_attempt_settlement, attempt_all_sources, attempt_selected, run_provider, set_background, provider_attempt_started (plus 38 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/sideband.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/slash_exec.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/slash_exec.rs SHALL 维护 session actor lifecycle and notifications 的入口 HOST_COMMAND_INVOCATION, typed_command_notice_is_ui_only_and_preserves_tone, lifecycle_notice_is_ui_only_and_never_masquerades_as_agent_output, completed_goal_control_cancel_trigger, str, invocation, update, notices, seed_goal, explicit_goal_pause_has_one_command_result_but_runtime_stop_has_a_lifecycle_fact, queued_goal_result_keeps_its_invocation_on_apply_reject_clear_and_shutdown, disabled_memory_returns_one_actionable_notice_without_a_browser_result, memory_query_success_is_ephemeral_and_correlated, plugin_path_change_without_registry_reports_partial_success_once, unreadable_memory_returns_error_without_opening_browser, manual_compaction_error_has_one_backend_terminal_and_marks_its_rpc, GoalControlCancellation, queue_host_command (plus 9 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** HOST_COMMAND_INVOCATION, typed_command_notice_is_ui_only_and_preserves_tone, lifecycle_notice_is_ui_only_and_never_masquerades_as_agent_output, completed_goal_control_cancel_trigger, str, invocation, update, notices, seed_goal, explicit_goal_pause_has_one_command_result_but_runtime_stop_has_a_lifecycle_fact, queued_goal_result_keeps_its_invocation_on_apply_reject_clear_and_shutdown, disabled_memory_returns_one_actionable_notice_without_a_browser_result, memory_query_success_is_ephemeral_and_correlated, plugin_path_change_without_registry_reports_partial_success_once, unreadable_memory_returns_error_without_opening_browser, manual_compaction_error_has_one_backend_terminal_and_marks_its_rpc, GoalControlCancellation, queue_host_command (plus 9 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** HOST_COMMAND_INVOCATION, typed_command_notice_is_ui_only_and_preserves_tone, lifecycle_notice_is_ui_only_and_never_masquerades_as_agent_output, completed_goal_control_cancel_trigger, str, invocation, update, notices, seed_goal, explicit_goal_pause_has_one_command_result_but_runtime_stop_has_a_lifecycle_fact, queued_goal_result_keeps_its_invocation_on_apply_reject_clear_and_shutdown, disabled_memory_returns_one_actionable_notice_without_a_browser_result, memory_query_success_is_ephemeral_and_correlated, plugin_path_change_without_registry_reports_partial_success_once, unreadable_memory_returns_error_without_opening_browser, manual_compaction_error_has_one_backend_terminal_and_marks_its_rpc, GoalControlCancellation, queue_host_command (plus 9 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** HOST_COMMAND_INVOCATION, typed_command_notice_is_ui_only_and_preserves_tone, lifecycle_notice_is_ui_only_and_never_masquerades_as_agent_output, completed_goal_control_cancel_trigger, str, invocation, update, notices, seed_goal, explicit_goal_pause_has_one_command_result_but_runtime_stop_has_a_lifecycle_fact, queued_goal_result_keeps_its_invocation_on_apply_reject_clear_and_shutdown, disabled_memory_returns_one_actionable_notice_without_a_browser_result, memory_query_success_is_ephemeral_and_correlated, plugin_path_change_without_registry_reports_partial_success_once, unreadable_memory_returns_error_without_opening_browser, manual_compaction_error_has_one_backend_terminal_and_marks_its_rpc, GoalControlCancellation, queue_host_command (plus 9 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/slash_exec.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/stop_gate.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/stop_gate.rs SHALL 维护 session actor lifecycle and notifications 的入口 MAX_STOP_HOOK_CONTINUATIONS_PER_TURN, stop_entry_from_task, stop_entry_from_subagent, stop_cron_from_scheduled, STOP_FEEDBACK_TEXT_MAX, format_stop_feedback, list_active_subagents, stop_gate_work_snapshot, build_stop_payload, run_stop_gate, task_snapshot, task_snapshot_maps_to_stop_entry, subagent_summary_maps_to_stop_entry, format_stop_feedback_lists_blocks_then_appends_context, scheduled_task_maps_to_stop_cron, continuation_cap_records_policy_skipped_stop_occurrence_before_allowing_stop。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_STOP_HOOK_CONTINUATIONS_PER_TURN, stop_entry_from_task, stop_entry_from_subagent, stop_cron_from_scheduled, STOP_FEEDBACK_TEXT_MAX, format_stop_feedback, list_active_subagents, stop_gate_work_snapshot, build_stop_payload, run_stop_gate, task_snapshot, task_snapshot_maps_to_stop_entry, subagent_summary_maps_to_stop_entry, format_stop_feedback_lists_blocks_then_appends_context, scheduled_task_maps_to_stop_cron, continuation_cap_records_policy_skipped_stop_occurrence_before_allowing_stop 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** MAX_STOP_HOOK_CONTINUATIONS_PER_TURN, stop_entry_from_task, stop_entry_from_subagent, stop_cron_from_scheduled, STOP_FEEDBACK_TEXT_MAX, format_stop_feedback, list_active_subagents, stop_gate_work_snapshot, build_stop_payload, run_stop_gate, task_snapshot, task_snapshot_maps_to_stop_entry, subagent_summary_maps_to_stop_entry, format_stop_feedback_lists_blocks_then_appends_context, scheduled_task_maps_to_stop_cron, continuation_cap_records_policy_skipped_stop_occurrence_before_allowing_stop 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/stop_gate.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tasks_cancel.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/tasks_cancel.rs SHALL 维护 session actor lifecycle and notifications 的入口 TURN_USAGE_EPOCH, TURN_DURABLE_START_ACK, turn_usage_epoch_or, signal_durable_turn_start, TurnSubagentScopeGuard, new, drop, TurnActiveGuard, activate, AgentTask, new_prompt, abort, is_finished, TaskSlot, arm, take, cancel, is_running (plus 27 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** TURN_USAGE_EPOCH, TURN_DURABLE_START_ACK, turn_usage_epoch_or, signal_durable_turn_start, TurnSubagentScopeGuard, new, drop, TurnActiveGuard, activate, AgentTask, new_prompt, abort, is_finished, TaskSlot, arm, take, cancel, is_running (plus 27 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** TURN_USAGE_EPOCH, TURN_DURABLE_START_ACK, turn_usage_epoch_or, signal_durable_turn_start, TurnSubagentScopeGuard, new, drop, TurnActiveGuard, activate, AgentTask, new_prompt, abort, is_finished, TaskSlot, arm, take, cancel, is_running (plus 27 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/tasks_cancel.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/teardown.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/teardown.rs SHALL 维护 session actor lifecycle and notifications 的入口 cleanup_session_scratch, shutdown_sampler, shutdown_sampler_joins_drainer_and_breaks_session_cycle, stop_session_background_services, join_background_service, str, join_background_service_exact, shutdown_workflows, stop_permission_manager_and_drain_audit, final_session_persistence_flush, terminate_failed_timeline_writer。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** cleanup_session_scratch, shutdown_sampler, shutdown_sampler_joins_drainer_and_breaks_session_cycle, stop_session_background_services, join_background_service, str, join_background_service_exact, shutdown_workflows, stop_permission_manager_and_drain_audit, final_session_persistence_flush, terminate_failed_timeline_writer 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** cleanup_session_scratch, shutdown_sampler, shutdown_sampler_joins_drainer_and_breaks_session_cycle, stop_session_background_services, join_background_service, str, join_background_service_exact, shutdown_workflows, stop_permission_manager_and_drain_audit, final_session_persistence_flush, terminate_failed_timeline_writer 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** cleanup_session_scratch, shutdown_sampler, shutdown_sampler_joins_drainer_and_breaks_session_cycle, stop_session_background_services, join_background_service, str, join_background_service_exact, shutdown_workflows, stop_permission_manager_and_drain_audit, final_session_persistence_flush, terminate_failed_timeline_writer 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/teardown.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/types.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/types.rs SHALL 维护 session actor lifecycle and notifications 的入口 definitions, McpReminderMode, SamplerFailureRecovery, SamplerTurnOutcome, TurnOutcome, ControlDisposition, ToolLoop, LazinessAbortReason, ClassifierParseError, LazinessDecision, NoNudgeReason, StopGateDecision。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary、hook dispatch or hook source boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** definitions, McpReminderMode, SamplerFailureRecovery, SamplerTurnOutcome, TurnOutcome, ControlDisposition, ToolLoop, LazinessAbortReason, ClassifierParseError, LazinessDecision, NoNudgeReason, StopGateDecision 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/types.rs`。

### Requirement: Shell crates/codegen/shell/src/session/agent_rebuild.rs session timeline and state model contract

crates/codegen/shell/src/session/agent_rebuild.rs SHALL 维护 session timeline and state model 的入口 so, ResolvedToolParamsJson, AgentRebuildSpec, build_agent, build_agent_with_initial_overrides, build_agent_inner, test_rebuild_spec_default, model_entry, task_description, rebuild_projects_fresh_public_model_keys_into_task_description, rebuild_reuses_one_live_resource_domain_and_scheduler, workflow_rebuild_keeps_the_run_frozen_skill_body, every_child_rebuild_reapplies_immutable_owner_policy。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** so, ResolvedToolParamsJson, AgentRebuildSpec, build_agent, build_agent_with_initial_overrides, build_agent_inner, test_rebuild_spec_default, model_entry, task_description, rebuild_projects_fresh_public_model_keys_into_task_description, rebuild_reuses_one_live_resource_domain_and_scheduler, workflow_rebuild_keeps_the_run_frozen_skill_body, every_child_rebuild_reapplies_immutable_owner_policy 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** so, ResolvedToolParamsJson, AgentRebuildSpec, build_agent, build_agent_with_initial_overrides, build_agent_inner, test_rebuild_spec_default, model_entry, task_description, rebuild_projects_fresh_public_model_keys_into_task_description, rebuild_reuses_one_live_resource_domain_and_scheduler, workflow_rebuild_keeps_the_run_frozen_skill_body, every_child_rebuild_reapplies_immutable_owner_policy 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** so, ResolvedToolParamsJson, AgentRebuildSpec, build_agent, build_agent_with_initial_overrides, build_agent_inner, test_rebuild_spec_default, model_entry, task_description, rebuild_projects_fresh_public_model_keys_into_task_description, rebuild_reuses_one_live_resource_domain_and_scheduler, workflow_rebuild_keeps_the_run_frozen_skill_body, every_child_rebuild_reapplies_immutable_owner_policy 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/agent_rebuild.rs`。

### Requirement: Shell crates/codegen/shell/src/session/announcement_state.rs session timeline and state model contract

crates/codegen/shell/src/session/announcement_state.rs SHALL 维护 session timeline and state model 的入口 TIMELINE_ANNOUNCEMENT_VERSION, TIMELINE_ANNOUNCEMENT_SCOPE, TIMELINE_ANNOUNCEMENT_NAME, TimelineAnnouncementSnapshot, AnnouncementState, timeline_kind, latest_from_timeline, alias, McpServerFingerprint, to_persisted_fingerprints, from_persisted_fingerprints, timeline_round_trip_uses_latest_snapshot, unsupported_timeline_snapshot_version_fails_closed, fingerprint_conversion_round_trip。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、MCP integration boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** TIMELINE_ANNOUNCEMENT_VERSION, TIMELINE_ANNOUNCEMENT_SCOPE, TIMELINE_ANNOUNCEMENT_NAME, TimelineAnnouncementSnapshot, AnnouncementState, timeline_kind, latest_from_timeline, alias, McpServerFingerprint, to_persisted_fingerprints, from_persisted_fingerprints, timeline_round_trip_uses_latest_snapshot, unsupported_timeline_snapshot_version_fails_closed, fingerprint_conversion_round_trip 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/announcement_state.rs`。

### Requirement: Shell crates/codegen/shell/src/session/behavior.rs session timeline and state model contract

crates/codegen/shell/src/session/behavior.rs SHALL 维护 session timeline and state model 的入口 BehaviorChangeOutcome, BehaviorRequestAuthority, BehaviorForeground, BehaviorSwitchFacts, default, BehaviorEffect, BehaviorDecision, response_meta, PlanPhase, PlanDecisionHandoff, as, BehaviorState, PlanRuntime, BehaviorCoordinator, PendingBehaviorSwitch, BehaviorSnapshot, normal, selected (plus 83 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** BehaviorChangeOutcome, BehaviorRequestAuthority, BehaviorForeground, BehaviorSwitchFacts, default, BehaviorEffect, BehaviorDecision, response_meta, PlanPhase, PlanDecisionHandoff, as, BehaviorState, PlanRuntime, BehaviorCoordinator, PendingBehaviorSwitch, BehaviorSnapshot, normal, selected (plus 83 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/behavior.rs`。

### Requirement: Shell crates/codegen/shell/src/session/commands.rs session timeline and state model contract

crates/codegen/shell/src/session/commands.rs SHALL 维护 session timeline and state model 的入口 and, HostCommandInvocation, ControlIntent, CONTROL_INTENT_META_KEY, EFFORT_PATCH_META_KEY, effort_patch_from_meta, SessionEffortAuthority, from_meta, validate, str, insert_meta, DesiredStateOutcome, CONTROL_TERMINAL_PUBLISHED_KEY, mark_control_terminal_published, control_terminal_was_published, CancellationContext, SideQuestionError, PromptCompletionKind (plus 8 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** and, HostCommandInvocation, ControlIntent, CONTROL_INTENT_META_KEY, EFFORT_PATCH_META_KEY, effort_patch_from_meta, SessionEffortAuthority, from_meta, validate, str, insert_meta, DesiredStateOutcome, CONTROL_TERMINAL_PUBLISHED_KEY, mark_control_terminal_published, control_terminal_was_published, CancellationContext, SideQuestionError, PromptCompletionKind (plus 8 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** and, HostCommandInvocation, ControlIntent, CONTROL_INTENT_META_KEY, EFFORT_PATCH_META_KEY, effort_patch_from_meta, SessionEffortAuthority, from_meta, validate, str, insert_meta, DesiredStateOutcome, CONTROL_TERMINAL_PUBLISHED_KEY, mark_control_terminal_published, control_terminal_was_published, CancellationContext, SideQuestionError, PromptCompletionKind (plus 8 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/commands.rs`。

### Requirement: Shell crates/codegen/shell/src/session/compaction_config.rs session timeline and state model contract

crates/codegen/shell/src/session/compaction_config.rs SHALL 维护 session timeline and state model 的入口 COMPACTION_IDLE, COMPACTION_MANUAL, COMPACTION_AUTO, CompactionOwner, CompactionLease, CompactionLeaseGuard, drop, try_enter, is_in_flight, SUPPRESS_NONE, SUPPRESS_TURN, SUPPRESS_STICKY, SUPPRESS_UNTIL_SUCCESS, SUPPRESS_AUTH, PreviousModelInfo, CompactCancelGate, CompactCancelScope, enter (plus 13 additional private symbols)。实现显示该边界包含 async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Async lifecycle
- **WHEN** COMPACTION_IDLE, COMPACTION_MANUAL, COMPACTION_AUTO, CompactionOwner, CompactionLease, CompactionLeaseGuard, drop, try_enter, is_in_flight, SUPPRESS_NONE, SUPPRESS_TURN, SUPPRESS_STICKY, SUPPRESS_UNTIL_SUCCESS, SUPPRESS_AUTH, PreviousModelInfo, CompactCancelGate, CompactCancelScope, enter (plus 13 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/compaction_config.rs`。

### Requirement: Shell crates/codegen/shell/src/session/control.rs session timeline and state model contract

crates/codegen/shell/src/session/control.rs SHALL 维护 session timeline and state model 的入口 SESSION_CONTROL_ARCHITECTURE_VERSION, DurableControlReceipt, SessionControlSnapshot, new, with_applied_control, architecture_is_current, decode_persisted, retired_context_layers, validate, timeline_kind, timeline_kind_with_model_context, timeline_kind_with_model_context_item, timeline_kind_with_model_context_items, timeline_kind_inner, latest_from_timeline, durable_receipts_from_timeline, agent_role_transition_context, goal (plus 15 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SESSION_CONTROL_ARCHITECTURE_VERSION, DurableControlReceipt, SessionControlSnapshot, new, with_applied_control, architecture_is_current, decode_persisted, retired_context_layers, validate, timeline_kind, timeline_kind_with_model_context, timeline_kind_with_model_context_item, timeline_kind_with_model_context_items, timeline_kind_inner, latest_from_timeline, durable_receipts_from_timeline, agent_role_transition_context, goal (plus 15 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/control.rs`。

### Requirement: Shell crates/codegen/shell/src/session/diagnostics.rs session timeline and state model contract

crates/codegen/shell/src/session/diagnostics.rs SHALL 维护 session timeline and state model 的入口 permission_decision_source, str, emit_mcp_connection_span, skill_source_label, format_hook_name, format_hook_source, HookRegInfo, from_spec, SessionHarnessMetrics, into_event。实现显示该边界包含 explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary、hook dispatch or hook source boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** permission_decision_source, str, emit_mcp_connection_span, skill_source_label, format_hook_name, format_hook_source, HookRegInfo, from_spec, SessionHarnessMetrics, into_event 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/diagnostics.rs`。

### Requirement: Shell crates/codegen/shell/src/session/event_tracker.rs session timeline and state model contract

crates/codegen/shell/src/session/event_tracker.rs SHALL 维护 session timeline and state model 的入口 ActiveTool, ActiveRequest, deliberately, EventTracker, EventTrackerInner, Target, deref, fmt, new, current_turn, wait_for_causal_idle, current_goal_id, emit, start_turn, start_turn_transaction, record_observation, start_step, end_step (plus 39 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ActiveTool, ActiveRequest, deliberately, EventTracker, EventTrackerInner, Target, deref, fmt, new, current_turn, wait_for_causal_idle, current_goal_id, emit, start_turn, start_turn_transaction, record_observation, start_step, end_step (plus 39 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** ActiveTool, ActiveRequest, deliberately, EventTracker, EventTrackerInner, Target, deref, fmt, new, current_turn, wait_for_causal_idle, current_goal_id, emit, start_turn, start_turn_transaction, record_observation, start_step, end_step (plus 39 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/event_tracker.rs`。

### Requirement: Shell crates/codegen/shell/src/session/event_types.rs session timeline and state model contract

crates/codegen/shell/src/session/event_types.rs SHALL 维护 session timeline and state model 的入口 EVENT_SCHEMA_VERSION, Event, str, InterjectionSource, RedirectKind, McpErrorCategory, ToolOutcome, from, SessionRelationship, TurnOutcomeLabel, PermissionDecision, user_identity, interjected_event_serializes_tag_source_and_count, redirect_kind_serializes_snake_case, turn_started_redirect_kind_present_when_set_omitted_when_none。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** EVENT_SCHEMA_VERSION, Event, str, InterjectionSource, RedirectKind, McpErrorCategory, ToolOutcome, from, SessionRelationship, TurnOutcomeLabel, PermissionDecision, user_identity, interjected_event_serializes_tag_source_and_count, redirect_kind_serializes_snake_case, turn_started_redirect_kind_present_when_set_omitted_when_none 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/event_types.rs`。

### Requirement: Shell crates/codegen/shell/src/session/events.rs session timeline and state model contract

crates/codegen/shell/src/session/events.rs SHALL 维护 session timeline and state model 的入口 LAZINESS_STALLED_NARRATION, LAZINESS_STALLED_PERMISSION_ASKING, LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT, LAZINESS_STALLED_FALSE_COMPLETION, LAZINESS_NOT_STALLED_COMPLETE, LAZINESS_NOT_STALLED_WAITING_BG, LAZINESS_NOT_STALLED_WAITING_USER, LAZINESS_ABORT_USER_INPUT, LAZINESS_ABORT_MODEL_SWITCH, LAZINESS_ABORT_TIMEOUT, LAZINESS_ABORT_CLASSIFIER_ERROR, breaking, _, LazinessCategory, as_const_str, str, is_stalled, all (plus 15 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、hook dispatch or hook source boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LAZINESS_STALLED_NARRATION, LAZINESS_STALLED_PERMISSION_ASKING, LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT, LAZINESS_STALLED_FALSE_COMPLETION, LAZINESS_NOT_STALLED_COMPLETE, LAZINESS_NOT_STALLED_WAITING_BG, LAZINESS_NOT_STALLED_WAITING_USER, LAZINESS_ABORT_USER_INPUT, LAZINESS_ABORT_MODEL_SWITCH, LAZINESS_ABORT_TIMEOUT, LAZINESS_ABORT_CLASSIFIER_ERROR, breaking, _, LazinessCategory, as_const_str, str, is_stalled, all (plus 15 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/events.rs`。

### Requirement: Shell crates/codegen/shell/src/session/file_system.rs session timeline and state model contract

crates/codegen/shell/src/session/file_system.rs SHALL 维护 session timeline and state model 的入口 FsListParams, FsListNode, FsListData, FsExistsData, FsReadFileData, list, exists, read_file_ranged, write_file, delete_file, params, list_paginates_with_stable_offset, read_file_ranged_is_binary_safe_and_keeps_full_size, read_file_ranged_clamps_to_server_cap。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** FsListParams, FsListNode, FsListData, FsExistsData, FsReadFileData, list, exists, read_file_ranged, write_file, delete_file, params, list_paginates_with_stable_offset, read_file_ranged_is_binary_safe_and_keeps_full_size, read_file_ranged_clamps_to_server_cap 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** FsListParams, FsListNode, FsListData, FsExistsData, FsReadFileData, list, exists, read_file_ranged, write_file, delete_file, params, list_paginates_with_stable_offset, read_file_ranged_is_binary_safe_and_keeps_full_size, read_file_ranged_clamps_to_server_cap 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/file_system.rs`。

### Requirement: Shell crates/codegen/shell/src/session/fork.rs session timeline and state model contract

crates/codegen/shell/src/session/fork.rs SHALL 维护 session timeline and state model 的入口 FORK_LOG, ForkSessionRequest, ForkSessionResponse, generate_fork_session_id, fork_session, test_generate_fork_session_id_format, test_generate_fork_session_id_uniqueness, test_generate_fork_session_id_constant_length, test_fork_session_request_serialization, test_fork_session_request_without_optional_fields, test_fork_session_response_serialization, test_fork_session_response_without_model_override。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** FORK_LOG, ForkSessionRequest, ForkSessionResponse, generate_fork_session_id, fork_session, test_generate_fork_session_id_format, test_generate_fork_session_id_uniqueness, test_generate_fork_session_id_constant_length, test_fork_session_request_serialization, test_fork_session_request_without_optional_fields, test_fork_session_response_serialization, test_fork_session_response_without_model_override 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** FORK_LOG, ForkSessionRequest, ForkSessionResponse, generate_fork_session_id, fork_session, test_generate_fork_session_id_format, test_generate_fork_session_id_uniqueness, test_generate_fork_session_id_constant_length, test_fork_session_request_serialization, test_fork_session_request_without_optional_fields, test_fork_session_response_serialization, test_fork_session_response_without_model_override 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/fork.rs`。

### Requirement: Shell crates/codegen/shell/src/session/fs_watch.rs session timeline and state model contract

crates/codegen/shell/src/session/fs_watch.rs SHALL 维护 session timeline and state model 的入口 is_under_hidden_dir, forward_to_hunk_tracker, git_head_dedup_key, fs_event_to_codebase_graph_event, fs_event_to_delta, GIT_DIFF_REBUILD_THRESHOLD, parse_diff_name_status_line, refresh_codebase_graph_after_head_change, FsWatchCapabilities, needs_watcher, none, resolve, CapabilityInputs, FsWatchDeps, from_session, ClientNotify, on_change, send_initial_file_index (plus 73 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** is_under_hidden_dir, forward_to_hunk_tracker, git_head_dedup_key, fs_event_to_codebase_graph_event, fs_event_to_delta, GIT_DIFF_REBUILD_THRESHOLD, parse_diff_name_status_line, refresh_codebase_graph_after_head_change, FsWatchCapabilities, needs_watcher, none, resolve, CapabilityInputs, FsWatchDeps, from_session, ClientNotify, on_change, send_initial_file_index (plus 73 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** is_under_hidden_dir, forward_to_hunk_tracker, git_head_dedup_key, fs_event_to_codebase_graph_event, fs_event_to_delta, GIT_DIFF_REBUILD_THRESHOLD, parse_diff_name_status_line, refresh_codebase_graph_after_head_change, FsWatchCapabilities, needs_watcher, none, resolve, CapabilityInputs, FsWatchDeps, from_session, ClientNotify, on_change, send_initial_file_index (plus 73 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/fs_watch.rs`。

### Requirement: Shell crates/codegen/shell/src/session/goal_tracker.rs session timeline and state model contract

crates/codegen/shell/src/session/goal_tracker.rs SHALL 维护 session timeline and state model 的入口 GOAL_ARCHITECTURE_VERSION, REQUIRED_CONSECUTIVE_BLOCKED_TURNS, GoalBlockedAudit, GoalStatus, continues_automatically, can_restart, GoalPauseReason, default_message, str, GoalState, GoalTracker, usage_blocks_budget, default, new, from_snapshot, validate_snapshot, restore_runtime_snapshot, snapshot (plus 42 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** GOAL_ARCHITECTURE_VERSION, REQUIRED_CONSECUTIVE_BLOCKED_TURNS, GoalBlockedAudit, GoalStatus, continues_automatically, can_restart, GoalPauseReason, default_message, str, GoalState, GoalTracker, usage_blocks_budget, default, new, from_snapshot, validate_snapshot, restore_runtime_snapshot, snapshot (plus 42 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/goal_tracker.rs`。

### Requirement: Shell crates/codegen/shell/src/session/handle.rs session timeline and state model contract

crates/codegen/shell/src/session/handle.rs SHALL 维护 session timeline and state model 的入口 SessionModelRouteSnapshot, SessionModelRoute, new, snapshot, replace, SessionAgentProfileSnapshot, SessionAgentProfile, name, subagent_filter, SessionLiveState, SessionHandle, SessionLifecycleOwner, drop, get_model_metadata, background_foreground_command, kill_background_task, delete_scheduled_task, unload_if_idle (plus 18 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SessionModelRouteSnapshot, SessionModelRoute, new, snapshot, replace, SessionAgentProfileSnapshot, SessionAgentProfile, name, subagent_filter, SessionLiveState, SessionHandle, SessionLifecycleOwner, drop, get_model_metadata, background_foreground_command, kill_background_task, delete_scheduled_task, unload_if_idle (plus 18 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** SessionModelRouteSnapshot, SessionModelRoute, new, snapshot, replace, SessionAgentProfileSnapshot, SessionAgentProfile, name, subagent_filter, SessionLiveState, SessionHandle, SessionLifecycleOwner, drop, get_model_metadata, background_foreground_command, kill_background_task, delete_scheduled_task, unload_if_idle (plus 18 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/handle.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/compaction_context.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/compaction_context.rs SHALL 维护 session timeline and state model 的入口 and, McpToolNames, SubagentToolNames, to_system_reminder_sync, to_system_reminder, to_system_reminder_inner, ctx_with_running_subagents, system_reminder_includes_subagent_section_when_tool_names_present, system_reminder_includes_mcp_server_section, running_task_ids_render_verbatim, system_reminder_skips_subagent_section_when_tool_names_none, ctx_with_todos, todo, system_reminder_includes_active_todos, system_reminder_places_todos_below_background_tasks, system_reminder_omits_todos_when_none_active。实现显示该边界包含 platform or feature-gated branches、session/timeline state projection、MCP integration boundary、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/compaction_context.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/memory_context.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/memory_context.rs SHALL 维护 session timeline and state model 的入口 SNIPPET_MAX_CHARS, conversation_has_memory_context, format_memory_reminder, is_greeting, GREETINGS, test_format_empty, test_format_single_result, test_format_preserves_newlines, test_format_truncates_long_snippets, test_format_multiple_results, sample_result, test_detects_persisted_typed_memory_item, test_no_block_without_typed_memory_item, test_no_block_for_an_ordinary_user_message, test_no_block_for_empty_conversation, test_staleness_shown_for_old_session_result, test_no_staleness_for_workspace_result, test_greeting_detection (plus 3 additional private symbols)。实现显示该边界包含 platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/memory_context.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/memory_flush.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/memory_flush.rs SHALL 维护 session timeline and state model 的入口 LOG, should_flush, FLUSH_SYSTEM_PROMPT, FLUSH_DELTA_SYSTEM_PROMPT, FlushResult, process_flush_response, is_duplicate, SEMANTIC_DEDUP_SIMILARITY_THRESHOLD, MAX_L2_DISTANCE, SEMANTIC_DEDUP_KNN_LIMIT, is_semantically_duplicate, select_flush_window, default_flush_config, test_should_flush_disabled, test_should_flush_already_flushed_this_cycle, test_should_flush_below_threshold, test_should_flush_at_threshold, test_should_flush_above_threshold (plus 19 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LOG, should_flush, FLUSH_SYSTEM_PROMPT, FLUSH_DELTA_SYSTEM_PROMPT, FlushResult, process_flush_response, is_duplicate, SEMANTIC_DEDUP_SIMILARITY_THRESHOLD, MAX_L2_DISTANCE, SEMANTIC_DEDUP_KNN_LIMIT, is_semantically_duplicate, select_flush_window, default_flush_config, test_should_flush_disabled, test_should_flush_already_flushed_this_cycle, test_should_flush_below_threshold, test_should_flush_at_threshold, test_should_flush_above_threshold (plus 19 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LOG, should_flush, FLUSH_SYSTEM_PROMPT, FLUSH_DELTA_SYSTEM_PROMPT, FlushResult, process_flush_response, is_duplicate, SEMANTIC_DEDUP_SIMILARITY_THRESHOLD, MAX_L2_DISTANCE, SEMANTIC_DEDUP_KNN_LIMIT, is_semantically_duplicate, select_flush_window, default_flush_config, test_should_flush_disabled, test_should_flush_already_flushed_this_cycle, test_should_flush_below_threshold, test_should_flush_at_threshold, test_should_flush_above_threshold (plus 19 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/memory_flush.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/mod.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/mod.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/helpers/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/session_compact.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/session_compact.rs SHALL 维护 session timeline and state model 的入口 CompactFailure, COMPACT_CANCELLED_MSG, cancelled_error, CompactUsageObserver, CompactUsageMeter, new, observe, drop, classify_sampling_error, strings, classify_response_event_error, build_compaction_request_surface, build_compaction_prompt, CompactOutput, directly, CompactionOutcome, as_str, str (plus 41 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CompactFailure, COMPACT_CANCELLED_MSG, cancelled_error, CompactUsageObserver, CompactUsageMeter, new, observe, drop, classify_sampling_error, strings, classify_response_event_error, build_compaction_request_surface, build_compaction_prompt, CompactOutput, directly, CompactionOutcome, as_str, str (plus 41 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** CompactFailure, COMPACT_CANCELLED_MSG, cancelled_error, CompactUsageObserver, CompactUsageMeter, new, observe, drop, classify_sampling_error, strings, classify_response_event_error, build_compaction_request_surface, build_compaction_prompt, CompactOutput, directly, CompactionOutcome, as_str, str (plus 41 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/session_compact.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/session_recap.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/session_recap.rs SHALL 维护 session timeline and state model 的入口 RECAP_MAX_CHARS, recap_instruction, build_recap_items, RECAP_CONTEXT_WINDOW_CAP, RECAP_BUDGET_THRESHOLD_PERCENT, RECAP_BUDGET_HEADROOM_TOKENS, budget_recap_items, pop_trailing_tool_run, MIN_TURNS_FOR_AUTO_RECAP, main_turn_count, recap_gate, str, RECAP_AUTO_RAW_DISPLAY_MAX, should_suppress_auto_recap_display, clean_recap_text, clean_collapses_whitespace_and_newlines, clean_strips_leading_label, clean_strips_wrapping_quotes (plus 34 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、hook dispatch or hook source boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** RECAP_MAX_CHARS, recap_instruction, build_recap_items, RECAP_CONTEXT_WINDOW_CAP, RECAP_BUDGET_THRESHOLD_PERCENT, RECAP_BUDGET_HEADROOM_TOKENS, budget_recap_items, pop_trailing_tool_run, MIN_TURNS_FOR_AUTO_RECAP, main_turn_count, recap_gate, str, RECAP_AUTO_RAW_DISPLAY_MAX, should_suppress_auto_recap_display, clean_recap_text, clean_collapses_whitespace_and_newlines, clean_strips_leading_label, clean_strips_wrapping_quotes (plus 34 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/session_recap.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/text.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/text.rs SHALL 维护 session timeline and state model 的入口 floor_char_boundary。实现显示该边界包含 session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 floor_char_boundary 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/helpers/text.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/tool_input_parsing.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/tool_input_parsing.rs SHALL 维护 session timeline and state model 的入口 try_extract_concatenated_json_objects, normalize_empty_arguments, test_extract_objects, test_no_extract_for_valid_single_object, test_no_extract_for_valid_object_with_braces_in_value, test_no_extract_for_array, test_no_extract_for_empty_or_non_json, test_extract_with_nested_braces, test_extract_with_whitespace_between_objects, test_extract_real_world_20_files, test_no_extract_for_truncated_json, normalize_and_parse, empty_string_becomes_empty_object, whitespace_only_becomes_empty_object, valid_json_unchanged, empty_object_string_unchanged, invalid_json_falls_back_to_raw, complex_args_with_arrays_unchanged (plus 2 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/tool_input_parsing.rs`。

### Requirement: Shell crates/codegen/shell/src/session/image_normalize.rs session timeline and state model contract

crates/codegen/shell/src/session/image_normalize.rs SHALL 维护 session timeline and state model 的入口 MAX_IMAGE_BYTES, LIMIT_LABEL, MAX_ENCODE_PIXELS, MAX_ENCODE_SIDE_PX, MIN_ENCODE_SIDE_PX, DOWNSCALE_FILTER, JPEG_QUALITY_STEPS, MAX_DECODE_PIXELS, MAX_LOAD_ICO_DECODE_PIXELS, MIN_VISION_SIDE_PX, MIN_VISION_TOTAL_PX, MAX_VISION_TOTAL_PX, NORMALIZE_PARAMS, ImageCompressionInfo, reason_label, display, NormalizeResult, normalize_images (plus 67 additional private symbols)。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_IMAGE_BYTES, LIMIT_LABEL, MAX_ENCODE_PIXELS, MAX_ENCODE_SIDE_PX, MIN_ENCODE_SIDE_PX, DOWNSCALE_FILTER, JPEG_QUALITY_STEPS, MAX_DECODE_PIXELS, MAX_LOAD_ICO_DECODE_PIXELS, MIN_VISION_SIDE_PX, MIN_VISION_TOTAL_PX, MAX_VISION_TOTAL_PX, NORMALIZE_PARAMS, ImageCompressionInfo, reason_label, display, NormalizeResult, normalize_images (plus 67 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/image_normalize.rs`。

### Requirement: Shell crates/codegen/shell/src/session/inference_metrics.rs session timeline and state model contract

crates/codegen/shell/src/session/inference_metrics.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/inference_metrics.rs`。

### Requirement: Shell crates/codegen/shell/src/session/listing.rs session timeline and state model contract

crates/codegen/shell/src/session/listing.rs SHALL 维护 session timeline and state model 的入口 over_fetch, SessionListing, fetch_sessions, fetch_local_summaries, filter_summaries_by_repo, list_from_summaries, summary_to_session, effective_sort_time, dedup_empty_sessions, normalize_cwd, summary, listing_filters_and_keeps_only_newest_empty_session_per_cwd。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、session/timeline state projection、sandbox/trust boundary、git/worktree context、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/listing.rs`。

### Requirement: Shell crates/codegen/shell/src/session/memory/hooks.rs session timeline and state model contract

crates/codegen/shell/src/session/memory/hooks.rs SHALL 维护 session timeline and state model 的入口 MIN_USER_MESSAGES, MIN_TOTAL_QUERY_BYTES, SessionEndResult, on_session_end, generate_metadata_summary, make_user, make_synthetic_prefix_with_query, make_metadata_only, make_assistant, test_storage, test_on_session_end_skips_short_sessions, test_on_session_end_skips_brief_sessions, test_on_session_end_writes_summary, test_on_session_end_summary_has_structure, test_on_session_end_empty_conversation, test_generate_metadata_summary_format, test_on_session_end_save_on_end_false_skips, test_synthetic_prefix_alone_does_not_count_as_real_message (plus 6 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection、hook dispatch or hook source boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MIN_USER_MESSAGES, MIN_TOTAL_QUERY_BYTES, SessionEndResult, on_session_end, generate_metadata_summary, make_user, make_synthetic_prefix_with_query, make_metadata_only, make_assistant, test_storage, test_on_session_end_skips_short_sessions, test_on_session_end_skips_brief_sessions, test_on_session_end_writes_summary, test_on_session_end_summary_has_structure, test_on_session_end_empty_conversation, test_generate_metadata_summary_format, test_on_session_end_save_on_end_false_skips, test_synthetic_prefix_alone_does_not_count_as_real_message (plus 6 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MIN_USER_MESSAGES, MIN_TOTAL_QUERY_BYTES, SessionEndResult, on_session_end, generate_metadata_summary, make_user, make_synthetic_prefix_with_query, make_metadata_only, make_assistant, test_storage, test_on_session_end_skips_short_sessions, test_on_session_end_skips_brief_sessions, test_on_session_end_writes_summary, test_on_session_end_summary_has_structure, test_on_session_end_empty_conversation, test_generate_metadata_summary_format, test_on_session_end_save_on_end_false_skips, test_synthetic_prefix_alone_does_not_count_as_real_message (plus 6 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/memory/hooks.rs`。

### Requirement: Shell crates/codegen/shell/src/session/memory/mod.rs session timeline and state model contract

crates/codegen/shell/src/session/memory/mod.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 session/timeline state projection、hook dispatch or hook source boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/memory/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/memory_state.rs session timeline and state model contract

crates/codegen/shell/src/session/memory_state.rs SHALL 维护 session timeline and state model 的入口 SessionMemory, is_enabled, storage, try_acquire_flush_lock, release_flush_lock, try_begin_dream, finish_dream, record_flush_result, record_dream_result, record_dream_neutral, open_index, reindex_and_embed, delete_paths_from_index, diagnostics_snapshot, MemoryDiagnostic。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SessionMemory, is_enabled, storage, try_acquire_flush_lock, release_flush_lock, try_begin_dream, finish_dream, record_flush_result, record_dream_result, record_dream_neutral, open_index, reindex_and_embed, delete_paths_from_index, diagnostics_snapshot, MemoryDiagnostic 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SessionMemory, is_enabled, storage, try_acquire_flush_lock, release_flush_lock, try_begin_dream, finish_dream, record_flush_result, record_dream_result, record_dream_neutral, open_index, reindex_and_embed, delete_paths_from_index, diagnostics_snapshot, MemoryDiagnostic 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/memory_state.rs`。

### Requirement: Shell crates/codegen/shell/src/session/mod.rs session timeline and state model contract

crates/codegen/shell/src/session/mod.rs SHALL 维护 session timeline and state model 的入口 image_blocks, PromptOrigin, TurnKind, turn_identity, wire_name, str, is_synthetic, is_goal_internal, is_preemptible_wake, hide_user_echo_from_scrollback, completion_id, origin_and_kind_are_structured_independently_of_prompt_id, only_replaceable_wakes_are_preemptible, ClientFsMode, ClientFsConfig。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、MCP integration boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** image_blocks, PromptOrigin, TurnKind, turn_identity, wire_name, str, is_synthetic, is_goal_internal, is_preemptible_wake, hide_user_echo_from_scrollback, completion_id, origin_and_kind_are_structured_independently_of_prompt_id, only_replaceable_wakes_are_preemptible, ClientFsMode, ClientFsConfig 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/normalize_cache.rs session timeline and state model contract

crates/codegen/shell/src/session/normalize_cache.rs SHALL 维护 session timeline and state model 的入口 CACHE_MAX_BYTES, CACHE_TTL, CACHE_TTI, NormalizedEntry, NormalizeError, cache_key, NormalizeCache, global, Self, INSTANCE, with_capacity, set_enabled, is_enabled, get_or_try_insert_with, get_for_tests, weigh_entry, run_blocking, enabled_cache (plus 14 additional private symbols)。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CACHE_MAX_BYTES, CACHE_TTL, CACHE_TTI, NormalizedEntry, NormalizeError, cache_key, NormalizeCache, global, Self, INSTANCE, with_capacity, set_enabled, is_enabled, get_or_try_insert_with, get_for_tests, weigh_entry, run_blocking, enabled_cache (plus 14 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** CACHE_MAX_BYTES, CACHE_TTL, CACHE_TTI, NormalizedEntry, NormalizeError, cache_key, NormalizeCache, global, Self, INSTANCE, with_capacity, set_enabled, is_enabled, get_or_try_insert_with, get_for_tests, weigh_entry, run_blocking, enabled_cache (plus 14 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/normalize_cache.rs`。

### Requirement: Shell crates/codegen/shell/src/session/notification_inbox.rs session timeline and state model contract

crates/codegen/shell/src/session/notification_inbox.rs SHALL 维护 session timeline and state model 的入口 ARTIFACT_DIRECTORY, ORPHAN_SWEEP_BATCH_SIZE, write_payload, read_payload, remove_payload, visit_payload_hash_batches, remove_payload_hashes, payload_round_trip_is_content_addressed, orphan_cleanup_keeps_only_timeline_referenced_payloads, payload_hash_stream_is_bounded_and_unknown_files_do_not_starve_orphans, payload_hash_stream_stops_inside_an_unknown_file_tail_when_cancelled。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ARTIFACT_DIRECTORY, ORPHAN_SWEEP_BATCH_SIZE, write_payload, read_payload, remove_payload, visit_payload_hash_batches, remove_payload_hashes, payload_round_trip_is_content_addressed, orphan_cleanup_keeps_only_timeline_referenced_payloads, payload_hash_stream_is_bounded_and_unknown_files_do_not_starve_orphans, payload_hash_stream_stops_inside_an_unknown_file_tail_when_cancelled 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** ARTIFACT_DIRECTORY, ORPHAN_SWEEP_BATCH_SIZE, write_payload, read_payload, remove_payload, visit_payload_hash_batches, remove_payload_hashes, payload_round_trip_is_content_addressed, orphan_cleanup_keeps_only_timeline_referenced_payloads, payload_hash_stream_is_bounded_and_unknown_files_do_not_starve_orphans, payload_hash_stream_stops_inside_an_unknown_file_tail_when_cancelled 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/notification_inbox.rs`。

### Requirement: Shell crates/codegen/shell/src/session/pending_interaction.rs session timeline and state model contract

crates/codegen/shell/src/session/pending_interaction.rs SHALL 维护 session timeline and state model 的入口 PendingInteractions, PendingKind, has_parked_plan_approval, broadcast, PendingInteractionGuard, new, drop, new_registry, guard_inserts_then_removes, has_parked_plan_approval_only_counts_plan_approval, has_parked_plan_approval_recovers_poisoned_lock。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PendingInteractions, PendingKind, has_parked_plan_approval, broadcast, PendingInteractionGuard, new, drop, new_registry, guard_inserts_then_removes, has_parked_plan_approval_only_counts_plan_approval, has_parked_plan_approval_recovers_poisoned_lock 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** PendingInteractions, PendingKind, has_parked_plan_approval, broadcast, PendingInteractionGuard, new, drop, new_registry, guard_inserts_then_removes, has_parked_plan_approval_only_counts_plan_approval, has_parked_plan_approval_recovers_poisoned_lock 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/pending_interaction.rs`。

### Requirement: Shell crates/codegen/shell/src/session/sampling_evidence.rs session timeline and state model contract

crates/codegen/shell/src/session/sampling_evidence.rs SHALL 维护 session timeline and state model 的入口 CHUNK_BYTES, DIRECTORY, SCOPE, LIMIT, Record, BodyChunk, write_body, sink, verify, referenced_hashes, decode_record, append_only_request_bodies_reuse_prefix_chunks。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CHUNK_BYTES, DIRECTORY, SCOPE, LIMIT, Record, BodyChunk, write_body, sink, verify, referenced_hashes, decode_record, append_only_request_bodies_reuse_prefix_chunks 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** CHUNK_BYTES, DIRECTORY, SCOPE, LIMIT, Record, BodyChunk, write_body, sink, verify, referenced_hashes, decode_record, append_only_request_bodies_reuse_prefix_chunks 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/sampling_evidence.rs`。

### Requirement: Shell crates/codegen/shell/src/session/signals.rs session timeline and state model contract

crates/codegen/shell/src/session/signals.rs SHALL 维护 session timeline and state model 的入口 sample_rss_bytes, ToolOutcome, ToolDuration, PrCreatedSignal, TurnDeltaSnapshot, SessionSignalsDelta, strings, SessionSignals, TIMELINE_SIGNALS_VERSION, TIMELINE_SIGNALS_SCOPE, TIMELINE_SIGNALS_NAME, TimelineSignalsSnapshot, timeline_kind, latest_from_timeline, SignalEvent, string, SessionSignalsHandle, new (plus 89 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** sample_rss_bytes, ToolOutcome, ToolDuration, PrCreatedSignal, TurnDeltaSnapshot, SessionSignalsDelta, strings, SessionSignals, TIMELINE_SIGNALS_VERSION, TIMELINE_SIGNALS_SCOPE, TIMELINE_SIGNALS_NAME, TimelineSignalsSnapshot, timeline_kind, latest_from_timeline, SignalEvent, string, SessionSignalsHandle, new (plus 89 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** sample_rss_bytes, ToolOutcome, ToolDuration, PrCreatedSignal, TurnDeltaSnapshot, SessionSignalsDelta, strings, SessionSignals, TIMELINE_SIGNALS_VERSION, TIMELINE_SIGNALS_SCOPE, TIMELINE_SIGNALS_NAME, TimelineSignalsSnapshot, timeline_kind, latest_from_timeline, SignalEvent, string, SessionSignalsHandle, new (plus 89 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/signals.rs`。

### Requirement: Shell crates/codegen/shell/src/session/slash_commands.rs session timeline and state model contract

crates/codegen/shell/src/session/slash_commands.rs SHALL 维护 session timeline and state model 的入口 BuiltinCommand, str, BuiltinGate, BUILTIN_COMMANDS, OPS, parse_goal_budget, PROMPT_COMMANDS, CommandAvailability, allows, all_enabled, build_tools_meta, EffectiveCommandCatalog, SkillCommand, build, PAGER_COMMAND_KEYS, skill, workflow, available_commands (plus 128 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** BuiltinCommand, str, BuiltinGate, BUILTIN_COMMANDS, OPS, parse_goal_budget, PROMPT_COMMANDS, CommandAvailability, allows, all_enabled, build_tools_meta, EffectiveCommandCatalog, SkillCommand, build, PAGER_COMMAND_KEYS, skill, workflow, available_commands (plus 128 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** BuiltinCommand, str, BuiltinGate, BUILTIN_COMMANDS, OPS, parse_goal_budget, PROMPT_COMMANDS, CommandAvailability, allows, all_enabled, build_tools_meta, EffectiveCommandCatalog, SkillCommand, build, PAGER_COMMAND_KEYS, skill, workflow, available_commands (plus 128 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/slash_commands.rs`。

### Requirement: Shell crates/codegen/shell/src/session/storage/jsonl/durable_tests.rs durable session storage and search contract

crates/codegen/shell/src/session/storage/jsonl/durable_tests.rs SHALL 维护 durable session storage and search 的入口 info, update, timeline_event, append_timeline_with_prefix_state, prefix_validation_hashes_exact_bounded_bytes_and_validates_each_event, prefix_validation_rejects_short_blank_and_oversized_records, timeline_append_rejects_same_length_interior_replacement, timeline_append_rejects_interior_json_corruption, timeline_append_retries_are_idempotent_and_truncate_only_an_incomplete_tail, timeline_append_rejects_symlinked_ledger_and_lock_targets, update_append_and_replay_reject_symlinked_ledgers, timeline_reader_ignores_only_the_uncommitted_final_fragment, disk_materialization_derives_surface_and_reference_from_one_snapshot, ordinary_and_durable_appends_keep_every_physical_line_parseable, N, append_commit_is_reported_when_bookkeeping_fails, directory_barrier_failure_is_retried_even_after_file_exists, file_barrier_error_propagates (plus 2 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** info, update, timeline_event, append_timeline_with_prefix_state, prefix_validation_hashes_exact_bounded_bytes_and_validates_each_event, prefix_validation_rejects_short_blank_and_oversized_records, timeline_append_rejects_same_length_interior_replacement, timeline_append_rejects_interior_json_corruption, timeline_append_retries_are_idempotent_and_truncate_only_an_incomplete_tail, timeline_append_rejects_symlinked_ledger_and_lock_targets, update_append_and_replay_reject_symlinked_ledgers, timeline_reader_ignores_only_the_uncommitted_final_fragment, disk_materialization_derives_surface_and_reference_from_one_snapshot, ordinary_and_durable_appends_keep_every_physical_line_parseable, N, append_commit_is_reported_when_bookkeeping_fails, directory_barrier_failure_is_retried_even_after_file_exists, file_barrier_error_propagates (plus 2 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** info, update, timeline_event, append_timeline_with_prefix_state, prefix_validation_hashes_exact_bounded_bytes_and_validates_each_event, prefix_validation_rejects_short_blank_and_oversized_records, timeline_append_rejects_same_length_interior_replacement, timeline_append_rejects_interior_json_corruption, timeline_append_retries_are_idempotent_and_truncate_only_an_incomplete_tail, timeline_append_rejects_symlinked_ledger_and_lock_targets, update_append_and_replay_reject_symlinked_ledgers, timeline_reader_ignores_only_the_uncommitted_final_fragment, disk_materialization_derives_surface_and_reference_from_one_snapshot, ordinary_and_durable_appends_keep_every_physical_line_parseable, N, append_commit_is_reported_when_bookkeeping_fails, directory_barrier_failure_is_retried_even_after_file_exists, file_barrier_error_propagates (plus 2 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** info, update, timeline_event, append_timeline_with_prefix_state, prefix_validation_hashes_exact_bounded_bytes_and_validates_each_event, prefix_validation_rejects_short_blank_and_oversized_records, timeline_append_rejects_same_length_interior_replacement, timeline_append_rejects_interior_json_corruption, timeline_append_retries_are_idempotent_and_truncate_only_an_incomplete_tail, timeline_append_rejects_symlinked_ledger_and_lock_targets, update_append_and_replay_reject_symlinked_ledgers, timeline_reader_ignores_only_the_uncommitted_final_fragment, disk_materialization_derives_surface_and_reference_from_one_snapshot, ordinary_and_durable_appends_keep_every_physical_line_parseable, N, append_commit_is_reported_when_bookkeeping_fails, directory_barrier_failure_is_retried_even_after_file_exists, file_barrier_error_propagates (plus 2 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/storage/jsonl/durable_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/storage/search_fts.rs durable session storage and search contract

crates/codegen/shell/src/session/storage/search_fts.rs SHALL 维护 durable session storage and search 的入口 SCHEMA_VERSION, META_KEY_BOOTSTRAP_CLAIM, CLAIM_TOKEN_SQL, claim_stamp, SessionDoc, SessionSearchRow, QueryResult, SessionSearchIndex, with_index, open_or_create, open_existing, probe_usable, recreate, open_with_journal_mode, upsert_doc, insert_doc_if_absent, delete_doc, get_content_hash (plus 48 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SCHEMA_VERSION, META_KEY_BOOTSTRAP_CLAIM, CLAIM_TOKEN_SQL, claim_stamp, SessionDoc, SessionSearchRow, QueryResult, SessionSearchIndex, with_index, open_or_create, open_existing, probe_usable, recreate, open_with_journal_mode, upsert_doc, insert_doc_if_absent, delete_doc, get_content_hash (plus 48 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SCHEMA_VERSION, META_KEY_BOOTSTRAP_CLAIM, CLAIM_TOKEN_SQL, claim_stamp, SessionDoc, SessionSearchRow, QueryResult, SessionSearchIndex, with_index, open_or_create, open_existing, probe_usable, recreate, open_with_journal_mode, upsert_doc, insert_doc_if_absent, delete_doc, get_content_hash (plus 48 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/storage/search_fts.rs`。

### Requirement: Shell crates/codegen/shell/src/session/storage/search_recovery.rs durable session storage and search contract

crates/codegen/shell/src/session/storage/search_recovery.rs SHALL 维护 durable session storage and search 的入口 HEAL_LOCK, CACHE_EPOCH, current_epoch, CacheEpoch, now, changed, is_unusable_db_error, message_indicates_unusable_db, with_suffix, quarantine_db_files, heal_unusable, has_corrupt_sibling, quarantine_moves_main_and_sidecars, quarantined_after, heal_quarantines_only_on_confirmed_corruption, classifier_ignores_bad_query_but_catches_corruption。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** HEAL_LOCK, CACHE_EPOCH, current_epoch, CacheEpoch, now, changed, is_unusable_db_error, message_indicates_unusable_db, with_suffix, quarantine_db_files, heal_unusable, has_corrupt_sibling, quarantine_moves_main_and_sidecars, quarantined_after, heal_quarantines_only_on_confirmed_corruption, classifier_ignores_bad_query_but_catches_corruption 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** HEAL_LOCK, CACHE_EPOCH, current_epoch, CacheEpoch, now, changed, is_unusable_db_error, message_indicates_unusable_db, with_suffix, quarantine_db_files, heal_unusable, has_corrupt_sibling, quarantine_moves_main_and_sidecars, quarantined_after, heal_quarantines_only_on_confirmed_corruption, classifier_ignores_bad_query_but_catches_corruption 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/storage/search_recovery.rs`。

### Requirement: Shell crates/codegen/shell/src/session/subagent_capability.rs session timeline and state model contract

crates/codegen/shell/src/session/subagent_capability.rs SHALL 维护 session timeline and state model 的入口 CAPABILITY_CATALOG_TAG, project_agent_mcp_bindings, DelegableCapabilityCeiling, new, constrain_mode, permits_mcp_binding, CapabilityAuthority, SubagentCapabilityState, native_descriptor_is_eligible, native_catalog, from_bridge, authorization_epoch, replace_agent_harness, preview_native_catalog_prompt, mode_access, effective_access_locked, native_call_eligible, native_call_available (plus 15 additional private symbols)。实现显示该边界包含 child process lifecycle、platform or feature-gated branches、session/timeline state projection、MCP integration boundary、git/worktree context、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/subagent_capability.rs`。

### Requirement: Shell crates/codegen/shell/src/session/testkit/e2e.rs session timeline and state model contract

crates/codegen/shell/src/session/testkit/e2e.rs SHALL 维护 session timeline and state model 的入口 DUPLEX_BUFFER_BYTES, INIT_TIMEOUT, LOAD_TIMEOUT, LoadedAgent, load_session_via_agent。实现显示该边界包含 serde-backed wire/config types、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Async lifecycle
- **WHEN** DUPLEX_BUFFER_BYTES, INIT_TIMEOUT, LOAD_TIMEOUT, LoadedAgent, load_session_via_agent 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/testkit/e2e.rs`。

### Requirement: Shell crates/codegen/shell/src/session/testkit/mod.rs session timeline and state model contract

crates/codegen/shell/src/session/testkit/mod.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 filesystem or durable record I/O、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Durable boundary
- **WHEN** the file module entrypoint 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/testkit/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/testkit/synth/bench.rs session timeline and state model contract

crates/codegen/shell/src/session/testkit/synth/bench.rs SHALL 维护 session timeline and state model 的入口 AGENT_CHUNKS_PER_TURN, BULKY_CHUNK_BYTES, turn_updates, synthesize_to_target_bytes。实现显示该边界包含 filesystem or durable record I/O、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Durable boundary
- **WHEN** AGENT_CHUNKS_PER_TURN, BULKY_CHUNK_BYTES, turn_updates, synthesize_to_target_bytes 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/testkit/synth/bench.rs`。

### Requirement: Shell crates/codegen/shell/src/session/testkit/synth/mod.rs session timeline and state model contract

crates/codegen/shell/src/session/testkit/synth/mod.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 filesystem or durable record I/O、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Durable boundary
- **WHEN** the file module entrypoint 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/testkit/synth/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/testkit/synth/replay.rs session timeline and state model contract

crates/codegen/shell/src/session/testkit/synth/replay.rs SHALL 维护 session timeline and state model 的入口 parse_or, SessionSpec, default, from_env_prefixed, from_lookup, filler, WORDS, sid, text_chunk, available_commands_update, envelope_line, write_updates_jsonl, expected_replay_lines, write_rewind_jsonl, locate_session_dir, prepare_session, filler_is_exactly_n_bytes, from_lookup_falls_back_to_defaults_when_absent (plus 1 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** parse_or, SessionSpec, default, from_env_prefixed, from_lookup, filler, WORDS, sid, text_chunk, available_commands_update, envelope_line, write_updates_jsonl, expected_replay_lines, write_rewind_jsonl, locate_session_dir, prepare_session, filler_is_exactly_n_bytes, from_lookup_falls_back_to_defaults_when_absent (plus 1 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** parse_or, SessionSpec, default, from_env_prefixed, from_lookup, filler, WORDS, sid, text_chunk, available_commands_update, envelope_line, write_updates_jsonl, expected_replay_lines, write_rewind_jsonl, locate_session_dir, prepare_session, filler_is_exactly_n_bytes, from_lookup_falls_back_to_defaults_when_absent (plus 1 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/testkit/synth/replay.rs`。

### Requirement: Shell crates/codegen/shell/src/session/tool_index.rs session timeline and state model contract

crates/codegen/shell/src/session/tool_index.rs SHALL 维护 session timeline and state model 的入口 split_identifier, normalize_query, ToolMetadata, to_document, ServerMetadata, ToolMetadataSnapshot, method, Bm25ToolSearchIndex, new, search_snapshot, list_server_summaries, extract_parameter_names, split_qualified_name, make_snapshot, make_snapshot_with_servers, linear_tools, search_create_linear_issue, search_read_slack_thread (plus 113 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/tool_index.rs`。

### Requirement: Shell crates/codegen/shell/src/session/trajectory.rs session timeline and state model contract

crates/codegen/shell/src/session/trajectory.rs SHALL 维护 session timeline and state model 的入口 MAX_TRAJECTORY_DEPTH, MAX_TRAJECTORY_ENTITIES, MAX_TRAJECTORY_FILES, MAX_TRAJECTORY_SOURCE_BYTES, MAX_TRAJECTORY_EVENTS, DEFAULT_TRAJECTORY_PAGE_ROWS, MAX_TRAJECTORY_PAGE_ROWS, TRAJECTORY_OVERVIEW_BINS, TRAJECTORY_SUMMARY_CHARS, TRAJECTORY_WIRE_FIELD_CHARS, TRAJECTORY_DETAIL_PREVIEW_CHARS, TRAJECTORY_DETAIL_PREVIEW_NODES, TRAJECTORY_DETAIL_PREVIEW_DEPTH, TRAJECTORY_DETAIL_PREVIEW_ITEMS, MAX_TRAJECTORY_FULL_DETAIL_BYTES, LEDGER_TAIL_CHECK_BYTES, TrajectoryReadBudget, enter_entity (plus 155 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_TRAJECTORY_DEPTH, MAX_TRAJECTORY_ENTITIES, MAX_TRAJECTORY_FILES, MAX_TRAJECTORY_SOURCE_BYTES, MAX_TRAJECTORY_EVENTS, DEFAULT_TRAJECTORY_PAGE_ROWS, MAX_TRAJECTORY_PAGE_ROWS, TRAJECTORY_OVERVIEW_BINS, TRAJECTORY_SUMMARY_CHARS, TRAJECTORY_WIRE_FIELD_CHARS, TRAJECTORY_DETAIL_PREVIEW_CHARS, TRAJECTORY_DETAIL_PREVIEW_NODES, TRAJECTORY_DETAIL_PREVIEW_DEPTH, TRAJECTORY_DETAIL_PREVIEW_ITEMS, MAX_TRAJECTORY_FULL_DETAIL_BYTES, LEDGER_TAIL_CHECK_BYTES, TrajectoryReadBudget, enter_entity (plus 155 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MAX_TRAJECTORY_DEPTH, MAX_TRAJECTORY_ENTITIES, MAX_TRAJECTORY_FILES, MAX_TRAJECTORY_SOURCE_BYTES, MAX_TRAJECTORY_EVENTS, DEFAULT_TRAJECTORY_PAGE_ROWS, MAX_TRAJECTORY_PAGE_ROWS, TRAJECTORY_OVERVIEW_BINS, TRAJECTORY_SUMMARY_CHARS, TRAJECTORY_WIRE_FIELD_CHARS, TRAJECTORY_DETAIL_PREVIEW_CHARS, TRAJECTORY_DETAIL_PREVIEW_NODES, TRAJECTORY_DETAIL_PREVIEW_DEPTH, TRAJECTORY_DETAIL_PREVIEW_ITEMS, MAX_TRAJECTORY_FULL_DETAIL_BYTES, LEDGER_TAIL_CHECK_BYTES, TrajectoryReadBudget, enter_entity (plus 155 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** MAX_TRAJECTORY_DEPTH, MAX_TRAJECTORY_ENTITIES, MAX_TRAJECTORY_FILES, MAX_TRAJECTORY_SOURCE_BYTES, MAX_TRAJECTORY_EVENTS, DEFAULT_TRAJECTORY_PAGE_ROWS, MAX_TRAJECTORY_PAGE_ROWS, TRAJECTORY_OVERVIEW_BINS, TRAJECTORY_SUMMARY_CHARS, TRAJECTORY_WIRE_FIELD_CHARS, TRAJECTORY_DETAIL_PREVIEW_CHARS, TRAJECTORY_DETAIL_PREVIEW_NODES, TRAJECTORY_DETAIL_PREVIEW_DEPTH, TRAJECTORY_DETAIL_PREVIEW_ITEMS, MAX_TRAJECTORY_FULL_DETAIL_BYTES, LEDGER_TAIL_CHECK_BYTES, TrajectoryReadBudget, enter_entity (plus 155 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/trajectory.rs`。

### Requirement: Shell crates/codegen/shell/src/session/unified_list/cursor.rs session listing and facets contract

crates/codegen/shell/src/session/unified_list/cursor.rs SHALL 维护 session listing and facets 的入口 Cursor, BoundaryKey, decode, encode, paginate, SortKey, row_sort_key, boundary_sort_key, boundary_of, row, cursor_walk_is_stable_and_has_no_duplicates。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/unified_list/cursor.rs`。

### Requirement: Shell crates/codegen/shell/src/session/unified_list/envelope.rs session listing and facets contract

crates/codegen/shell/src/session/unified_list/envelope.rs SHALL 维护 session listing and facets 的入口 SessionKind, as_str, str, FacetValue, values, intersects, FacetMap, SessionMetaEnvelope。实现显示该边界包含 serde-backed wire/config types、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 SessionKind, as_str, str, FacetValue, values, intersects, FacetMap, SessionMetaEnvelope 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/unified_list/envelope.rs`。

### Requirement: Shell crates/codegen/shell/src/session/unified_list/facets.rs session listing and facets contract

crates/codegen/shell/src/session/unified_list/facets.rs SHALL 维护 session listing and facets 的入口 KIND_FACET_KEY, CWD_FACET_KEY, REPO_FACET_KEY, BRANCH_FACET_KEY, WORKTREE_FACET_KEY, GIT_ROOT_FACET_KEY, SOURCE_WORKSPACE_FACET_KEY, NormalizedItem, from_listing, FacetProvider, key, str, extract, KindFacet, string_facet_provider, CwdFacet, string_facet, FacetRegistry (plus 10 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/unified_list/facets.rs`。

### Requirement: Shell crates/codegen/shell/src/session/unified_list/mod.rs session listing and facets contract

crates/codegen/shell/src/session/unified_list/mod.rs SHALL 维护 session listing and facets 的入口 DEFAULT_LIMIT, FACET_REGISTRY, facet_registry, FacetRegistry, parse_list_req, ListReq, ListScope, as_str, fn, str, is_relaxed, UnifiedListResult, ParsedMeta, parse, value_list, build_unified_list, relax_eligible, lane_has_no_messages (plus 5 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** DEFAULT_LIMIT, FACET_REGISTRY, facet_registry, FacetRegistry, parse_list_req, ListReq, ListScope, as_str, fn, str, is_relaxed, UnifiedListResult, ParsedMeta, parse, value_list, build_unified_list, relax_eligible, lane_has_no_messages (plus 5 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/unified_list/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/unified_list/row.rs session listing and facets contract

crates/codegen/shell/src/session/unified_list/row.rs SHALL 维护 session listing and facets 的入口 UnifiedRow, envelope, into_session_list_row, into_session_info, sort_timestamp, session_listing_to_row, effective_local_ts, RowMeta, SessionListRow, SessionInfo。实现显示该边界包含 serde-backed wire/config types、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 UnifiedRow, envelope, into_session_list_row, into_session_info, sort_timestamp, session_listing_to_row, effective_local_ts, RowMeta, SessionListRow, SessionInfo 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/unified_list/row.rs`。

### Requirement: Shell crates/codegen/shell/src/session/user_message.rs session timeline and state model contract

crates/codegen/shell/src/session/user_message.rs SHALL 维护 session timeline and state model 的入口 user_query。实现显示该边界包含 prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 user_query 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/user_message.rs`。

### Requirement: Shell crates/codegen/shell/src/session/worktree.rs session timeline and state model contract

crates/codegen/shell/src/session/worktree.rs SHALL 维护 session timeline and state model 的入口 WORKTREE_LOG, ensure_subagent_worktree_repository, create_subagent_worktree, from, create_worktree_for_resume, cleanup_worktree_on_failure, checkout_persisted_head_in_worktree, WorktreeRestoreDecision, build_worktree_restore_outcome, resolve_session_repo_wide, resume_session_in_worktree, resume_local_session_in_worktree, subagent_worktree_initializes_non_repo_and_copies_untracked_inputs, subagent_worktree_concurrent_initialization_shares_one_baseline, subagent_worktree_reuses_existing_repo_without_changing_head_or_index, subagent_worktree_does_not_reinitialize_broken_git_metadata, resume_request_deserializes_with_defaults, resume_request_deserializes_explicit_fields (plus 38 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WORKTREE_LOG, ensure_subagent_worktree_repository, create_subagent_worktree, from, create_worktree_for_resume, cleanup_worktree_on_failure, checkout_persisted_head_in_worktree, WorktreeRestoreDecision, build_worktree_restore_outcome, resolve_session_repo_wide, resume_session_in_worktree, resume_local_session_in_worktree, subagent_worktree_initializes_non_repo_and_copies_untracked_inputs, subagent_worktree_concurrent_initialization_shares_one_baseline, subagent_worktree_reuses_existing_repo_without_changing_head_or_index, subagent_worktree_does_not_reinitialize_broken_git_metadata, resume_request_deserializes_with_defaults, resume_request_deserializes_explicit_fields (plus 38 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WORKTREE_LOG, ensure_subagent_worktree_repository, create_subagent_worktree, from, create_worktree_for_resume, cleanup_worktree_on_failure, checkout_persisted_head_in_worktree, WorktreeRestoreDecision, build_worktree_restore_outcome, resolve_session_repo_wide, resume_session_in_worktree, resume_local_session_in_worktree, subagent_worktree_initializes_non_repo_and_copies_untracked_inputs, subagent_worktree_concurrent_initialization_shares_one_baseline, subagent_worktree_reuses_existing_repo_without_changing_head_or_index, subagent_worktree_does_not_reinitialize_broken_git_metadata, resume_request_deserializes_with_defaults, resume_request_deserializes_explicit_fields (plus 38 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** WORKTREE_LOG, ensure_subagent_worktree_repository, create_subagent_worktree, from, create_worktree_for_resume, cleanup_worktree_on_failure, checkout_persisted_head_in_worktree, WorktreeRestoreDecision, build_worktree_restore_outcome, resolve_session_repo_wide, resume_session_in_worktree, resume_local_session_in_worktree, subagent_worktree_initializes_non_repo_and_copies_untracked_inputs, subagent_worktree_concurrent_initialization_shares_one_baseline, subagent_worktree_reuses_existing_repo_without_changing_head_or_index, subagent_worktree_does_not_reinitialize_broken_git_metadata, resume_request_deserializes_with_defaults, resume_request_deserializes_explicit_fields (plus 38 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/worktree.rs`。

### Requirement: Shell crates/codegen/shell/src/session/worktree_pool.rs session timeline and state model contract

crates/codegen/shell/src/session/worktree_pool.rs SHALL 维护 session timeline and state model 的入口 WORKTREE_POOL_LOG, READY_SUFFIX, CLAIMED_SUFFIX, CLAIMING_SUFFIX, marker_path, ClaimedWorktree, AcquiredWorktree, WorktreePool, new, instance_dir, adopt_orphan_worktrees, adopt_orphan_worktrees_impl, fill_loop, try_claim_ready_worktree, count_ready_worktrees, try_claim, acquire, release (plus 45 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WORKTREE_POOL_LOG, READY_SUFFIX, CLAIMED_SUFFIX, CLAIMING_SUFFIX, marker_path, ClaimedWorktree, AcquiredWorktree, WorktreePool, new, instance_dir, adopt_orphan_worktrees, adopt_orphan_worktrees_impl, fill_loop, try_claim_ready_worktree, count_ready_worktrees, try_claim, acquire, release (plus 45 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WORKTREE_POOL_LOG, READY_SUFFIX, CLAIMED_SUFFIX, CLAIMING_SUFFIX, marker_path, ClaimedWorktree, AcquiredWorktree, WorktreePool, new, instance_dir, adopt_orphan_worktrees, adopt_orphan_worktrees_impl, fill_loop, try_claim_ready_worktree, count_ready_worktrees, try_claim, acquire, release (plus 45 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** WORKTREE_POOL_LOG, READY_SUFFIX, CLAIMED_SUFFIX, CLAIMING_SUFFIX, marker_path, ClaimedWorktree, AcquiredWorktree, WorktreePool, new, instance_dir, adopt_orphan_worktrees, adopt_orphan_worktrees_impl, fill_loop, try_claim_ready_worktree, count_ready_worktrees, try_claim, acquire, release (plus 45 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/worktree_pool.rs`。
### Requirement: Tools crates/codegen/tools/src/notification/handle.rs notification fanout and wire types contract
crates/codegen/tools/src/notification/handle.rs SHALL implement the notification fanout and wire types boundary through construct typed notifications, route fanout/acks, and classify dropped or rejected delivery. Its source symbols AcknowledgedToolNotification, NotificationAcknowledgementError, NotificationAcknowledgementBatch, DurableNotificationTargets, wait, ToolNotificationTarget, CappedNotificationQueue, push, is_critical_notification, CappedToolNotificationReceiver, recv, drop, ToolNotificationHandle, default, convenience_sends, new, from_sender, channel (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/notification/handle.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/notification/handle.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/notification/handle.rs` — `AcknowledgedToolNotification`；`crates/codegen/tools/src/notification/handle.rs` — `NotificationAcknowledgementError`；`crates/codegen/tools/src/notification/handle.rs` — `NotificationAcknowledgementBatch`；`crates/codegen/tools/src/notification/handle.rs` — `DurableNotificationTargets`；`crates/codegen/tools/src/notification/handle.rs` — `wait`；`crates/codegen/tools/src/notification/handle.rs` — `ToolNotificationTarget`；`crates/codegen/tools/src/notification/handle.rs` — `CappedNotificationQueue`；`crates/codegen/tools/src/notification/handle.rs` — `push`；`crates/codegen/tools/src/notification/handle.rs` — `is_critical_notification`；`crates/codegen/tools/src/notification/handle.rs` — `CappedToolNotificationReceiver`；`crates/codegen/tools/src/notification/handle.rs` — `recv`；`crates/codegen/tools/src/notification/handle.rs` — `drop`；`crates/codegen/tools/src/notification/handle.rs` — `ToolNotificationHandle`；`crates/codegen/tools/src/notification/handle.rs` — `default`；`crates/codegen/tools/src/notification/handle.rs` — `convenience_sends`；`crates/codegen/tools/src/notification/handle.rs` — `new`；`crates/codegen/tools/src/notification/handle.rs` — `from_sender`；`crates/codegen/tools/src/notification/handle.rs` — `channel`；`crates/codegen/tools/src/notification/handle.rs` — `bounded_channel`；`crates/codegen/tools/src/notification/handle.rs` — `capped_channel`；`crates/codegen/tools/src/notification/handle.rs` — `acknowledged_channel`；`crates/codegen/tools/src/notification/handle.rs` — `noop`；`crates/codegen/tools/src/notification/handle.rs` — `tee`；`crates/codegen/tools/src/notification/handle.rs` — `durable_targets`；`crates/codegen/tools/src/notification/handle.rs` — `send`；`crates/codegen/tools/src/notification/handle.rs` — `send_acknowledged`；`crates/codegen/tools/src/notification/handle.rs` — `send_scheduled_task_removed_acknowledged`；`crates/codegen/tools/src/notification/handle.rs` — `send_task_complete_acknowledged`。

### Requirement: Tools crates/codegen/tools/src/notification/handle_tests.rs notification fanout and wire types contract
crates/codegen/tools/src/notification/handle_tests.rs SHALL implement the notification fanout and wire types boundary through construct typed notifications, route fanout/acks, and classify dropped or rejected delivery. Its source symbols removed, created, task_id, acknowledged_removal_stays_in_fifo, mixed_fanout_attempts_every_target_before_reporting_closed_dispatch, batch_distinguishes_dropped_and_rejected_acknowledgements, bounded_channel_drops_newest_when_full, capped_channel_evicts_lossy_event_for_terminal_event follow explicit markers explicit error classification、channel, fanout, or acknowledgement flow、child process execution、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/notification/handle_tests.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/notification/handle_tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/notification/handle_tests.rs` — `removed`；`crates/codegen/tools/src/notification/handle_tests.rs` — `created`；`crates/codegen/tools/src/notification/handle_tests.rs` — `task_id`；`crates/codegen/tools/src/notification/handle_tests.rs` — `acknowledged_removal_stays_in_fifo`；`crates/codegen/tools/src/notification/handle_tests.rs` — `mixed_fanout_attempts_every_target_before_reporting_closed_dispatch`；`crates/codegen/tools/src/notification/handle_tests.rs` — `batch_distinguishes_dropped_and_rejected_acknowledgements`；`crates/codegen/tools/src/notification/handle_tests.rs` — `bounded_channel_drops_newest_when_full`；`crates/codegen/tools/src/notification/handle_tests.rs` — `capped_channel_evicts_lossy_event_for_terminal_event`。

### Requirement: Tools crates/codegen/tools/src/notification/mod.rs notification fanout and wire types contract
crates/codegen/tools/src/notification/mod.rs SHALL implement the notification fanout and wire types boundary through construct typed notifications, route fanout/acks, and classify dropped or rejected delivery. Its source symbols mod follow explicit markers explicit error classification、timeout, budget, or rate limit、tool definition, schema, or registry projection、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/notification/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/notification/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/notification/types.rs notification fanout and wire types contract
crates/codegen/tools/src/notification/types.rs SHALL implement the notification fanout and wire types boundary through construct typed notifications, route fanout/acks, and classify dropped or rejected delivery. Its source symbols BashNotificationBase, output_lossy, BashOutputChunk, BashExecutionComplete, was_signaled, BashExecutionTimeout, BashExecutionBackgrounded, BashExecutionFailed, FileRead, FileWritten, CoordinationPhase, UserQuestionAsked, LspServerStarting, LspServerReady, LspServerCrashed, LspServerRetrying, LspServerFailed, ScheduledTaskFired (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/notification/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/notification/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/notification/types.rs` — `BashNotificationBase`；`crates/codegen/tools/src/notification/types.rs` — `output_lossy`；`crates/codegen/tools/src/notification/types.rs` — `BashOutputChunk`；`crates/codegen/tools/src/notification/types.rs` — `BashExecutionComplete`；`crates/codegen/tools/src/notification/types.rs` — `was_signaled`；`crates/codegen/tools/src/notification/types.rs` — `BashExecutionTimeout`；`crates/codegen/tools/src/notification/types.rs` — `BashExecutionBackgrounded`；`crates/codegen/tools/src/notification/types.rs` — `BashExecutionFailed`；`crates/codegen/tools/src/notification/types.rs` — `FileRead`；`crates/codegen/tools/src/notification/types.rs` — `FileWritten`；`crates/codegen/tools/src/notification/types.rs` — `CoordinationPhase`；`crates/codegen/tools/src/notification/types.rs` — `UserQuestionAsked`；`crates/codegen/tools/src/notification/types.rs` — `LspServerStarting`；`crates/codegen/tools/src/notification/types.rs` — `LspServerReady`；`crates/codegen/tools/src/notification/types.rs` — `LspServerCrashed`；`crates/codegen/tools/src/notification/types.rs` — `LspServerRetrying`；`crates/codegen/tools/src/notification/types.rs` — `LspServerFailed`；`crates/codegen/tools/src/notification/types.rs` — `ScheduledTaskFired`；`crates/codegen/tools/src/notification/types.rs` — `ScheduledTaskRemoved`；`crates/codegen/tools/src/notification/types.rs` — `ScheduledTaskCreated`；`crates/codegen/tools/src/notification/types.rs` — `MonitorEvent`；`crates/codegen/tools/src/notification/types.rs` — `ToolNotification`；`crates/codegen/tools/src/notification/types.rs` — `notification_variants`；`crates/codegen/tools/src/notification/types.rs` — `ALL_NOTIFICATION_TAGS`；`crates/codegen/tools/src/notification/types.rs` — `notification_schema_catalog`；`crates/codegen/tools/src/notification/types.rs` — `_assert_all_variants_listed`；`crates/codegen/tools/src/notification/types.rs` — `catalog_has_one_schema_per_variant`；`crates/codegen/tools/src/notification/types.rs` — `output_lossy_replaces_invalid_utf8`。

### Requirement: Tools crates/codegen/tools/src/persistence.rs tools crate module boundary contract
crates/codegen/tools/src/persistence.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols MAX_RESOURCES_STATE_BYTES, ResourcesStateStore, display_path, read, write_atomic, LocalResourcesStateStore, open, open_read, PublishedPersistenceError, fmt, source, published_persistence_error, ResourcesPersistence, ControlledSave, ResourcesPersistenceCommand, error_was_published, noop, controlled (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/persistence.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/persistence.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/persistence.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/persistence.rs` — `MAX_RESOURCES_STATE_BYTES`；`crates/codegen/tools/src/persistence.rs` — `ResourcesStateStore`；`crates/codegen/tools/src/persistence.rs` — `display_path`；`crates/codegen/tools/src/persistence.rs` — `read`；`crates/codegen/tools/src/persistence.rs` — `write_atomic`；`crates/codegen/tools/src/persistence.rs` — `LocalResourcesStateStore`；`crates/codegen/tools/src/persistence.rs` — `open`；`crates/codegen/tools/src/persistence.rs` — `open_read`；`crates/codegen/tools/src/persistence.rs` — `PublishedPersistenceError`；`crates/codegen/tools/src/persistence.rs` — `fmt`；`crates/codegen/tools/src/persistence.rs` — `source`；`crates/codegen/tools/src/persistence.rs` — `published_persistence_error`；`crates/codegen/tools/src/persistence.rs` — `ResourcesPersistence`；`crates/codegen/tools/src/persistence.rs` — `ControlledSave`；`crates/codegen/tools/src/persistence.rs` — `ResourcesPersistenceCommand`；`crates/codegen/tools/src/persistence.rs` — `error_was_published`；`crates/codegen/tools/src/persistence.rs` — `noop`；`crates/codegen/tools/src/persistence.rs` — `controlled`；`crates/codegen/tools/src/persistence.rs` — `new`；`crates/codegen/tools/src/persistence.rs` — `local`；`crates/codegen/tools/src/persistence.rs` — `load`；`crates/codegen/tools/src/persistence.rs` — `save`；`crates/codegen/tools/src/persistence.rs` — `enqueue_save_and_flush`；`crates/codegen/tools/src/persistence.rs` — `await_save_and_flush`；`crates/codegen/tools/src/persistence.rs` — `save_and_flush`；`crates/codegen/tools/src/persistence.rs` — `state_path`；`crates/codegen/tools/src/persistence.rs` — `flush`；`crates/codegen/tools/src/persistence.rs` — `value_to_nested_map`。

### Requirement: Tools crates/codegen/tools/src/reminders/lsp_diagnostics.rs reminder generation and completion display contract
crates/codegen/tools/src/reminders/lsp_diagnostics.rs SHALL implement the reminder generation and completion display boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols LspDiagnosticsReminder, collect_reminders follow explicit markers filesystem or durable persistence、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/reminders/lsp_diagnostics.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/reminders/lsp_diagnostics.rs` — `LspDiagnosticsReminder`；`crates/codegen/tools/src/reminders/lsp_diagnostics.rs` — `collect_reminders`。

### Requirement: Tools crates/codegen/tools/src/reminders/mod.rs reminder generation and completion display contract
crates/codegen/tools/src/reminders/mod.rs SHALL implement the reminder generation and completion display boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols to, DEFAULT_REMINDER_TAG, neutralize_reminder_tags, TAGS, wrap_reminder, wrap_reminder_with_tag, format_loop_iteration_prompt, format_with_reminders, wrap_reminder_adds_tags, format_with_reminders_wraps_and_appends, format_with_reminders_custom_tag, format_loop_iteration_prompt_frames_subagent_iteration, format_with_reminders_returns_unchanged_when_empty follow explicit markers platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/reminders/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/reminders/mod.rs` — `to`；`crates/codegen/tools/src/reminders/mod.rs` — `DEFAULT_REMINDER_TAG`；`crates/codegen/tools/src/reminders/mod.rs` — `neutralize_reminder_tags`；`crates/codegen/tools/src/reminders/mod.rs` — `TAGS`；`crates/codegen/tools/src/reminders/mod.rs` — `wrap_reminder`；`crates/codegen/tools/src/reminders/mod.rs` — `wrap_reminder_with_tag`；`crates/codegen/tools/src/reminders/mod.rs` — `format_loop_iteration_prompt`；`crates/codegen/tools/src/reminders/mod.rs` — `format_with_reminders`；`crates/codegen/tools/src/reminders/mod.rs` — `wrap_reminder_adds_tags`；`crates/codegen/tools/src/reminders/mod.rs` — `format_with_reminders_wraps_and_appends`；`crates/codegen/tools/src/reminders/mod.rs` — `format_with_reminders_custom_tag`；`crates/codegen/tools/src/reminders/mod.rs` — `format_loop_iteration_prompt_frames_subagent_iteration`；`crates/codegen/tools/src/reminders/mod.rs` — `format_with_reminders_returns_unchanged_when_empty`。

### Requirement: Tools crates/codegen/tools/src/reminders/skill_discovery.rs reminder generation and completion display contract
crates/codegen/tools/src/reminders/skill_discovery.rs SHALL implement the reminder generation and completion display boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols SKILL_CONFIG_DIR, SkillDiscoveryReminder, extract_target_path, extract_activation_paths, is_in_supported_skills_dir, requires_expr, collect_reminders follow explicit markers explicit error classification、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/reminders/skill_discovery.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/reminders/skill_discovery.rs` — `SKILL_CONFIG_DIR`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `SkillDiscoveryReminder`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `extract_target_path`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `extract_activation_paths`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `is_in_supported_skills_dir`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `requires_expr`；`crates/codegen/tools/src/reminders/skill_discovery.rs` — `collect_reminders`。

### Requirement: Tools crates/codegen/tools/src/reminders/task_completion.rs reminder generation and completion display contract
crates/codegen/tools/src/reminders/task_completion.rs SHALL implement the reminder generation and completion display boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols MAX_INLINE_COMPLETION_BYTES, format_bash_completion, format_monitor_completion, split_wrapped_monitor_event, format_monitor_events, Event, render_completion_output_delivery, resolve_task_output_tool_name, resolve_read_tool_name, format_subagent_completion, task_text_agent_id, consumed_completion_ids, task_snapshot, text_tool_result_identifies_consumed_completion, task_form_subagent_result_identifies_consumed_completion, every_terminal_task_output_consumes_its_completion_receipt, bash_pointer_and_inline_delivery_are_exclusive, subagent_inline_output_is_not_truncated (additional symbols omitted from the title but included in source evidence) follow explicit markers child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/reminders/task_completion.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/reminders/task_completion.rs` — `MAX_INLINE_COMPLETION_BYTES`；`crates/codegen/tools/src/reminders/task_completion.rs` — `format_bash_completion`；`crates/codegen/tools/src/reminders/task_completion.rs` — `format_monitor_completion`；`crates/codegen/tools/src/reminders/task_completion.rs` — `split_wrapped_monitor_event`；`crates/codegen/tools/src/reminders/task_completion.rs` — `format_monitor_events`；`crates/codegen/tools/src/reminders/task_completion.rs` — `Event`；`crates/codegen/tools/src/reminders/task_completion.rs` — `render_completion_output_delivery`；`crates/codegen/tools/src/reminders/task_completion.rs` — `resolve_task_output_tool_name`；`crates/codegen/tools/src/reminders/task_completion.rs` — `resolve_read_tool_name`；`crates/codegen/tools/src/reminders/task_completion.rs` — `format_subagent_completion`；`crates/codegen/tools/src/reminders/task_completion.rs` — `task_text_agent_id`；`crates/codegen/tools/src/reminders/task_completion.rs` — `consumed_completion_ids`；`crates/codegen/tools/src/reminders/task_completion.rs` — `task_snapshot`；`crates/codegen/tools/src/reminders/task_completion.rs` — `text_tool_result_identifies_consumed_completion`；`crates/codegen/tools/src/reminders/task_completion.rs` — `task_form_subagent_result_identifies_consumed_completion`；`crates/codegen/tools/src/reminders/task_completion.rs` — `every_terminal_task_output_consumes_its_completion_receipt`；`crates/codegen/tools/src/reminders/task_completion.rs` — `bash_pointer_and_inline_delivery_are_exclusive`；`crates/codegen/tools/src/reminders/task_completion.rs` — `subagent_inline_output_is_not_truncated`；`crates/codegen/tools/src/reminders/task_completion.rs` — `monitor_batch_groups_by_task_without_repeating_description`；`crates/codegen/tools/src/reminders/task_completion.rs` — `monitor_completion_inlines_bounded_output_without_poll_tool`。
### Requirement: Pager task-result test: command_transport_loss_is_unknown_until_a_durable_result_arrives
A slash-command transport failure SHALL remain an outcome-unknown live status until a durable correlated UiNotice arrives; the later stale error then becomes the terminal local notice exactly once.

#### Scenario: Durable command outcome
- **WHEN** transport reports an error before a durable command notice
- **THEN** the pager keeps the live unknown marker, clears it when the notice arrives, and does not duplicate the durable result.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `command_transport_loss_is_unknown_until_a_durable_result_arrives`。

### Requirement: Pager task-result test: running_status_shims_only_while_submitting
A Running prompt-status result SHALL adopt a turn only from TurnSubmitting, render the prompt boundary, clear prior follow-up chips, and retire the matching status query.

#### Scenario: Prompt status turn start
- **WHEN** status Running arrives while submitting
- **THEN** the session enters TurnRunning with fresh timing anchors and no matching query.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `running_status_shims_only_while_submitting`。

### Requirement: Pager task-result test: running_status_skips_shim_when_turn_already_running
A duplicate Running status for an already running turn SHALL refresh timing anchors without rerunning the turn-start shim or clearing follow-up chips.

#### Scenario: Duplicate prompt status
- **WHEN** status Running arrives while already TurnRunning
- **THEN** follow-up chips remain and the status query is retired.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `running_status_skips_shim_when_turn_already_running`。

### Requirement: Pager task-result test: cancel_complete_does_nothing
A CancelComplete task result SHALL be a no-op in the pager task-result reducer.

#### Scenario: Cancel completion
- **WHEN** CancelComplete is dispatched
- **THEN** no effects or visible state change are produced.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `cancel_complete_does_nothing`。

### Requirement: Pager task-result test: reconnect_generation_discards_old_control_completion
A completion from before session reload SHALL be discarded when a replacement control target has a newer dispatch generation.

#### Scenario: Reconnect generation fence
- **WHEN** reload replaces an old model control before its acknowledgement arrives
- **THEN** the old completion cannot consume the replacement target.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `reconnect_generation_discards_old_control_completion`。

### Requirement: Pager task-result test: delete_session_complete_clears_local_picker_entries_and_content_hits
Successful session deletion SHALL remove the deleted id and its content hits from both the active agent picker and welcome picker while preserving the current query.

#### Scenario: Session deletion success
- **WHEN** the selected session is deleted while both picker surfaces contain it
- **THEN** both entry lists and content-hit lists exclude the deleted session.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `delete_session_complete_clears_local_picker_entries_and_content_hits`。

### Requirement: Pager task-result test: delete_session_failed_keeps_all_entries
A failed session deletion SHALL preserve every local picker entry.

#### Scenario: Session deletion failure
- **WHEN** deletion returns an error
- **THEN** the picker remains unchanged.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `delete_session_failed_keeps_all_entries`。

### Requirement: Pager task-result test: rename_session_failed_keeps_local_display_name_and_pushes_system_block
Rename failure SHALL preserve the optimistic local display name and append a system block containing the error.

#### Scenario: Session rename failure
- **WHEN** on-disk rename fails after local cache update
- **THEN** the name remains visible and the failure is appended.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `rename_session_failed_keeps_local_display_name_and_pushes_system_block`。

### Requirement: Pager task-result test: session_list_relax_surfaces_notice_once
A relaxed repository session-list response SHALL explain the broadened scope once, suppress repeats for the same scope, avoid search rearming, and rearm after returning from a cwd-scoped browse.

#### Scenario: Relaxed session list notice
- **WHEN** repo-scoped results include sessions outside the cwd
- **THEN** one notice appears per relaxed scope transition.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `session_list_relax_surfaces_notice_once`。

### Requirement: Pager task-result test: session_list_relax_on_welcome_does_not_latch
A relaxed session-list response on the Welcome view SHALL not latch a notice that the surface cannot render.

#### Scenario: Welcome relaxed session list
- **WHEN** repo-scoped sessions load before an agent view exists
- **THEN** the relaxed-notified marker remains unset.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `session_list_relax_on_welcome_does_not_latch`。

### Requirement: Pager task-result test: session_list_relax_renotifies_when_cwd_changes
The relaxed session-list notice SHALL be keyed by browse cwd so a different cwd produces a new notice.

#### Scenario: Cwd-scoped relaxed notice
- **WHEN** repo results are received under two different cwd values
- **THEN** both cwd transitions show the guidance.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `session_list_relax_renotifies_when_cwd_changes`。

### Requirement: Pager task-result test: session_list_empty_without_partial_keeps_generic_toast
An empty nondegraded cwd session list SHALL keep the generic No sessions found toast.

#### Scenario: Empty session list
- **WHEN** no sessions are returned without a relaxed/partial lane
- **THEN** the generic empty-list message remains visible.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `session_list_empty_without_partial_keeps_generic_toast`。
### Requirement: Agent focus restoration contract
FocusGained SHALL move from Scrollback to Prompt when needs-input overlays are pending regardless of Vim/turn state, or when the agent is idle and non-Vim without a modal; Vim idle and busy non-overlay states SHALL remain in Scrollback.

#### Scenario: Focus restoration
- **WHEN** FocusGained arrives while Scrollback is active
- **THEN** the predicate selects prompt restoration only for the documented state combinations.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_permission_vim_turn_running
FocusGained SHALL restore Prompt when a permission overlay is pending even in Vim mode during a running turn.

#### Scenario: Permission focus restoration
- **WHEN** Scrollback is active with Vim, running state, and permission
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_permission_vim_turn_running`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_permission_non_vim_turn_running
FocusGained SHALL restore Prompt when permission is pending in non-Vim running state.

#### Scenario: Permission focus restoration non-Vim
- **WHEN** Scrollback is active with a running turn and permission
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_permission_non_vim_turn_running`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_idle_non_vim_no_overlay
Idle non-Vim Scrollback SHALL restore Prompt on FocusGained when no modal blocks it.

#### Scenario: Idle focus restoration
- **WHEN** Scrollback is active, idle, non-Vim, and clear
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_idle_non_vim_no_overlay`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_idle_vim_no_overlay
Idle Vim Scrollback SHALL remain in Scrollback on FocusGained without an input overlay.

#### Scenario: Vim focus restoration
- **WHEN** Scrollback is active, idle, Vim, and clear
- **THEN** the predicate is false.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_idle_vim_no_overlay`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_busy_non_vim_no_overlay
Busy non-Vim Scrollback SHALL not restore Prompt without a needs-input overlay.

#### Scenario: Busy focus restoration
- **WHEN** Scrollback is active during a running turn without overlay
- **THEN** the predicate is false.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_busy_non_vim_no_overlay`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_permission_already_prompt
FocusGained SHALL not request restoration when Prompt is already active, even if permission is pending.

#### Scenario: Prompt focus no-op
- **WHEN** Prompt is already active with permission
- **THEN** the predicate is false.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_permission_already_prompt`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_permission_with_modal
An active top-level modal SHALL block focus restoration even when a permission overlay is pending.

#### Scenario: Modal focus precedence
- **WHEN** Scrollback, Vim, running, permission, and command palette coexist
- **THEN** the predicate is false.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_permission_with_modal`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_plan_approval_vim
Plan approval SHALL force Prompt restoration from Scrollback in Vim mode during a running turn.

#### Scenario: Plan focus restoration
- **WHEN** plan approval is pending in Vim running state
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_plan_approval_vim`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_question_vim
Question input SHALL force Prompt restoration from Scrollback in Vim mode during a running turn.

#### Scenario: Question focus restoration
- **WHEN** a question input overlay is pending
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_question_vim`。

### Requirement: Pager agent input test: should_restore_prompt_on_focus_gained_cancel_turn_vim
Cancel-turn input SHALL force Prompt restoration from Scrollback in Vim mode during a running turn.

#### Scenario: Cancel focus restoration
- **WHEN** cancel-turn confirmation is pending
- **THEN** the predicate is true.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `should_restore_prompt_on_focus_gained_cancel_turn_vim`。
