## ADDED Requirements

### Requirement: Memory storage roots and flat mode
MemoryStorage SHALL 将默认根设置为 grow_home/memory，可用 root_override 替换；hashed 模式使用工作区身份子目录，flat 模式全局与工作区同根。

#### Scenario: 实现边界
- **WHEN** 构造 storage
- **THEN** 不立即创建目录；flat 模式不启用 ephemeral 跳过。

证据：`crates/codegen/memory/src/storage.rs` — `new_inner`。

### Requirement: Memory repository identity
工作区身份 SHALL 优先从 Git origin 字符串提取去主机的仓库路径，再以 slug40 和 blake3 前8 hex 命名；无法取得时使用 canonical cwd，失败回退原路径。

#### Scenario: 实现边界
- **WHEN** 两个主机提供同名 org/repo
- **THEN** 身份不含主机，可能共用记忆目录；hash8 不提供无碰撞保证。

证据：`crates/codegen/memory/src/storage.rs` — `compute_workspace_hash`。

### Requirement: Memory ASCII slug normalization
slugify SHALL 小写后仅保留 ASCII 字母数字，其他字符变短横线、折叠连续短横线，截断并去两端短横线。

#### Scenario: 实现边界
- **WHEN** 输入全为非ASCII文字
- **THEN** 可能返回空串；工作区命名调用方另回退 workspace。

证据：`crates/codegen/memory/src/storage.rs` — `slugify`。

### Requirement: Memory ephemeral workspace writes
hashed storage SHALL 根据系统临时目录及内置路径模式识别 ephemeral，跳过工作区初始化、长期写入、追加和日志写入。

#### Scenario: 实现边界
- **WHEN** 临时工作区写全局 memory
- **THEN** 全局写入仍执行；跳过的工作区写操作返回成功而非持久化证明。

证据：`crates/codegen/memory/src/storage.rs` — `is_ephemeral_cwd`。

### Requirement: Memory daily log lifecycle
write_daily_log SHALL 使用 date-slug-session前8字节.md；append 且文件存在时追加分隔线和 UTC flush 注释，否则覆盖。

#### Scenario: 实现边界
- **WHEN** 传入任意文件名组件或多字节 session ID
- **THEN** 本层不净化路径组件，字节8切片可能不是字符边界；输入约束由调用方承担。

证据：`crates/codegen/memory/src/storage.rs` — `write_daily_log`。

### Requirement: Memory long term overwrite and append
长期 memory SHALL 按 scope 写 MEMORY.md；write_long_term 直接覆盖，append_to_memory 规范化后追加，已有非空文件前加两个换行。

#### Scenario: 实现边界
- **WHEN** 内容为空白
- **THEN** append 不创建文件；这些方法不自动更新索引，也不提供跨进程写入事务。

证据：`crates/codegen/memory/src/storage.rs` — `append_to_memory`。

### Requirement: Memory note heading normalization
normalize_memory_content SHALL trim 输入，# 开头保留，单行转为标题，多行首行不超过80字节时提升为标题，否则使用 Note 标题。

#### Scenario: 实现边界
- **WHEN** 单行用户笔记被规范化
- **THEN** 得到纯标题，搜索结构过滤可将其排除；不能承诺追加后所有内容可搜。

证据：`crates/codegen/memory/src/storage.rs` — `normalize_memory_content`。

### Requirement: Memory root confined reads
read_file SHALL canonicalize 文件和全局根并检查目标位于根下，再完整读取UTF8文本后应用零起始行范围。

#### Scenario: 实现边界
- **WHEN** 读取其它工作区在同根下的文件
- **THEN** 本层允许；范围读取会规范化换行，默认完整读取保留原文，不以索引成员资格授权。

证据：`crates/codegen/memory/src/storage.rs` — `read_file`。

### Requirement: Memory file enumeration and source classification
list_memory_files SHALL 列举全局和工作区 MEMORY.md，再列举 sessions 直接子项的小写md后缀，后者按路径排序。

#### Scenario: 实现边界
- **WHEN** flat根或md后缀目录
- **THEN** 同一MEMORY.md可重复，sessions不检查is_file；classify_source是词法位置分类，不是授权。

