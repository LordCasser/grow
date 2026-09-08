# memory 逐包核查

包路径：`crates/codegen/memory`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/memory/Cargo.toml`
- `crates/codegen/memory/src/backend.rs`
- `crates/codegen/memory/src/chunker.rs`
- `crates/codegen/memory/src/dream.rs`
- `crates/codegen/memory/src/dream_lock.rs`
- `crates/codegen/memory/src/embedding.rs`
- `crates/codegen/memory/src/index.rs`
- `crates/codegen/memory/src/lib.rs`
- `crates/codegen/memory/src/mmr.rs`
- `crates/codegen/memory/src/query_expansion.rs`
- `crates/codegen/memory/src/schema.rs`
- `crates/codegen/memory/src/search.rs`
- `crates/codegen/memory/src/storage.rs`
- `crates/codegen/memory/src/text_utils.rs`
- `crates/codegen/memory/src/watcher.rs`

Cargo feature：`{"test-support": []}`。

## 功能与规范映射

- [Memory storage roots and flat mode](../specs/memory-search/spec.md#requirement-memory-storage-roots-and-flat-mode)：MemoryStorage SHALL 将默认根设置为 grow_home/memory，可用 root_override 替换；hashed 模式使用工作区身份子目录，flat 模式全局与工作区同根。
- [Memory repository identity](../specs/memory-search/spec.md#requirement-memory-repository-identity)：工作区身份 SHALL 优先从 Git origin 字符串提取去主机的仓库路径，再以 slug40 和 blake3 前8 hex 命名；无法取得时使用 canonical cwd，失败回退原路径。
- [Memory ASCII slug normalization](../specs/memory-search/spec.md#requirement-memory-ascii-slug-normalization)：slugify SHALL 小写后仅保留 ASCII 字母数字，其他字符变短横线、折叠连续短横线，截断并去两端短横线。
- [Memory ephemeral workspace writes](../specs/memory-search/spec.md#requirement-memory-ephemeral-workspace-writes)：hashed storage SHALL 根据系统临时目录及内置路径模式识别 ephemeral，跳过工作区初始化、长期写入、追加和日志写入。
- [Memory daily log lifecycle](../specs/memory-search/spec.md#requirement-memory-daily-log-lifecycle)：write_daily_log SHALL 使用 date-slug-session前8字节.md；append 且文件存在时追加分隔线和 UTC flush 注释，否则覆盖。
- [Memory long term overwrite and append](../specs/memory-search/spec.md#requirement-memory-long-term-overwrite-and-append)：长期 memory SHALL 按 scope 写 MEMORY.md；write_long_term 直接覆盖，append_to_memory 规范化后追加，已有非空文件前加两个换行。
- [Memory note heading normalization](../specs/memory-search/spec.md#requirement-memory-note-heading-normalization)：normalize_memory_content SHALL trim 输入，# 开头保留，单行转为标题，多行首行不超过80字节时提升为标题，否则使用 Note 标题。
- [Memory root confined reads](../specs/memory-search/spec.md#requirement-memory-root-confined-reads)：read_file SHALL canonicalize 文件和全局根并检查目标位于根下，再完整读取UTF8文本后应用零起始行范围。
- [Memory file enumeration and source classification](../specs/memory-search/spec.md#requirement-memory-file-enumeration-and-source-classification)：list_memory_files SHALL 列举全局和工作区 MEMORY.md，再列举 sessions 直接子项的小写md后缀，后者按路径排序。
- [Memory template initialization and clearing](../specs/memory-search/spec.md#requirement-memory-template-initialization-and-clearing)：ensure_initialized SHALL 在不存在时创建全局和工作区模板；clear_workspace 删除整个工作区根，clear_global 只删全局 MEMORY.md。
- [Memory conservative empty directory collection](../specs/memory-search/spec.md#requirement-memory-conservative-empty-directory-collection)：gc SHALL 跳过flat根和当前工作区，只删除满足年龄条件的空目录或仅含空sessions的目录；tmp前缀门限7天，其余使用max_age_days。
- [Memory chunk hash and short document preservation](../specs/memory-search/spec.md#requirement-memory-chunk-hash-and-short-document-preservation)：chunk_markdown SHALL 对空输入返回空，短于配置字节阈值的文本原样为一个块，块哈希使用blake3。
- [Memory heading chunks and context limits](../specs/memory-search/spec.md#requirement-memory-heading-chunks-and-context-limits)：大文本分块 SHALL 按宽松井号标题及行段拆分，加入父标题上下文，部分空行溢出分支附加字符尾部重叠。
- [Memory query keyword extraction](../specs/memory-search/spec.md#requirement-memory-query-keyword-extraction)：extract_keywords SHALL Unicode小写、按非字母数字且非下划线分割，过滤小于2字节、英文停用词和纯数字词，再按首次次序去重。
- [Memory SQLite schema and optional vectors](../specs/memory-search/spec.md#requirement-memory-sqlite-schema-and-optional-vectors)：memory index SHALL 使用meta、chunks和contentless FTS5表；扩展可用时创建指定维度的vec表，连接journal由sqlite-journal处理。
- [Memory vector space identity switching](../specs/memory-search/spec.md#requirement-memory-vector-space-identity-switching)：open_or_create SHALL 在Immediate事务内核对维度及endpoint/model身份，变化时重建vec表并更新元数据，保留chunks和FTS。
- [Memory content reindex transactions](../specs/memory-search/spec.md#requirement-memory-content-reindex-transactions)：reindex_file SHALL 按lossy路径加块序号标识，比较hash以新增、更新和删除chunk并维护FTS。
- [Memory index vector invalidation limits](../specs/memory-search/spec.md#requirement-memory-index-vector-invalidation-limits)：内容更新和路径删除 SHALL 尝试删除旧vec项，但忽略vec删除错误；文本与FTS主要变更使用事务。
- [Memory FTS source filtering](../specs/memory-search/spec.md#requirement-memory-fts-source-filtering)：FTS SHALL 将关键词以OR合并并按rank排序，source查询在限制数量之前过滤，空关键词或sources返回空。
- [Memory vector operation identity checks](../specs/memory-search/spec.md#requirement-memory-vector-operation-identity-checks)：向量读写 SHALL 在同一事务内核对当前cache身份；旧身份查询返回空，旧身份upsert返回错误。
- [Memory reindex claim lease](../specs/memory-search/spec.md#requirement-memory-reindex-claim-lease)：try_claim_reindex SHALL 原子取得空或时间严格早于cutoff的claim，记录pid和时间；SQL失败视为未取得。
- [Memory access and administrative reads](../specs/memory-search/spec.md#requirement-memory-access-and-administrative-reads)：record_access SHALL 增加访问次数并更新时间；all_indexed_paths 返回排序去重路径，get_chunk按ID读元数据。
- [Memory watcher dirty path handoff](../specs/memory-search/spec.md#requirement-memory-watcher-dirty-path-handoff)：watcher SHALL 递归收集Create/Modify/Remove事件中的小写md路径，以ArcSwap RCU去重并原子换空。
- [Memory endpoint credential capability](../specs/memory-search/spec.md#requirement-memory-endpoint-credential-capability)：EmbeddingEndpoint SHALL 私有绑定URL及凭据；静态key拒绝空白，live能力要求与进程配置的非loopback HTTPS完整URL一致。
- [Memory endpoint URL policy](../specs/memory-search/spec.md#requirement-memory-endpoint-url-policy)：嵌入端点 SHALL 拒绝userinfo、query、fragment，仅允许HTTPS或识别为loopback的HTTP，并追加embeddings路径段。
- [Memory embedding provider credential resolution](../specs/memory-search/spec.md#requirement-memory-embedding-provider-credential-resolution)：make_provider SHALL 在模型存在且非空时建立provider，Auth优先于动态key；动态key异步读取一次后绑定本provider。
- [Memory embedding request retries](../specs/memory-search/spec.md#requirement-memory-embedding-request-retries)：embed_batch SHALL 顺序分32项批次发送model/input/dimensions，send错误、429及5xx最多总尝试3次，间隔1秒和2秒。
- [Memory embedding response pairing](../specs/memory-search/spec.md#requirement-memory-embedding-response-pairing)：嵌入响应 SHALL 按data数组顺序取得embedding数组并过滤非数字元素，转f32；不核对index、结果数或维度。
- [Memory embedding backfill helper](../specs/memory-search/spec.md#requirement-memory-embedding-backfill-helper)：embed_missing_chunks SHALL 查询全部缺失项后逐32项批次嵌入，逐条写入并返回成功次数，批失败或写失败警告后继续。
- [Memory deterministic embedding test support](../specs/memory-search/spec.md#requirement-memory-deterministic-embedding-test-support)：test-support SHALL 暴露MockEmbeddingProvider，以blake3字节循环除255构造给定维度向量。
- [Memory hybrid candidate collection](../specs/memory-search/spec.md#requirement-memory-hybrid-candidate-collection)：hybrid_search SHALL 收集max_results三倍的FTS候选并额外查询global/workspace，再以chunk ID合并；向量可用且有provider时嵌入查询。
- [Memory hybrid score combination](../specs/memory-search/spec.md#requirement-memory-hybrid-score-combination)：搜索 SHALL 对FTS候选相对归一化、向量按clamp(1-distance/2)计分；两者均正时取加权和与FTS分的较大值。
- [Memory temporal and access ranking](../specs/memory-search/spec.md#requirement-memory-temporal-and-access-ranking)：搜索 SHALL 对非global/workspace来源按创建时间半衰减，再乘source权重和1+0.05*ln1p(access_count)。
- [Memory empty content search filter](../specs/memory-search/spec.md#requirement-memory-empty-content-search-filter)：搜索 SHALL 排除结构上只有空白、井号标题及完整HTML注释的块，evergreen另排除短scaffold标记文本。
- [Memory MMR diversity ordering](../specs/memory-search/spec.md#requirement-memory-mmr-diversity-ordering)：mmr_rerank SHALL 用小写snippet词集合Jaccard和独立raw relevance贪心重排，再由搜索截断结果数。
- [Memory search output and soft failures](../specs/memory-search/spec.md#requirement-memory-search-output-and-soft-failures)：搜索merge SHALL 返回完整chunk文本及来源行范围，底层向量查询错误或无法取得chunk时跳过，不设置snippet长度预算。
- [Memory backend session factory](../specs/memory-search/spec.md#requirement-memory-backend-session-factory)：from_session_params SHALL 配置会话ID、search参数、成对embedding配置及能力、可选watcher与诊断source，每实例建立计数器。
- [Memory backend sync on search](../specs/memory-search/spec.md#requirement-memory-backend-sync-on-search)：backend search SHALL 在dirty且取得claim时取出路径，存在则重建、不存在则删索引；向量空间可用时另尝试回填缺失项。
- [Memory backend embedding backfill ordering](../specs/memory-search/spec.md#requirement-memory-backend-embedding-backfill-ordering)：backend SHALL 先完成所有回填批次并积累upserts，写回后释放claim，再执行FTS与查询向量搜索。
- [Memory backend access and diagnostics](../specs/memory-search/spec.md#requirement-memory-backend-access-and-diagnostics)：backend SHALL 为返回项记录访问（失败忽略），记录空或非空搜索事件，成功走到末尾才递增搜索计数。
- [Memory backend read and count contracts](../specs/memory-search/spec.md#requirement-memory-backend-read-and-count-contracts)：backend get SHALL 委托根限制文件读取，total_chunks以journal-aware只读连接统计，默认搜索参数访问器返回保存配置。
- [Memory dream lock best effort ownership](../specs/memory-search/spec.md#requirement-memory-dream-lock-best-effort-ownership)：DreamLock SHALL 用PID正文与mtime写后复读协调，活PID且未超龄拒绝，失效或超龄可重取。
- [Memory dream lock rollback and timestamp](../specs/memory-search/spec.md#requirement-memory-dream-lock-rollback-and-timestamp)：DreamLock SHALL 支持记录整理时间及回滚；无prior删除锁，有prior清空PID并恢复mtime。
- [Memory dream eligible session scan](../specs/memory-search/spec.md#requirement-memory-dream-eligible-session-scan)：sessions_since SHALL 列举直接小写md条目，按mtime严格晚于cutoff筛选，并排除stem以后缀匹配的当前session。
- [Memory dream gate order](../specs/memory-search/spec.md#requirement-memory-dream-gate-order)：check_dream_gates SHALL 依次检查enabled、距锁mtime整小时间隔和会话数量，无锁使用epoch，IO失败返回Error。
- [Memory scaffold template predicate](../specs/memory-search/spec.md#requirement-memory-scaffold-template-predicate)：is_scaffold_template SHALL 要求trim文本小于500字节且含三个固定标记之一。
- [Memory dream prompt and input budget](../specs/memory-search/spec.md#requirement-memory-dream-prompt-and-input-budget)：build_dream_user_message SHALL 在会话前加入非scaffold现有记忆，现有记忆截至16000字节UTF8边界；会话整份追加后检查32000字节阈值。
- [Memory dream model instruction seam](../specs/memory-search/spec.md#requirement-memory-dream-model-instruction-seam)：DREAM_SYSTEM_PROMPT SHALL 请求合并旧知识、解决矛盾、绝对日期、删除短期噪声和保留决策；模型请求由宿主完成。
- [Memory dream response acceptance](../specs/memory-search/spec.md#requirement-memory-dream-response-acceptance)：process_dream_response SHALL 拒绝空、NO_REPLY及无标题子串响应，随后按Unicode字符截至16000。
- [Memory dream execution outcomes](../specs/memory-search/spec.md#requirement-memory-dream-execution-outcomes)：execute_dream SHALL 取得锁后处理现成response，有效则覆盖workspace MEMORY.md并清理processed日志，写失败尝试rollback。
- [Memory dream cleanup eligibility](../specs/memory-search/spec.md#requirement-memory-dream-cleanup-eligibility)：成功整理 SHALL 只尝试删除processed_stems中超过5分钟未修改的日志，并返回真正删除的stems；删除失败不改变Completed。

## 边界

- 不立即创建目录；flat 模式不启用 ephemeral 跳过。
- 身份不含主机，可能共用记忆目录；hash8 不提供无碰撞保证。
- 可能返回空串；工作区命名调用方另回退 workspace。
- 全局写入仍执行；跳过的工作区写操作返回成功而非持久化证明。
- 本层不净化路径组件，字节8切片可能不是字符边界；输入约束由调用方承担。
- append 不创建文件；这些方法不自动更新索引，也不提供跨进程写入事务。
- 得到纯标题，搜索结构过滤可将其排除；不能承诺追加后所有内容可搜。
- 本层允许；范围读取会规范化换行，默认完整读取保留原文，不以索引成员资格授权。
- 同一MEMORY.md可重复，sessions不检查is_file；classify_source是词法位置分类，不是授权。
- 删除flat根整体；缺失返回false，不自动停后台任务或协调打开的数据库。
- 正常读取时保留，不因tmp名称或年龄而删除；进程Mutex不等于跨进程写入租约。
- 实际比较UTF8字节数；行范围零起始末尾排他。
- 不保证硬长度上限或围栏完整，上下文不从预算扣除；不是完整Markdown解析器。
- 可能保留，不执行词干还原或中文分词。
- 保留FTS模式；SCHEMA_VERSION常量本身不实施自动迁移。
- 不使已有向量失效；扩展不可用时不重标vec身份。
- 不可读返回零变化保留旧索引；hash相同跳过全部字段，source也不更新。
- 不能承诺三表全部失败一起回滚；现存chunk预读也不在写事务内。
- 跳过该条；普通limit使用checked转换，source版本使用as i64。
- 本层不比较chunk hash或确认chunk仍存在；身份检查不是内容版本检查。
- release_claim不检查owner，无续租或PID存活检查。
- get_chunk行查询错误可变None，claim读取错误变空，paths坏行跳过；不能以空值证明数据库健康。
- 订阅失败警告返回None，运行时错误忽略；目录事件不展开子文件，返回路径无序。
- 拒绝复用live凭据；能力建立后环境变化不重定向既有端点。
- HTTP客户端不跟随重定向；配置连接超时30秒，不额外设置整个响应读取超时。
- 不在每个批次重新执行key命令；Auth认证中间件另按请求取得凭据。
- 直接错误，不按瞬态状态重试；空输入返回空。
- 调用方zip按位置配对，不能保证与输入一一对应；后续批失败使整个调用返回Err。
- zip只处理配对项，不把剩余项报告为成功。
- 它不保证单位范数或语义相似度，不能替代真实模型质量测试。
- FTS错误转空，嵌入错误警告后降级；evergreen补充不是最终结果配额。
- 使用完整FTS分，不受text_weight惩罚；仅向量使用vector_weight。
- 仍按未截断原始分排序；min_score比较截断显示分，未来时间年龄视0。
- 同样会被排除；未闭合HTML注释作为文字保留，不按完整Markdown语法识别。
- 原序返回；启用路径要求relevance长度一致，重排保留所有字段和结果总数。
- 可返回空成功；相同raw分无固定chunk ID排序保证。
- 覆盖保存的这两项，其余搜索参数保留；索引分块使用默认配置。
- 失败路径不重新放回dirty集合；取消无RAII释放claim，依赖过期重取。
- 仍尝试回填；provider不可得则跳过嵌入并释放已取得claim。
- 诊断mode仍可能是hybrid；duration不含前置打开及回填，不能据mode证明向量命中。
- backend返回错误；MemoryStorage的同类统计返回0，两者不可混为同一失败契约。
- 不保证严格互斥，无独立owner token或自动续期；Unix和Windows存活探测权限失败处理不同。
- rollback不核对owner；record_consolidation仍保留当前PID正文。
- 空串排除全部；本层不检查is_file，结果按stem排序而非修改时间。
- 仅代表门控通过，未取得锁；未来mtime产生零间隔。
- 仍可判为scaffold；达到500字节不按该谓词过滤。
- 仍完整读取和追加，阈值不是内存硬上限；processed_stems只含成功追加项，无可读会话返回None。
- 本包不验证事实正确或旧知识完整，提示词不是语义验收保证。
- 检查先于截断，最终文本未重新验证标题；标题检测非锚定。
- 返回NothingToConsolidate并保留新锁mtime，不清理日志；rollback失败不改变原Failed结果。
- 元数据失败仍尝试删除，无hash或租约验证；未处理文件保留不保证下一轮mtime门控会重新纳入。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 完整逐文件阅读记录

# memory 逐文件审阅记录

状态：进行中，不能据此标记全包 reviewed。2026-09-07。源根 crates/codegen/memory。已完整阅读 manifest、lib.rs、text_utils.rs、chunker.rs、schema.rs、query_expansion.rs、mmr.rs、watcher.rs；embedding.rs 读至 510 行，后续测试及其余模块待审。本轮未运行动态测试。

## 已核对事实

- lib：14 个 public module，公开 backend、endpoint、index、storage。embed_missing_chunks 先取全部缺失向量的 chunk，查询失败警告并返回 0；每批 32，批失败后继续，成功结果按 zip 配对，各条 upsert 失败只警告，返回成功写入数。没有在此核对响应数量。实验开关注释尚未核对宿主，不作为已证实契约。
- text_utils：has_markdown_headers 是非锚定字符串 contains，不解析 Markdown；is_no_reply 先 Unicode lowercase 再保留 alphanumeric，与 noreply 精确比较。
- chunker：blake3 文本哈希；短文本按 UTF-8 字节长度与 max_chunk_chars 比较，原样返回，行号从 0 起、末尾排他。长文本按标题分段再按行/段落处理；header_level 允许缩进和超过 6 个井号，井号后需 ASCII 空格或为空，不识别围栏状态。父标题加入 Context 前缀但不从长度预算扣除。长文本 lines/join 会规范化换行。单个超长行不会内部切分，max_chunk_chars 不是硬上限；overlap_chars 只在空行溢出分支按 Unicode 字符取尾部，不是所有后续块都有重叠。来源行范围不包含新增上下文/重叠的额外文本。preserves_code_blocks 测试用例短到直接返回，不能证明超长围栏保持完整。
- schema：SCHEMA_VERSION=1；meta、chunks、contentless FTS5 chunks_fts；chunks 包含路径、行范围、文本/哈希/source、创建更新时间、访问次数及末次访问时间，path/hash 有索引；预置 reindex_claim 空值。vec_available 才生成 FLOAT[dimensions] chunks_vec；schema_sql 自身没有维度校验或连接 PRAGMA，连接行为需继续核对 index。3 个测试只核对 SQL 字符串。
- query_expansion：Unicode lowercase，按非 alphanumeric 且非下划线分割，长度过滤使用 str.len 字节数 >=2，因此单个多字节字符可保留。过滤固定英文 stop words 和全 numeric token，按首次出现顺序去重；不做词干或中文分词。下划线被保留，全下划线可成为词。空结果的实际调用方降级待 search 核对。14 个测试覆盖常规英文、数字、大小写和标识符，没有多字节长度断言。
- mmr：禁用、<=1 项或 lambda==1 原样返回，且这些分支不检查 relevance 长度；启用重排才 assert 长度一致。snippet 小写后分词保留下划线，不过滤停用词。Jaccard 双空集合=1，单空=0。以单独 relevance 的 min/max 范围归一化，EPSILON 保底，按 lambda*相关性-(1-lambda)*与已选项最大相似度贪心选择；相同 MMR 优先较高原始 relevance，再同分保持当前次序。保留全部结果和各字段、不删除重复项、不改 score；调用方 relevance 数组不会同步重排。函数本身未验证 lambda/NaN。文档写 O(n²)，循环候选再遍历全部 selected，不能把该注释直接当精确复杂度契约。测试包含 unclamped relevance 排名、大小写重复、结果数与字段保留、Jaccard，不覆盖非法配置。
- watcher：recommended_watcher 递归订阅指定目录，建立或订阅失败警告并返回 None；运行中 notify Err 静默忽略。只接收 Create/Modify/Remove，事件各路径扩展名须严格等于 md；不主动扫描目录、不归一化路径、不限 dirty 集合大小。ArcSwap RCU clone+insert 去重，take_dirty 原子换空并返回无序路径；is_dirty 读同一集合快照。目录删除事件若只有目录路径，不会在此展开其子文件。消费者重建/删除索引行为仍待 backend 核对。并发测试确定性覆盖 RCU 与 take 交错；OS 测试可能因 watcher 创建失败直接早退，starts_on_valid_dir 没有成功断言，不能把测试名当平台订阅成功证据。

## embedding 已读生产路径（测试未读完）

- Endpoint 将 URL 与 Auth/DynamicApiKey/StaticApiKey 绑定为私有字段。静态 key 仅用 trim 检查是否空，保存原串；模型只拒绝 None/空串，不拒绝纯空格。cache_identity 为实际 request URL、model、dimensions 的 JSON，不含 key。
- parse_base_url 要求主机，HTTPS 或 loopback HTTP，拒绝用户信息、query、fragment。live 读取 GROW_CLI_CHAT_PROXY_BASE_URL，标准 URL 解析后精确比较整个 URL，额外要求 HTTPS、非 loopback；不是仅同域判定。Auth 优先于 DynamicApiKey。能力创建后环境变化不重定向现有 URL；embeddings 作为路径 segment 追加。IPv6 host 表示是否符合 is_loopback_host 尚未实测，不做扩展结论。
- make_provider：Auth 使用每请求认证中间件；DynamicApiKey 在创建 provider 时异步读取一次 key，再构造静态中间件；已有 provider 不持续向 DynamicApiKey 拉取更新。共享 OnceLock HTTP client：connect timeout 30 秒，禁止 redirect，process UA；本层未设置整个请求/读取时限。
- embed_batch 空输入返回空，顺序批次上限 32，发送 model/input/dimensions JSON，附 Content-Type、X-Grow-Token-Auth 和版本头。MAX_RETRIES=3 是总共 3 次尝试，实际等待 1 秒、2 秒，没有注释描述的第 3 次 4 秒等待。send 错误、429、5xx 会重试；其它非成功状态直接错误。成功响应 JSON 或字段解析错误直接返回，不重试。
- data 按返回数组顺序处理，忽略 index；embedding 数组只 filter_map 数字转 f32，未检查条数、维度、有限性。后续索引写入是否拒绝仍待核对。任何后续批失败则整个函数 Err，已积累数组不返回。Mock 仅 cfg(test/test-support)，blake3 字节循环除以 255 生成给定维度，不代表真实语义向量。
- 已读测试 static_key_is_bound_to_the_exact_request_url；cache_identity 测试仅读到中段 510 行，下次从 511 行继续，不宣称测试区完成。

## embedding 全文完成；index 全文完成

本轮读完 embedding.rs 511–742、index.rs 1–1365；没有运行动态测试。其余 backend/storage/search/dream/dream_lock 待审。

embedding 测试核对：静态 endpoint 的实际 HTTP 路径/认证头、缓存身份排除凭据且绑定 model/dimensions、URL 精确匹配、动态 key 异步读取一次、无凭据/不可信 live 拒绝、redirect 目标 200ms 内无连接、403 错误不保留响应体、Mock 稳定性等。动态 key 测试只构造 provider，不发送远程 HTTPS；未见请求数量/索引顺序/非数字/错误维度/重试次数回归。cache_identity 的 /v1 和 /v1/ 经追加路径后相同，不等于 live endpoint 接受这两个 base URL 互换。

index：

- init_sqlite_vec 用 Once 注册全局扩展，sqlite3_auto_extension 返回码未检查；open_or_create 不自动调用它。父目录创建失败忽略后交给打开数据库报错；JournalMode 决定实际数据库路径和连接配置，vec_version 探测失败只进程内警告一次。
- schema 建立和 vector identity 更新同一 Immediate 事务；identity 为 dimensions:identity。vec 可用且有身份且不一致时 drop/recreate vec 表，更新 identity/dimensions 元数据，不删 chunks/FTS。identity=None 的句柄不使已有向量失效；扩展不可用时不改 vec 元数据。SCHEMA_VERSION 常量没有在该路径检查或执行迁移。vec_available() 只检查启动时探测结果和本句柄身份存在，不实时核对当前数据库身份。
- reindex_file 读取 UTF-8 文本失败警告并返回零变化，保留旧索引。chunk ID 使用 lossy path 加枚举序号，未做路径 canonicalize 或验证 source 值。已有 chunk 在事务外预读；同 ID 同 hash 全跳过，未同步行范围/source。内容变更更新文本/hash/行范围/updated_at，保留 created_at/access_count/source；新增记录 source，删除消失的 ID。文本与 FTS 的主要写入受事务保护，但旧向量删除错误被忽略，不能承诺三表任何失败都原子拒绝。变更分支 rowid 查询失败被 .ok() 吞掉，可跳过新 FTS 插入。并发窗口待调用方约束核对，不把预读自动视为完整快照。
- FTS 提取关键词用 OR 连接，按 SQLite rank 升序，空关键词返回空；按 source 查询时先 JOIN/IN 过滤再 limit，空 sources 返回空。普通 search_fts 用 sqlite_integer 拒绝超 i64 的 limit，by_sources 用 as i64，边界不同。resolve_fts_rowids 对单条 id 查询错误跳过，可能返回少于候选数量。
- get_chunk prepare 失败返回 Err，但 query_row 的错误都 .ok() 变 None；行范围 i64 as usize。all_indexed_paths DISTINCT ORDER BY path，逐行读取失败跳过；get_reindex_claim 任何查询错误变空串。record_access 增量并更新 last_accessed，对不存在 ID 也返回成功。
- chunks_without_embeddings、vector_search 在读事务内核对数据库 identity，失配返回空；前者 LEFT JOIN vec 内部 chunks_vec_rowids，取全量无 embedding 文本；vector_search 转小端 f32 bytes，用 checked k 传 KNN。upsert_embedding 身份失配返回 InvalidQuery；匹配时直接 INSERT OR REPLACE，维度/数值是否接受交由 sqlite-vec，本层无预校验。也不检查 chunk 存在或当前文本 hash，因此身份保护不能表述为内容版本保护。扩展不可用时 upsert 无操作成功。
- try_claim_reindex 原子 UPDATE 空 claim 或时间戳严格早于 cutoff，值 pid:秒；SQL 失败警告并 false。不检查 PID 存活、不续租；阈值运算未显式检查溢出。release_claim 无持有者比较，直接清空且忽略错误。相关测试注释称 i64::MAX 让所有旧 claim 过期，与 now-threshold 相反；测试实际只证明空 claim 可取得、60 秒新 claim 拒绝、释放后可重取。0 阈值也不是无条件偷取同秒 claim。
- delete_path 按同样 lossy path 查已有记录，再事务删 FTS/chunks，vec 删除错误忽略；空记录返回 0，其他文件不受正常路径删除影响。不是直接文件删除操作，不自动删除磁盘内容。
- 测试覆盖 vector 身份切换拒绝旧句柄、FTS-only 保留 vec、正常新建/重开、显式 Truncate 接缝和本地 WAL、重建增删改/无变化、FTS、访问次数、delete 幂等、孤儿模拟维护、claim 基础流程。WAL 测试遇环境覆盖直接早退；网络文件系统测试使用本地显式 JournalMode，并非真实网络挂载。append/maintenance 测试直接调用 index 或复制调用逻辑，不能据此证明 CLI/TUI 实际接线。未见向量删除故障注入、并发文本版本更新、超限 source limit 的动态覆盖。

## storage 全文完成

完成 storage.rs 1–1897 源码及全部测试阅读，未运行动态测试。

- new 使用 override 或 grow_home/memory 根，延迟建目录；默认 workspace 是 slug-hash8 子目录，new_flat 则 global/workspace 同根且永不 ephemeral。with_paths 仅 test/test-support。原始 cwd 另存供显示。
- 工作区身份优先 git2 discover 后 origin URL，简单字符串去协议/主机得到路径，不保留服务器 authority，不区分同主机不同协议。不同主机同 org/repo 会共享身份；deep path 全保留，大小写不归一化；先 trim_end_matches(.git) 再尾斜杠，repo.git/ 的结果与 repo.git 不同，不是完整 URL parser。无身份时 canonical cwd，失败警告后 raw 路径；取 basename slug40 与 blake3 前8 hex，不保证无碰撞。slugify 只留 ASCII 字母数字，其他字符成短横线并折叠，截断后 trim；全中文可空，workspace 命名才回退 workspace。
- ephemeral 仅 hashed 模式按 canonical temp_dir 前缀、raw/canonical 若干 Unix/macOS 临时路径模式判断，不检查路径存活租约。它跳过 workspace init/longterm/append/daily，返回成功；global 写入仍生效，读取、clear、gc 不受该字段统一禁用。
- daily 文件名 date-slug-sid8.md，由输入直接拼接；sid8 按字节 slice，任意 Unicode session_id 在第8字节非边界可 panic；本方法不净化 date/slug/ID 路径分隔符，调用方输入约束待 backend/dream 核对。ephemeral 判断发生在文件名计算后。append 且 exists 时 OpenOptions append，插入分隔线及 UTC HTML flush 注释（不是注释文档所说 heading）；首次或 append=false 直接 fs::write。无独占锁、临时文件 rename、fsync 或大小上限，不保证并发多段写入原子。
- write_long_term 建 scope 目录后直接覆盖 MEMORY.md；append normalize 后空串不建文件，非空 create+append，已有长度>0 插入两个换行。normalize trim，任意 # 开头原样返回（不验证 heading），单行全部作 ##；多行第一行按字节 <=80 作标题，否则 ## Note 包住全部。以上方法不自动更新索引。
- read_file canonicalize 目标与 global root，两者必须存在，目标须 starts_with canonical global，再读取 canonical 路径。边界是整个根，包括其他工作区子目录，非仅当前 workspace；无 .md 类型检查。路径 canonicalize 后再次按路径打开，不能把注释“防止 TOCTOU”提升为消除所有中间目录替换竞态。先读取整个 UTF-8 文件，再按 from/count 截取；完整默认返回原串，切片经 lines/join 规范化换行并去尾换行，零 count 返回空。
- list_memory_files 仅 global MEMORY.md、workspace MEMORY.md、workspace sessions 直接子项且小写 md，session 按路径排序，非递归扫描全部 root。flat root 会重复加入同一 MEMORY.md；session 条目不验证 is_file，.md 目录也能进入列表；entry 读取错误过滤。classify_source 为词法 starts_with，workspace 优先，其中任意 basename MEMORY.md 为 workspace，其余 session；global 根下其他路径 global，根外默认 session，非授权检查。
- ensure_initialized 先全局目录及不存在时写模板，再 ephemeral 早退，再工作区模板；存在检查+write 非原子 create_new。flat 模式同文件不会另写 project 模板。clear_workspace remove_dir_all 整个 workspace 根（flat 即整个 flat root）；clear_global 仅 remove_file MEMORY.md；缺失 false，其他错误返回，不做索引/后台任务停机协调。
- total_chunk_count 通过 JournalMode 对工作区 index.sqlite 只读打开，缺失/查询失败变0，不创建文件。
- gc flat 根返回0；进程全局 Mutex 只串行本进程 gc，poison 恢复。遍历 root 所有目录，不验证 slug-hash 格式，词法跳过当前目录。空目录或仅空 sessions 视为空；任何其它持久条目保留，tmp 前缀也不能越过该空检查。tmp* 7天，其他 max_age_days，按目录 mtime 严格大于；未来mtime=零龄，读取metadata失败不删。root entry错误flatten忽略，is_empty_workspace内部条目错误也flatten，不能宣称所有目录读取失败均保守保留。remove_dir_all错误仅debug，计数只算成功。无跨进程写入租约；days乘秒未显式检查溢出。
- 测试完整覆盖常规日志覆盖/追加、行片段、读根外拒绝、模板幂等、标准normalize边界、append、clear重建、远端SSH/HTTPS/深路径、git2真实临时origin和子目录身份、ephemeral写入跳过、GC空/非空/龄阈值/flat/current、只读计数缺失不建库。未见任意Unicode session ID、不同host身份分离、flat重复列表、symlink/竞态、故障注入或并发写入测试。GC current 测试同时放 MEMORY.md，单靠该测试不能区分 current 跳过与非空保护，但源码有显式 current 分支。

## search 全文完成

完成 search.rs 1–1356 全部实现与测试阅读，尚未运行动态测试。

- hybrid_search 候选数为 max_results*3，未 checked multiplication。先普通 FTS，再单独 global/workspace FTS 各同限，按 chunk_id 补入普通结果没有的 evergreen；不是预留最终配额，合并后仍可被过滤/排序淘汰。两次 FTS 错误均变空结果。provider 仅在 index.vec_available 时请求 query 的 embedding，空数组视为无向量，非空只取第一个；错误警告后用 FTS。无额外查询总时限。index 引用跨阶段使用，注释 Send 论证不能替代实际 future 类型编译证明。
- merge 向量查询错误转空，按 chunk_id 汇合。FTS 相对当前候选 min/max 归一化，最强=1，全部同分=1，最弱在非同分时=0；因此“FTS存在”不等于分数>0。向量固定 clamp(1-distance/2,0,1)，依赖单位向量解释但本层不归一化输入。
- 两分数均>0 时 max(text_weight*fts+vector_weight*vec,fts)；仅正FTS用全FTS，其他用vector_weight*vec。不是简单两信号加权平均，也不实际改 config.text_weight。source weight 未设置默认为1；raw_score=base*decay*source_weight*(1+ln1p(access_count)*0.05)。显示分clamp0..1后与min_score比较，按未截断raw排序，MMR消费同样raw relevance，最后truncate。同分来自HashMap遍历，无固定chunk ID次序。snippet 是完整chunk文本，无额外字符/token截断。这里没有 record_access，访问计数更新需核对 backend。
- evergreen 仅 global/workspace 不衰减；所有其它 source 在有效half-life下按 created_at 衰减。None/<=0禁用，created_at负数按0、未来年龄按0，无最大年龄上限；读取 effective_half_life_days，具体配置行为已有 config-types 记录，后续需映射来源。不是按 updated_at 或 last_accessed 衰减。
- get_chunk 查询失败或不存在跳过；merge正常路径最后总是Ok，多个底层失败可能呈现空成功。过滤发生在搜索不是索引层：所有source的纯空白/ATX标题/完整HTML注释被视为空，未闭注释保留。并非Markdown解析，使用chunker宽松header_level，HTML删除不识别代码上下文；引用正文保留，setext标题也视正文。evergreen额外套用 dream::is_scaffold_template，具体阈值待dream阅读。只包含标题的用户笔记也被过滤：这与 append_to_memory 把单行笔记全部转成标题的路径存在交互，需在backend接线后单独记录，不可称“所有用户笔记均可搜”。
- 测试覆盖FTS空/命中/限数、vec+FTS、时间衰减、raw访问加权、MMR接线、FTS无vec不惩罚、模板过滤及显示分上限。source_weights 测试有双重条件断言，缺结果也可能通过，且注释仍说workspace更高但断言期望相等。vector_absolute_normalization 混入正FTS，单凭score>0.1不能独立证明向量贡献（max保底FTS）；mock4维不等价真实高维单位向量。模板过滤回归有raw FTS候选和真实结果对照，非空断言充分。MMR接线测试不能独立证明读取raw而非display，独立mmr单测才有反序输入证据。

## backend 生产路径完成，测试读至 660 行

本轮读取 backend.rs 1–660。生产实现全部读完；factory_tests 从头到 search_source 传播测试已读，剩余测试仍需补齐，包继续 pending。

- Params 集中 session_id、embed config/endpoint、search config、watcher、claim 阈值和诊断 source，工厂按 workspace/index.sqlite 构建；只有 config/endpoint 都存在才配嵌入，只有 watcher 存在才复制其 stale 秒数。每个工厂实例创建独立 Arc 搜索计数器，宿主是否共享需看后续包。
- backend 每次 search 打开可写 index，default MemoryIndexConfig，不携带外部 chunk 配置；嵌入维度默认1024或配置值，身份由endpoint/config生成。不会在这里 init_sqlite_vec。new 文档说必须已有数据库，但 search 实际 open_or_create 可以新建；模块顶部 !Send 注释与生产函数内 Send/!Sync 说明矛盾，以实现/编译为准。
- watcher dirty 且成功取得 claim 才 take_dirty；claim失败保留集合。取出后逐路径 exists 决定重建/删除，重建或删除错误不重投dirty集合。root全树事件由classify_source处理，未另做当前工作区过滤。changed_chunk_count 包含 added/updated/removed 和 delete计数。watcher时间包含随后的嵌入等待。
- 即使没有dirty，vec_available也尝试claim并取全部缺失向量以回填新空间。provider在获得claim及读取后异步构造；每32批调用embed_batch，失败警告继续，全部批次结果累积于upserts再逐条写回，zip不验数量，写失败不计数。没有RAII claim guard，await取消/任务panic会留下claim直到过期；lease不续租，释放无owner核对的影响仍存在。provider不可得也释放已取claim。
- 然后复制search配置，以调用参数覆盖max_results和min_score（f64转f32），其余权重/MMR/decay保留。FTS普通+evergreen补充路径与search.rs一致；query嵌入失败/空结果退FTS。mode使用“扩展+provider可用”决定，query失败仍可能诊断hybrid；不能用mode证明向量查询实际命中。总search duration从同步/回填完成后才开始，不含前面的open/watcher/backfill/provider构建。
- merge返回后逐条record_access，错误忽略；因此访问加权作用于后续搜索，不改变当前排序。空/非空事件记录query字节长度、keyword数、阈值、mode/source、耗时及结果量等，末尾Relaxed计数加1；open失败不递增。结果created_at封装Some。get直接委托storage根目录权限读取，不要求该文件已索引；total_chunks只读查询失败返回Err，与storage计数失败变0不同。default_search_*返回保存配置供调用方使用。
- 已读工厂测试明确直接断言session_id、max_results、MMR/decay和source被保存；search counter测试同时走正常FTS调用。这里尚未证明三个宿主入口全使用工厂，需宿主crate后续审阅。

## backend 与 dream_lock 全文完成；dream 读至 270 行

backend 补读 661–1488：watcher runtime/初始化顺序测试成功分支允许 None；删除集成提前 canonicalize 索引键和订阅根，无法代表所有调用方路径已规范化，且 watcher 创建失败或2秒内未dirty均提前返回。异步key测试断言每provider构建恰好一次async、零sync，不是请求中途轮换。模型切换无watcher回填测试用本地HTTP明确读取两份完整payload，断言先chunk再query，数据库missing清空、旧身份搜索空，具有实际接线证据。evergreen测试没有兑现注释所说“base候选已挤出”：只有7条记录而backend候选上限30，不能据此独立证明补充路径必要。chunks_without_embeddings测试未自己注册vec，允许不可用分支提前返回。未见取消后claim恢复、dirty失败重投、单行append搜索的专门回归。

dream_lock 全部488行：

- .dream-lock 的mtime同时作为上次整理时间；try_acquire看年龄整数秒，age<stale_secs且PID活则拒绝，死PID、超龄、空/无效/读取失败body均可写入当前PID并重新读取验证。它不是原子create/CAS，且同进程PID无法区分两个任务；无自动Drop释放/续期。年龄达到阈值即可重取，未来mtime按零龄。
- Unix kill(pid as i32,0)，ESRCH才死、其它错误视活，未拒绝零或超i32 PID；Windows OpenProcess失败直接视死（含权限失败），WaitForSingleObject零超时仅WAIT_TIMEOUT视活，随后关闭句柄。不是跨主机process身份。
- rollback无owner检查：prior=None删文件（缺失允许），Some清空body并恢复旧mtime。record_consolidation写当前PID刷新mtime，完成后仍会暂时阻止同活PID的新取得。last_consolidated_at对缺失None，其它metadata错误返回Err。
- sessions_since只直接遍历小写.md后缀，未检查is_file；按stem.ends_with(exclude)排除，不验证8字节或分隔符，Some空串排除全部。mtime严格>since，返回按名字排序stems，不按mtime排序；目录缺失空，其它entry/metadata错误传播。
- 测试覆盖空/损坏body、活/死PID、超龄重取、mtime恢复/刷新、基本生命周期、时间严格边界、会话后缀排除/排序/非md。未验证并发互斥；死PID用4000000000假定不存在，Unix转换实际负值探测，不能宣称所有平台死PID边界已覆盖。

dream.rs 1–270：

- gates顺序enabled、lock mtime间隔整小时、sessions_since数量；无锁文件用epoch，未来时间间隔0，IO错误DreamGate::Error。gate本身不取得锁，Open只携带eligible stems。
- system prompt要求合并/去冲突/绝对日期/去短期噪声/保留决策，NO_REPLY可跳过；这些是模型指令，不代表强制验证了语义质量。
- scaffold谓词trim后字节<500且包含三个固定marker之一，不解析模板结构。DreamResult区分eligible总数和真正cleaned stems。
- build_dream_user_message现有非空非scaffold memory最多取16000字节UTF8边界，但buffer capacity先按原existing长度预分配。session整文件read_to_string成功非空则整段追加，然后才检查总buf>=32000，因此32000不是硬读取/内存上限，超大单session可完整进入prompt。processed_stems只算确实追加的文件，IO/空内容跳过，至少一个session才Some；只有existing也None。输入名stem直接join，不在本函数验证路径。输出处理从271行起待审。

## dream 全文完成，源码阅读闭合

补读 dream.rs 271–1471，至此本包14个Rust模块及manifest全部已读。仍需功能映射、动态验证及哈希登记，尚未标reviewed。

- process_dream_response trim后拒绝空、NO_REPLY和没有非锚定header子串；随后按Unicode字符截到16000，先检查header再截断，若唯一header在截断段外也可能返回无header内容。不校验模型合并是否保全旧知识、不补标题。chars_written是字符数，区别input字节预算。
- execute_dream接收现成模型response，不发模型请求，不自己检查gates。取得锁失败/已占用分别Failed/Skipped且eligible=0；取得后无有效输出NothingToConsolidate，保留本次新mtime/PID且不清理，不rollback。写workspace MEMORY.md失败才尝试rollback，rollback错误忽略，日志仍说已回滚。成功后不另record_consolidation，留下的是取得锁的时间；ephemeral storage写入no-op仍可Completed和清理，调用方须限制使用场景。
- cleanup仅传入processed_stems：metadata/modified成功且年龄整数秒<300跳过（未来也跳过），否则尝试remove_file；metadata读取失败并不保守跳过。无文本hash或读取时mtime比对、无文件租约，5分钟保护不等于防止全部并发更新丢失。不存在不计，失败警告不改Completed；返回只真正删除stems，函数本身不清理索引。未处理的旧session虽然保留，下一轮gates只看本次lock mtime之后修改的文件，不能据“文件保留”承诺下轮自动纳入。
- 全部测试覆盖gates阈值/顺序、输入可读/空/缺失/已有memory、输出NO_REPLY/结构/字符数、写入覆盖/失败回滚、锁占用、清理成功/失败/近期保护/未处理保留和scaffold499/500字节。min_hours_boundary_exact_is_too_soon测试实际23h59m，不是24小时精确边界。input_size_cap测试明确断言>=32000，只证明停止后续会话；cap清理端到端是函数链加固定response，没有真实模型调用，也未再跑下一轮gates证明遗漏会话最终被处理。

## 动态测试完成，映射进行中

2026-09-07：CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p memory --all-features --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target -- --test-threads=1 退出0，296单测通过，0失败/ignored，doctest 0项。日志 /tmp/grow-memory-all-features-tests.log；早退型watcher测试的限制仍适用，不能从0 ignored推导所有OS断言执行。测试结束cargo clean --profile dev显式本工作树target，删除6539文件2.1GiB，target恢复8KiB，磁盘可用77GiB。

首批23条映射草稿保存在memory-features-draft.json，尚未包含embedding/search/backend/dream全量契约，不能代替正式feature-map，不标包完成。下一步补齐后一次性登记并删除中间草稿。

## 本包登记完成

14个Rust模块及manifest已完成阅读，51项契约已合入活动change的feature-map和delta，296项全特性单测通过。此前进行中状态为历史阅读记录；宿主接线仍在所属包继续核对。
