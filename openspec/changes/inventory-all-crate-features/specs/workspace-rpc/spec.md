## ADDED Requirements

### Requirement: Workspace typed RPC boundary
WorkspaceRpc SHALL 将Serialize请求与静态METHOD、Serialize+DeserializeOwned+Send响应类型关联；此包仅提供wire类型，不执行运行时、权限或传输封装。

#### Scenario: 方法覆盖
- **WHEN** 使用工作区API
- **THEN** 提供fs、git、worktree、hunks、search、code navigation、agents md、skills、hooks、workspace info与GitHub export方法；完整METHOD和response映射登记逐包review。

#### Scenario: 共享常量
- **WHEN** 构造MCP限定工具名
- **THEN** 分隔常量为双下划线；default和default-bazel均为空feature。

证据：`crates/codegen/workspace-types/src/rpc/mod.rs` — `WorkspaceRpc`；`crates/codegen/workspace-types/src/lib.rs` — `MCP_TOOL_NAME_DELIMITER`。

### Requirement: Workspace observed event encoding
WorkspaceEvent SHALL 以type/data相邻tag和snake_case编码FsChanged、CodebaseIndexUpdated、ToolsChanged，分别携带path+kind、files_indexed、session_id。

#### Scenario: 文件变化种类
- **WHEN** 解析FsEventKind
- **THEN** 只接受created/modified/removed/renamed。

#### Scenario: 职责边界
- **WHEN** 查询事件类型
- **THEN** 本枚举不提供prompt/tool/compaction/agent生命周期事件；不凭类型定义推断watcher已发送事件。

证据：`crates/codegen/workspace-types/src/events/workspace.rs` — `WorkspaceEvent`；`crates/codegen/workspace-types/src/events/workspace.rs` — `FsEventKind`。

### Requirement: Workspace service file transfer schema
put_files/get_files SHALL 分别承载files数组与逐文件results；PutFileEntry缺省create_dirs=true、append=false，GetFileEntry具有可选if_none_match、offset、length。

#### Scenario: 写入结果
- **WHEN** 序列化PutFileResult
- **THEN** 包含path/ok及可选error/hash，None省略；此类型不执行hash计算、root containment或事务处理。

#### Scenario: 读取结果
- **WHEN** 序列化GetFileResult
- **THEN** 包含path/exists、缺省false的matched和可省略content/hash/size/error；UTF8范围与缓存命中行为须由实现包验证。

证据：`crates/codegen/workspace-types/src/rpc/fs.rs` — `PutFileEntry`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `GetFileEntry`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `GetFileResult`。

### Requirement: Workspace shell filesystem schema
workspace.fs_* SHALL 提供list/exists/read/write/delete请求；请求字段为snake_case，节点及读取数据按camelCase并将类型字段命名为type。

#### Scenario: 列表缺省
- **WHEN** 从只有path的JSON读取FsListReq
- **THEN** depth=1、limit=1000、offset=0、hidden/follow_symlinks/respect_git_ignore=true，globs为空。

#### Scenario: 读取与写入缺省
- **WHEN** 读取FsReadFileReq或FsWriteFileReq
- **THEN** 读取max_bytes=1048576、encoding=utf8、offset/length=None且拒绝未知字段；写入create_dirs=true。预算clamp、symlink安全及字节编码并不在DTO中实现。

证据：`crates/codegen/workspace-types/src/rpc/fs.rs` — `FsListReq`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `FsReadFileReq`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `FsWriteFileReq`。

### Requirement: Workspace client filesystem schema
client_fs_list/stat/read_file SHALL 使用独立camelCase schema和固定宽整数；列表节点以type表示file/directory，时间字段mtimeMs，stat的可选种类字段为nodeType。

#### Scenario: 分页和读预算
- **WHEN** 反序列化最小请求
- **THEN** list depth=1、limit=1000、offset=0、三个过滤布尔true；read maxBytes=1MiB、utf8、offset/length=None，类型本身不拒绝超大正预算。

#### Scenario: 读取响应
- **WHEN** 序列化ClientFsReadFileRes
- **THEN** size/hash/type必需，content/contentBase64可省略；两者互斥和hash有效性只是调用方契约，struct本身不强制。text/binary及utf8/base64为闭合枚举。

证据：`crates/codegen/workspace-types/src/rpc/fs.rs` — `ClientFsListReq`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `ClientFsStatRes`；`crates/codegen/workspace-types/src/rpc/fs.rs` — `ClientFsReadFileRes`。