证据：`crates/codegen/memory/src/storage.rs` — `list_memory_files`。

### Requirement: Memory template initialization and clearing
ensure_initialized SHALL 在不存在时创建全局和工作区模板；clear_workspace 删除整个工作区根，clear_global 只删全局 MEMORY.md。

#### Scenario: 实现边界
- **WHEN** flat模式清工作区
- **THEN** 删除flat根整体；缺失返回false，不自动停后台任务或协调打开的数据库。

证据：`crates/codegen/memory/src/storage.rs` — `clear_workspace`。

### Requirement: Memory conservative empty directory collection
gc SHALL 跳过flat根和当前工作区，只删除满足年龄条件的空目录或仅含空sessions的目录；tmp前缀门限7天，其余使用max_age_days。

#### Scenario: 实现边界
- **WHEN** 目录含MEMORY.md、索引或日志
- **THEN** 正常读取时保留，不因tmp名称或年龄而删除；进程Mutex不等于跨进程写入租约。

证据：`crates/codegen/memory/src/storage.rs` — `gc`。

### Requirement: Memory chunk hash and short document preservation
chunk_markdown SHALL 对空输入返回空，短于配置字节阈值的文本原样为一个块，块哈希使用blake3。

#### Scenario: 实现边界
- **WHEN** max_chunk_chars用于多字节文本
- **THEN** 实际比较UTF8字节数；行范围零起始末尾排他。

证据：`crates/codegen/memory/src/chunker.rs` — `chunk_markdown`。

### Requirement: Memory heading chunks and context limits
大文本分块 SHALL 按宽松井号标题及行段拆分，加入父标题上下文，部分空行溢出分支附加字符尾部重叠。

#### Scenario: 实现边界
- **WHEN** 单行超长或围栏内出现标题
- **THEN** 不保证硬长度上限或围栏完整，上下文不从预算扣除；不是完整Markdown解析器。

证据：`crates/codegen/memory/src/chunker.rs` — `header_level`。

### Requirement: Memory query keyword extraction
extract_keywords SHALL Unicode小写、按非字母数字且非下划线分割，过滤小于2字节、英文停用词和纯数字词，再按首次次序去重。

#### Scenario: 实现边界
- **WHEN** 单个多字节字符或全下划线
- **THEN** 可能保留，不执行词干还原或中文分词。

证据：`crates/codegen/memory/src/query_expansion.rs` — `extract_keywords`。

### Requirement: Memory SQLite schema and optional vectors
memory index SHALL 使用meta、chunks和contentless FTS5表；扩展可用时创建指定维度的vec表，连接journal由sqlite-journal处理。

#### Scenario: 实现边界
- **WHEN** sqlite-vec探测失败
- **THEN** 保留FTS模式；SCHEMA_VERSION常量本身不实施自动迁移。

证据：`crates/codegen/memory/src/schema.rs` — `schema_sql`。

### Requirement: Memory vector space identity switching
open_or_create SHALL 在Immediate事务内核对维度及endpoint/model身份，变化时重建vec表并更新元数据，保留chunks和FTS。

#### Scenario: 实现边界
- **WHEN** 无identity的FTS句柄打开同库
- **THEN** 不使已有向量失效；扩展不可用时不重标vec身份。

证据：`crates/codegen/memory/src/index.rs` — `open_or_create_with_journal_mode`。

### Requirement: Memory content reindex transactions
reindex_file SHALL 按lossy路径加块序号标识，比较hash以新增、更新和删除chunk并维护FTS。

#### Scenario: 实现边界
- **WHEN** 文件不可读或hash相同
- **THEN** 不可读返回零变化保留旧索引；hash相同跳过全部字段，source也不更新。

证据：`crates/codegen/memory/src/index.rs` — `reindex_file`。

### Requirement: Memory index vector invalidation limits
内容更新和路径删除 SHALL 尝试删除旧vec项，但忽略vec删除错误；文本与FTS主要变更使用事务。