### Requirement: Workspace Git read and status schemas
Git读取接口 SHALL 提供status/files/diff/info/branches/root/current commit/VCS/branch info/metadata/collect changes请求；status输出使用严格format tagged structured或prompt枚举。

#### Scenario: 默认状态
- **WHEN** Rust Default或JSON缺省GitStatusExtReq
- **THEN** include_untracked和ignore_submodules=true，stats/patches=false，format=structured；GitFiles version=HEAD，GitDiff from=HEAD/to=working，patch/content/merge_base=false。

#### Scenario: 严格响应
- **WHEN** 解析GitStatusExtResponse
- **THEN** structured必须data，prompt必须prompt，未知额外字段和无format的平铺状态拒绝；into_structured对prompt返回None。

证据：`crates/codegen/workspace-types/src/rpc/git.rs` — `GitStatusExtReq`；`crates/codegen/workspace-types/src/rpc/git.rs` — `GitStatusExtResponse`；`crates/codegen/workspace-types/src/rpc/git.rs` — `GitDiffReq`。

### Requirement: Workspace Git mutation schemas
Git修改接口 SHALL 声明stage、stage content、unstage、discard、commit、sync base、checkout、stash、checkout commit请求及对应响应。

#### Scenario: 默认修改选项
- **WHEN** 反序列化请求缺省值
- **THEN** discard scope=both且include_untracked=false；commit amend/signoff/push/sync/stage_all/seed_default_excludes=false且expected_branch=None；checkout create、stash include_untracked、sync abort=false。

#### Scenario: 提交与同步结果
- **WHEN** 序列化结果
- **THEN** CommitResult含data/warning及可省略outcome，PushStatus为not_requested/skipped/ok/conflict/failed；sync outcome以kind表示up_to_date/merged/conflicts/aborted。类型不执行分支保护、push或merge。

证据：`crates/codegen/workspace-types/src/rpc/git.rs` — `GitCommitReq`；`crates/codegen/workspace-types/src/rpc/git.rs` — `GitSyncBaseReq`；`crates/codegen/workspace-types/src/rpc/git.rs` — `PushStatus`；`crates/codegen/workspace-types/src/rpc/git.rs` — `GitSyncBaseOutcome`。

### Requirement: Workspace Git change and repository data
Git状态与diff数据 SHALL 表达staged/unstaged、仓库和upstream元信息及文件变更；ChangeType为create/edit/delete/rename/copy/typechange/untracked，以type字段输出。

#### Scenario: 收集patch
- **WHEN** 调用GitDiffsData::collect_patches
- **THEN** 按files顺序过滤None并用单换行连接；没有Some返回None，Some空串仍参与。

#### Scenario: VCS辅助
- **WHEN** 使用VcsKind
- **THEN** 默认git，其他wire值jujutsuColocated/none；is_jj仅colocated为true，is_repo仅none为false，不访问文件系统。

证据：`crates/codegen/workspace-types/src/rpc/git.rs` — `GitFileChange`；`crates/codegen/workspace-types/src/rpc/git.rs` — `collect_patches`；`crates/codegen/workspace-types/src/rpc/git.rs` — `VcsKind`。

### Requirement: Workspace Git archive collection schema
GitCollectChangesReq SHALL 默认include_commits/include_uncommitted=true、max_file_bytes=0、base_ref=None和空force_include_paths；响应包含repo/head/publicBase/commits/uncommitted/untracked/warnings/totalSizeBytes。

#### Scenario: 归档内容类型
- **WHEN** 构造CommitWithPatchData或UncommittedChangesData
- **THEN** 携带base64命名patch、stats、binary条目和作者/提交者时间字段，空binary数组省略；DTO不验证base64或数据总量一致性。

#### Scenario: 未跟踪文件阈值
- **WHEN** 读取共享阈值
- **THEN** UNTRACKED_CONTENT_THRESHOLD为1048576；UntrackedFileData携带binary/size/truncated/contentIncluded及可选contentBase64，实际过滤和截断仍由实现决定。

证据：`crates/codegen/workspace-types/src/rpc/git.rs` — `GitCollectChangesReq`；`crates/codegen/workspace-types/src/rpc/git.rs` — `GitCollectChangesResponse`；`crates/codegen/workspace-types/src/rpc/git.rs` — `UNTRACKED_CONTENT_THRESHOLD`。