#### Scenario: 实现边界
- **WHEN** 向量删除失败
- **THEN** 不能承诺三表全部失败一起回滚；现存chunk预读也不在写事务内。

证据：`crates/codegen/memory/src/index.rs` — `delete_path`。

### Requirement: Memory FTS source filtering
FTS SHALL 将关键词以OR合并并按rank排序，source查询在限制数量之前过滤，空关键词或sources返回空。

#### Scenario: 实现边界
- **WHEN** 单条FTS rowid无法解析为chunk
- **THEN** 跳过该条；普通limit使用checked转换，source版本使用as i64。

证据：`crates/codegen/memory/src/index.rs` — `search_fts_by_sources`。

### Requirement: Memory vector operation identity checks
向量读写 SHALL 在同一事务内核对当前cache身份；旧身份查询返回空，旧身份upsert返回错误。

#### Scenario: 实现边界
- **WHEN** chunk文本在嵌入等待期间变更
- **THEN** 本层不比较chunk hash或确认chunk仍存在；身份检查不是内容版本检查。

证据：`crates/codegen/memory/src/index.rs` — `upsert_embedding`。

### Requirement: Memory reindex claim lease
try_claim_reindex SHALL 原子取得空或时间严格早于cutoff的claim，记录pid和时间；SQL失败视为未取得。

#### Scenario: 实现边界
- **WHEN** 释放claim或旧任务超时
- **THEN** release_claim不检查owner，无续租或PID存活检查。

证据：`crates/codegen/memory/src/index.rs` — `try_claim_reindex`。

### Requirement: Memory access and administrative reads
record_access SHALL 增加访问次数并更新时间；all_indexed_paths 返回排序去重路径，get_chunk按ID读元数据。

#### Scenario: 实现边界
- **WHEN** 查询局部错误
- **THEN** get_chunk行查询错误可变None，claim读取错误变空，paths坏行跳过；不能以空值证明数据库健康。

证据：`crates/codegen/memory/src/index.rs` — `get_chunk`。

### Requirement: Memory watcher dirty path handoff
watcher SHALL 递归收集Create/Modify/Remove事件中的小写md路径，以ArcSwap RCU去重并原子换空。

#### Scenario: 实现边界
- **WHEN** 订阅失败或运行时notify错误
- **THEN** 订阅失败警告返回None，运行时错误忽略；目录事件不展开子文件，返回路径无序。

证据：`crates/codegen/memory/src/watcher.rs` — `MemoryFileWatcher`。

### Requirement: Memory endpoint credential capability
EmbeddingEndpoint SHALL 私有绑定URL及凭据；静态key拒绝空白，live能力要求与进程配置的非loopback HTTPS完整URL一致。

#### Scenario: 实现边界
- **WHEN** 端点仅主机相同而路径不同
- **THEN** 拒绝复用live凭据；能力建立后环境变化不重定向既有端点。

证据：`crates/codegen/memory/src/embedding.rs` — `EmbeddingEndpoint`。

### Requirement: Memory endpoint URL policy
嵌入端点 SHALL 拒绝userinfo、query、fragment，仅允许HTTPS或识别为loopback的HTTP，并追加embeddings路径段。

#### Scenario: 实现边界
- **WHEN** 服务返回重定向
- **THEN** HTTP客户端不跟随重定向；配置连接超时30秒，不额外设置整个响应读取超时。

证据：`crates/codegen/memory/src/embedding.rs` — `embedding_http_client`。

### Requirement: Memory embedding provider credential resolution
make_provider SHALL 在模型存在且非空时建立provider，Auth优先于动态key；动态key异步读取一次后绑定本provider。

#### Scenario: 实现边界
- **WHEN** 复用同一个动态key provider实例
- **THEN** 不在每个批次重新执行key命令；Auth认证中间件另按请求取得凭据。

证据：`crates/codegen/memory/src/embedding.rs` — `make_provider`。

### Requirement: Memory embedding request retries
embed_batch SHALL 顺序分32项批次发送model/input/dimensions，send错误、429及5xx最多总尝试3次，间隔1秒和2秒。