### Requirement: Workspace worktree creation schema
Worktree创建请求 SHALL 使用camelCase，copyMode缺省dirty；WorktreeType闭合小写linked/standalone/git，Rust默认linked，FromStr只接受精确小写值。

#### Scenario: 同步请求包装
- **WHEN** 序列化两种sync请求
- **THEN** WorktreeCreateSyncReq透明展开；CreateWorktreeFromWorktreeSyncReq保留inner，wire mirror不携带cancellationToken/resolvedDestPath。

#### Scenario: 创建状态
- **WHEN** 序列化CreateWorktreeResponse
- **THEN** status为creating或exists，sessionId/worktreePath必需，exists还含commit，sourceGitRoot=None时省略；METHOD的部分Response仍为Value，不强制此enum。

证据：`crates/codegen/workspace-types/src/rpc/worktree.rs` — `WorktreeType`；`crates/codegen/workspace-types/src/rpc/worktree.rs` — `CreateWorktreeRequest`；`crates/codegen/workspace-types/src/rpc/worktree.rs` — `CreateWorktreeFromWorktreeSyncReq`。

### Requirement: Workspace worktree management schema
Worktree管理 SHALL 声明remove/apply/show/gc/list/db rebuild/path/stats方法，DTO不直接删除或合并。

#### Scenario: 缺省与严格字段
- **WHEN** 解析remove/apply/list/gc
- **THEN** remove拒绝未知字段且force/dryRun=false；apply mode=overwrite，另有merge；list把types写作type数组，include_all=false；gc dry_run/force=false且max_age_secs可为负数。

#### Scenario: 返回结果
- **WHEN** 序列化worktree响应
- **THEN** apply以status区分success(files/gitRoot)与conflicts(files/conflicts)，冲突含base/ours/theirs可选文本；remove含removed和可省略resolvedPath，db path为可选path。

证据：`crates/codegen/workspace-types/src/rpc/worktree.rs` — `RemoveWorktreeRequest`；`crates/codegen/workspace-types/src/rpc/worktree.rs` — `ApplyWorktreeResponse`；`crates/codegen/workspace-types/src/rpc/worktree.rs` — `WorktreeGcReq`。

### Requirement: Workspace hunk action and query schema
Hunk接口 SHALL 声明单项、file、turn、all四种accept/reject动作及staged files/file summaries/all hunks/all file contents/session summary/filtered hunks查询。

#### Scenario: 单项包装
- **WHEN** 序列化HunkSingleActionReq
- **THEN** 外层action包含hunk_id和小写action；HunkActionResponse空struct编码为对象，不等于注释中的null。

#### Scenario: 汇总与来源
- **WHEN** 序列化HunkWire及summary
- **THEN** HunkWire使用camelCase且无selected，source以type编码agentEdit/externalEditOnAgentFile/external；agentEdit内部仍为prompt_index。汇总包含accepted/rejected统计、turns和pending数量，类型不保证数值相互一致。

证据：`crates/codegen/workspace-types/src/rpc/hunks.rs` — `HunkSingleActionReq`；`crates/codegen/workspace-types/src/rpc/hunks.rs` — `HunkActionResponse`；`crates/codegen/workspace-types/src/rpc/hunks.rs` — `HunkSourceWire`；`crates/codegen/workspace-types/src/rpc/hunks.rs` — `SessionSummaryWire`。

### Requirement: Workspace hunk content views
Hunk文件内容视图 SHALL 以missing/binary/tooLarge/lfsPointer/symlink/full闭合状态表示baseline/current，并支持可省略byteLen/content及isAgentFile/staged。

#### Scenario: 缺省和未知状态
- **WHEN** 使用Rust Default或解析未知status
- **THEN** Default为missing，未知状态拒绝；derive Default不意味着JSON省略status可用。

#### Scenario: payload边界
- **WHEN** 构造FileContentViewWire
- **THEN** status、byteLen和content之间没有额外一致性校验，内容读取或大小过滤在hunk-tracker实现核查。

证据：`crates/codegen/workspace-types/src/rpc/hunks.rs` — `FileContentStatusWire`；`crates/codegen/workspace-types/src/rpc/hunks.rs` — `FileContentEntryWire`。