#### Scenario: 实现边界
- **WHEN** 成功状态JSON解析失败或其它非成功状态
- **THEN** 直接错误，不按瞬态状态重试；空输入返回空。

证据：`crates/codegen/memory/src/embedding.rs` — `embed_batch`。

### Requirement: Memory embedding response pairing
嵌入响应 SHALL 按data数组顺序取得embedding数组并过滤非数字元素，转f32；不核对index、结果数或维度。

#### Scenario: 实现边界
- **WHEN** 服务返回乱序或缺项
- **THEN** 调用方zip按位置配对，不能保证与输入一一对应；后续批失败使整个调用返回Err。

证据：`crates/codegen/memory/src/embedding.rs` — `ApiEmbeddingProvider`。

### Requirement: Memory embedding backfill helper
embed_missing_chunks SHALL 查询全部缺失项后逐32项批次嵌入，逐条写入并返回成功次数，批失败或写失败警告后继续。

#### Scenario: 实现边界
- **WHEN** 结果数量少于输入
- **THEN** zip只处理配对项，不把剩余项报告为成功。

证据：`crates/codegen/memory/src/lib.rs` — `embed_missing_chunks`。

### Requirement: Memory deterministic embedding test support
test-support SHALL 暴露MockEmbeddingProvider，以blake3字节循环除255构造给定维度向量。

#### Scenario: 实现边界
- **WHEN** 使用Mock验证向量排名
- **THEN** 它不保证单位范数或语义相似度，不能替代真实模型质量测试。

证据：`crates/codegen/memory/src/embedding.rs` — `MockEmbeddingProvider`。

### Requirement: Memory hybrid candidate collection
hybrid_search SHALL 收集max_results三倍的FTS候选并额外查询global/workspace，再以chunk ID合并；向量可用且有provider时嵌入查询。

#### Scenario: 实现边界
- **WHEN** FTS错误或查询嵌入失败
- **THEN** FTS错误转空，嵌入错误警告后降级；evergreen补充不是最终结果配额。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search`。

### Requirement: Memory hybrid score combination
搜索 SHALL 对FTS候选相对归一化、向量按clamp(1-distance/2)计分；两者均正时取加权和与FTS分的较大值。

#### Scenario: 实现边界
- **WHEN** 只有正FTS分
- **THEN** 使用完整FTS分，不受text_weight惩罚；仅向量使用vector_weight。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search_merge`。

### Requirement: Memory temporal and access ranking
搜索 SHALL 对非global/workspace来源按创建时间半衰减，再乘source权重和1+0.05*ln1p(access_count)。

#### Scenario: 实现边界
- **WHEN** 显示分都截为1
- **THEN** 仍按未截断原始分排序；min_score比较截断显示分，未来时间年龄视0。

证据：`crates/codegen/memory/src/search.rs` — `temporal_decay_multiplier`。

### Requirement: Memory empty content search filter
搜索 SHALL 排除结构上只有空白、井号标题及完整HTML注释的块，evergreen另排除短scaffold标记文本。

#### Scenario: 实现边界
- **WHEN** 块只有单行标题笔记
- **THEN** 同样会被排除；未闭合HTML注释作为文字保留，不按完整Markdown语法识别。

证据：`crates/codegen/memory/src/search.rs` — `is_content_free`。

### Requirement: Memory MMR diversity ordering
mmr_rerank SHALL 用小写snippet词集合Jaccard和独立raw relevance贪心重排，再由搜索截断结果数。

#### Scenario: 实现边界
- **WHEN** 关闭MMR、结果少于2或lambda为1
- **THEN** 原序返回；启用路径要求relevance长度一致，重排保留所有字段和结果总数。

证据：`crates/codegen/memory/src/mmr.rs` — `mmr_rerank`。

### Requirement: Memory search output and soft failures
搜索merge SHALL 返回完整chunk文本及来源行范围，底层向量查询错误或无法取得chunk时跳过，不设置snippet长度预算。