### Requirement: Workspace content and fuzzy search schema
内容搜索 SHALL 使用camelCase pattern/flags/globs/limits/cwd/contextId，响应files与totalMatches/totalFiles/truncated；模糊搜索提供open/change/close，后两者关联bool响应。

#### Scenario: 默认差异
- **WHEN** JSON省略respectGitignore或Rust直接Default
- **THEN** JSON默认true，derive Default为false；其他搜索flags=false、globs为空、limits可选。ContentMatchFile::new从路径末段取name，取不到则用原路径。

#### Scenario: 通知路由
- **WHEN** 使用FuzzyOpenReq和TargetClientId
- **THEN** root/request_id/session_id可选，hidden=false，target为null或instanceId/connId对象；change dirs_only=false、limit=None，DTO不把注释100直接写入字段。

证据：`crates/codegen/workspace-types/src/rpc/search.rs` — `ContentSearchRequest`；`crates/codegen/workspace-types/src/rpc/search.rs` — `ContentMatchFile`；`crates/codegen/workspace-types/src/rpc/search.rs` — `TargetClientId`；`crates/codegen/workspace-types/src/rpc/search.rs` — `FuzzyChangeReq`。

### Requirement: Workspace code navigation schema
代码导航 SHALL 声明按file/line/col跳转definition/references、按symbol和可选context_file查找，以及index status五种请求，均允许可选root。

#### Scenario: 引用请求
- **WHEN** 省略include_definition
- **THEN** 默认false；line/col使用usize，不在类型层强制坐标起点或文件存在。

#### Scenario: 导航结果
- **WHEN** 返回CodeNavResponse或index status
- **THEN** locations包含path/line及可省略symbol；status包含active、可选file_count和files/definitions/references统计。

证据：`crates/codegen/workspace-types/src/rpc/code_nav.rs` — `CodeGotoReferencesReq`；`crates/codegen/workspace-types/src/rpc/code_nav.rs` — `CodeNavResponse`；`crates/codegen/workspace-types/src/rpc/code_nav.rs` — `CodeIndexStats`。

### Requirement: Workspace discovery wire mirrors
发现接口 SHALL 使用严格空请求声明agents md、skills、hook registry；AgentConfigFile、SkillInfo及HookRegistryWire/HookSpecWire拒绝未知字段。

#### Scenario: Skill元信息
- **WHEN** 解析最小SkillInfo
- **THEN** name/description/path/scope必需，enabled/user_invocable默认true，has_user_specified_description/disable_model_invocation=false；可选展示、paths、来源JSON、plugin身份/目录、allowed_tools/model/effort/body省略None。scope排序local<repo<user<server<bundled<plugin，未知scope拒绝。

#### Scenario: Hook镜像
- **WHEN** 解析HookRegistryWire
- **THEN** hooks映射15种snake_case闭合事件到spec列表，spec含name/event/handler_type/enabled/timeout_ms/on_failure/source_dir/extra_env/layer及可选matcher/command/raw/url；不包含编译matcher，也不执行发现和hook。

证据：`crates/codegen/workspace-types/src/rpc/agents_md.rs` — `AgentConfigFile`；`crates/codegen/workspace-types/src/rpc/skills.rs` — `SkillInfo`；`crates/codegen/workspace-types/src/rpc/skills.rs` — `SkillScope`；`crates/codegen/workspace-types/src/rpc/hooks.rs` — `HookSpecWire`。

### Requirement: Workspace identity and GitHub export schema
workspace.info SHALL 关联空严格请求与任意JSON Value；export_github关联project_dir及可选repo_full_name/branch/commit_message请求与repo_full_name/repo_url/branch/commit_sha/no_changes响应。

#### Scenario: 导出错误分类
- **WHEN** wire_code与from_wire_code互转
- **THEN** 覆盖repo_not_specified/invalid_repo_name/project_dir_invalid/auth_failed/push_rejected/timeout/git_failed七个gh_export前缀码，未知返回None。

#### Scenario: 执行边界
- **WHEN** 构造导出请求
- **THEN** 不访问GitHub、不校验仓库名或授权；成功提交和推送需实现包独立证据。

证据：`crates/codegen/workspace-types/src/rpc/workspace.rs` — `WorkspaceInfoReq`；`crates/codegen/workspace-types/src/rpc/export_github.rs` — `ExportGithubReq`；`crates/codegen/workspace-types/src/rpc/export_github.rs` — `ExportGithubError`。