#### Scenario: 实现边界
- **WHEN** 所有候选失败或被过滤
- **THEN** 可返回空成功；相同raw分无固定chunk ID排序保证。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search_merge`。

### Requirement: Memory backend session factory
from_session_params SHALL 配置会话ID、search参数、成对embedding配置及能力、可选watcher与诊断source，每实例建立计数器。

#### Scenario: 实现边界
- **WHEN** 调用search指定max_results和min_score
- **THEN** 覆盖保存的这两项，其余搜索参数保留；索引分块使用默认配置。

证据：`crates/codegen/memory/src/backend.rs` — `from_session_params`。

### Requirement: Memory backend sync on search
backend search SHALL 在dirty且取得claim时取出路径，存在则重建、不存在则删索引；向量空间可用时另尝试回填缺失项。

#### Scenario: 实现边界
- **WHEN** 重建失败或异步任务取消
- **THEN** 失败路径不重新放回dirty集合；取消无RAII释放claim，依赖过期重取。

证据：`crates/codegen/memory/src/backend.rs` — `search`。

### Requirement: Memory backend embedding backfill ordering
backend SHALL 先完成所有回填批次并积累upserts，写回后释放claim，再执行FTS与查询向量搜索。

#### Scenario: 实现边界
- **WHEN** 没有文件变更但向量身份切换
- **THEN** 仍尝试回填；provider不可得则跳过嵌入并释放已取得claim。

证据：`crates/codegen/memory/src/backend.rs` — `reindex_chunks`。

### Requirement: Memory backend access and diagnostics
backend SHALL 为返回项记录访问（失败忽略），记录空或非空搜索事件，成功走到末尾才递增搜索计数。

#### Scenario: 实现边界
- **WHEN** 查询嵌入失败但provider可用
- **THEN** 诊断mode仍可能是hybrid；duration不含前置打开及回填，不能据mode证明向量命中。

证据：`crates/codegen/memory/src/backend.rs` — `search_counter`。

### Requirement: Memory backend read and count contracts
backend get SHALL 委托根限制文件读取，total_chunks以journal-aware只读连接统计，默认搜索参数访问器返回保存配置。

#### Scenario: 实现边界
- **WHEN** 数据库打开或计数失败
- **THEN** backend返回错误；MemoryStorage的同类统计返回0，两者不可混为同一失败契约。

证据：`crates/codegen/memory/src/backend.rs` — `total_chunks`。

### Requirement: Memory dream lock best effort ownership
DreamLock SHALL 用PID正文与mtime写后复读协调，活PID且未超龄拒绝，失效或超龄可重取。

#### Scenario: 实现边界
- **WHEN** 并发取得或同进程多任务
- **THEN** 不保证严格互斥，无独立owner token或自动续期；Unix和Windows存活探测权限失败处理不同。

证据：`crates/codegen/memory/src/dream_lock.rs` — `try_acquire`。

### Requirement: Memory dream lock rollback and timestamp
DreamLock SHALL 支持记录整理时间及回滚；无prior删除锁，有prior清空PID并恢复mtime。

#### Scenario: 实现边界
- **WHEN** 另一任务已重写锁
- **THEN** rollback不核对owner；record_consolidation仍保留当前PID正文。

证据：`crates/codegen/memory/src/dream_lock.rs` — `rollback`。

### Requirement: Memory dream eligible session scan
sessions_since SHALL 列举直接小写md条目，按mtime严格晚于cutoff筛选，并排除stem以后缀匹配的当前session。

#### Scenario: 实现边界
- **WHEN** 排除串为空或条目是md目录
- **THEN** 空串排除全部；本层不检查is_file，结果按stem排序而非修改时间。

证据：`crates/codegen/memory/src/dream_lock.rs` — `sessions_since`。

### Requirement: Memory dream gate order
check_dream_gates SHALL 依次检查enabled、距锁mtime整小时间隔和会话数量，无锁使用epoch，IO失败返回Error。

#### Scenario: 实现边界
- **WHEN** 得到Open
- **THEN** 仅代表门控通过，未取得锁；未来mtime产生零间隔。

证据：`crates/codegen/memory/src/dream.rs` — `check_dream_gates`。

### Requirement: Memory scaffold template predicate
is_scaffold_template SHALL 要求trim文本小于500字节且含三个固定标记之一。

#### Scenario: 实现边界
- **WHEN** 真实短笔记引用模板标记
- **THEN** 仍可判为scaffold；达到500字节不按该谓词过滤。

证据：`crates/codegen/memory/src/dream.rs` — `is_scaffold_template`。

### Requirement: Memory dream prompt and input budget
build_dream_user_message SHALL 在会话前加入非scaffold现有记忆，现有记忆截至16000字节UTF8边界；会话整份追加后检查32000字节阈值。

#### Scenario: 实现边界
- **WHEN** 单份会话超大
- **THEN** 仍完整读取和追加，阈值不是内存硬上限；processed_stems只含成功追加项，无可读会话返回None。

证据：`crates/codegen/memory/src/dream.rs` — `build_dream_user_message`。

### Requirement: Memory dream model instruction seam
DREAM_SYSTEM_PROMPT SHALL 请求合并旧知识、解决矛盾、绝对日期、删除短期噪声和保留决策；模型请求由宿主完成。

#### Scenario: 实现边界
- **WHEN** 模型返回含标题的错误总结
- **THEN** 本包不验证事实正确或旧知识完整，提示词不是语义验收保证。

证据：`crates/codegen/memory/src/dream.rs` — `DREAM_SYSTEM_PROMPT`。

### Requirement: Memory dream response acceptance
process_dream_response SHALL 拒绝空、NO_REPLY及无标题子串响应，随后按Unicode字符截至16000。

#### Scenario: 实现边界
- **WHEN** 唯一标题位于截断范围外
- **THEN** 检查先于截断，最终文本未重新验证标题；标题检测非锚定。

证据：`crates/codegen/memory/src/dream.rs` — `process_dream_response`。

### Requirement: Memory dream execution outcomes
execute_dream SHALL 取得锁后处理现成response，有效则覆盖workspace MEMORY.md并清理processed日志，写失败尝试rollback。

#### Scenario: 实现边界
- **WHEN** 没有有效输出
- **THEN** 返回NothingToConsolidate并保留新锁mtime，不清理日志；rollback失败不改变原Failed结果。

证据：`crates/codegen/memory/src/dream.rs` — `execute_dream`。

### Requirement: Memory dream cleanup eligibility
成功整理 SHALL 只尝试删除processed_stems中超过5分钟未修改的日志，并返回真正删除的stems；删除失败不改变Completed。

#### Scenario: 实现边界
- **WHEN** 元数据读取失败、并发更新或未处理旧文件
- **THEN** 元数据失败仍尝试删除，无hash或租约验证；未处理文件保留不保证下一轮mtime门控会重新纳入。

证据：`crates/codegen/memory/src/dream.rs` — `clean_processed_sessions`。
### Requirement: Pager correlated memory files modal admission

MemoryFiles SHALL require live non-loading delivery and exact pending invocation identity. Acceptance clears pending id/feedback before modal collision checking; an occupied modal discards the files, otherwise a MemoryBrowser opens. Replay/loading/stale ids leave pending state unchanged.

#### Scenario: Correlated
- **WHEN** identity matches and no modal is active
- **THEN** MemoryBrowser opens.

#### Scenario: Collision
- **WHEN** another modal is active
- **THEN** pending state clears but files are discarded.

#### Scenario: Stale
- **WHEN** id differs or replay is active
- **THEN** the update is ignored.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `MemoryFiles`。


### Requirement: Pager memory rewrite save BTW and live interjection effects

Memory rewrite SHALL call grow/memory/rewrite with raw text and context and fall back to the original text for ACP failure, malformed response or missing rewritten content. Global memory save SHALL append in blocking work and report join or storage failure. BTW SHALL query grow/btw and return its answer or the literal No response fallback. Live interjection SHALL materialize any images, represent text-only steering as a single content block, call grow/steer with expected turn and interjection ids, and preserve original input on failure for retry. This file does not prove model rewrite quality, memory file locking, BTW isolation, image placeholder semantics, expected-turn enforcement or server-side insertion ordering.

#### Scenario: Rewrite fallback
- **WHEN** rewrite fails or has no string rewritten value
- **THEN** MemoryNoteRewritten returns Ok with the original raw text.

#### Scenario: Memory save failure
- **WHEN** the blocking task cannot join or append_to_memory fails
- **THEN** MemoryNoteSaved carries a string error.

#### Scenario: Text-only interjection
- **WHEN** no content blocks are supplied
- **THEN** grow/steer receives one text content block and no second top-level text channel.

#### Scenario: Interjection image failure
- **WHEN** image materialization fails
- **THEN** no steer request is sent and InterjectFailed retains text, blocks and images.

#### Scenario: BTW missing answer
- **WHEN** a successful BTW response lacks result.answer
- **THEN** the displayed answer is No response.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。
### Requirement: Tools crates/codegen/tools/src/implementations/memory/get_tool.rs memory search tool contract
crates/codegen/tools/src/implementations/memory/get_tool.rs SHALL implement the memory search tool boundary through validate memory query/output and delegate search/get to the memory backend. Its source symbols format_with_line_numbers, MemoryGetImpl, kind, tool_namespace, description_template, Args, Output, id, description, capabilities, run, test_format_basic_line_numbers, test_format_offset_adjusts_line_numbers, test_format_empty_content, test_format_single_line, test_format_large_line_numbers, test_format_trailing_newline_emits_blank_line, test_format_double_trailing_newline (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/memory/get_tool.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `format_with_line_numbers`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `MemoryGetImpl`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `kind`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `description_template`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `Args`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `Output`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `id`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `description`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `capabilities`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `run`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_basic_line_numbers`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_offset_adjusts_line_numbers`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_empty_content`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_single_line`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_large_line_numbers`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_trailing_newline_emits_blank_line`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_double_trailing_newline`；`crates/codegen/tools/src/implementations/memory/get_tool.rs` — `test_format_no_trailing_newline_no_blank_line`。

### Requirement: Tools crates/codegen/tools/src/implementations/memory/mod.rs memory search tool contract
crates/codegen/tools/src/implementations/memory/mod.rs SHALL implement the memory search tool boundary through validate memory query/output and delegate search/get to the memory backend. Its source symbols MEMORY_SEARCH_TOOL_NAME, MEMORY_GET_TOOL_NAME, memory_tool_constants_match_registered_ids follow explicit markers platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/memory/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/memory/mod.rs` — `MEMORY_SEARCH_TOOL_NAME`；`crates/codegen/tools/src/implementations/memory/mod.rs` — `MEMORY_GET_TOOL_NAME`；`crates/codegen/tools/src/implementations/memory/mod.rs` — `memory_tool_constants_match_registered_ids`。

### Requirement: Tools crates/codegen/tools/src/implementations/memory/search_tool.rs memory search tool contract
crates/codegen/tools/src/implementations/memory/search_tool.rs SHALL implement the memory search tool boundary through validate memory query/output and delegate search/get to the memory backend. Its source symbols MemorySearchImpl, kind, tool_namespace, description_template, Args, Output, id, description, capabilities, run follow explicit markers explicit error classification、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/memory/search_tool.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `MemorySearchImpl`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `kind`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `description_template`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `Args`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `Output`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `id`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `description`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `capabilities`；`crates/codegen/tools/src/implementations/memory/search_tool.rs` — `run`。

### Requirement: Tools crates/codegen/tools/src/implementations/memory/types.rs memory search tool contract
crates/codegen/tools/src/implementations/memory/types.rs SHALL implement the memory search tool boundary through validate memory query/output and delegate search/get to the memory backend. Its source symbols MemorySearchInput, MemorySearchOutput, MemoryGetInput, MemoryGetOutput follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/memory/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/memory/types.rs` — `MemorySearchInput`；`crates/codegen/tools/src/implementations/memory/types.rs` — `MemorySearchOutput`；`crates/codegen/tools/src/implementations/memory/types.rs` — `MemoryGetInput`；`crates/codegen/tools/src/implementations/memory/types.rs` — `MemoryGetOutput`。
