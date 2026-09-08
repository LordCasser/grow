# workspace-types 逐包核查

包路径：`crates/codegen/workspace-types`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读；cargo test --locked -p workspace-types --all-features退出101：50项通过，Hook fixture 1项失败，0忽略，doctest未到达。日志/tmp/grow-workspace-types-tests.log。

## 模块与开关

- `crates/codegen/workspace-types/Cargo.toml`
- `crates/codegen/workspace-types/src/events/mod.rs`
- `crates/codegen/workspace-types/src/events/workspace.rs`
- `crates/codegen/workspace-types/src/lib.rs`
- `crates/codegen/workspace-types/src/rpc/agents_md.rs`
- `crates/codegen/workspace-types/src/rpc/code_nav.rs`
- `crates/codegen/workspace-types/src/rpc/export_github.rs`
- `crates/codegen/workspace-types/src/rpc/fs.rs`
- `crates/codegen/workspace-types/src/rpc/git.rs`
- `crates/codegen/workspace-types/src/rpc/hooks.rs`
- `crates/codegen/workspace-types/src/rpc/hunks.rs`
- `crates/codegen/workspace-types/src/rpc/mod.rs`
- `crates/codegen/workspace-types/src/rpc/search.rs`
- `crates/codegen/workspace-types/src/rpc/skills.rs`
- `crates/codegen/workspace-types/src/rpc/workspace.rs`
- `crates/codegen/workspace-types/src/rpc/worktree.rs`

Cargo feature：`{"default": [], "default-bazel": []}`。

## 功能与规范映射

- [Workspace typed RPC boundary](../specs/workspace-rpc/spec.md#requirement-workspace-typed-rpc-boundary)：WorkspaceRpc SHALL 将Serialize请求与静态METHOD、Serialize+DeserializeOwned+Send响应类型关联；此包仅提供wire类型，不执行运行时、权限或传输封装。
- [Workspace observed event encoding](../specs/workspace-rpc/spec.md#requirement-workspace-observed-event-encoding)：WorkspaceEvent SHALL 以type/data相邻tag和snake_case编码FsChanged、CodebaseIndexUpdated、ToolsChanged，分别携带path+kind、files_indexed、session_id。
- [Workspace service file transfer schema](../specs/workspace-rpc/spec.md#requirement-workspace-service-file-transfer-schema)：put_files/get_files SHALL 分别承载files数组与逐文件results；PutFileEntry缺省create_dirs=true、append=false，GetFileEntry具有可选if_none_match、offset、length。
- [Workspace shell filesystem schema](../specs/workspace-rpc/spec.md#requirement-workspace-shell-filesystem-schema)：workspace.fs_* SHALL 提供list/exists/read/write/delete请求；请求字段为snake_case，节点及读取数据按camelCase并将类型字段命名为type。
- [Workspace client filesystem schema](../specs/workspace-rpc/spec.md#requirement-workspace-client-filesystem-schema)：client_fs_list/stat/read_file SHALL 使用独立camelCase schema和固定宽整数；列表节点以type表示file/directory，时间字段mtimeMs，stat的可选种类字段为nodeType。
- [Workspace Git read and status schemas](../specs/workspace-rpc/spec.md#requirement-workspace-git-read-and-status-schemas)：Git读取接口 SHALL 提供status/files/diff/info/branches/root/current commit/VCS/branch info/metadata/collect changes请求；status输出使用严格format tagged structured或prompt枚举。
- [Workspace Git mutation schemas](../specs/workspace-rpc/spec.md#requirement-workspace-git-mutation-schemas)：Git修改接口 SHALL 声明stage、stage content、unstage、discard、commit、sync base、checkout、stash、checkout commit请求及对应响应。
- [Workspace Git change and repository data](../specs/workspace-rpc/spec.md#requirement-workspace-git-change-and-repository-data)：Git状态与diff数据 SHALL 表达staged/unstaged、仓库和upstream元信息及文件变更；ChangeType为create/edit/delete/rename/copy/typechange/untracked，以type字段输出。
- [Workspace Git archive collection schema](../specs/workspace-rpc/spec.md#requirement-workspace-git-archive-collection-schema)：GitCollectChangesReq SHALL 默认include_commits/include_uncommitted=true、max_file_bytes=0、base_ref=None和空force_include_paths；响应包含repo/head/publicBase/commits/uncommitted/untracked/warnings/totalSizeBytes。
- [Workspace worktree creation schema](../specs/workspace-rpc/spec.md#requirement-workspace-worktree-creation-schema)：Worktree创建请求 SHALL 使用camelCase，copyMode缺省dirty；WorktreeType闭合小写linked/standalone/git，Rust默认linked，FromStr只接受精确小写值。
- [Workspace worktree management schema](../specs/workspace-rpc/spec.md#requirement-workspace-worktree-management-schema)：Worktree管理 SHALL 声明remove/apply/show/gc/list/db rebuild/path/stats方法，DTO不直接删除或合并。
- [Workspace hunk action and query schema](../specs/workspace-rpc/spec.md#requirement-workspace-hunk-action-and-query-schema)：Hunk接口 SHALL 声明单项、file、turn、all四种accept/reject动作及staged files/file summaries/all hunks/all file contents/session summary/filtered hunks查询。
- [Workspace hunk content views](../specs/workspace-rpc/spec.md#requirement-workspace-hunk-content-views)：Hunk文件内容视图 SHALL 以missing/binary/tooLarge/lfsPointer/symlink/full闭合状态表示baseline/current，并支持可省略byteLen/content及isAgentFile/staged。
- [Workspace content and fuzzy search schema](../specs/workspace-rpc/spec.md#requirement-workspace-content-and-fuzzy-search-schema)：内容搜索 SHALL 使用camelCase pattern/flags/globs/limits/cwd/contextId，响应files与totalMatches/totalFiles/truncated；模糊搜索提供open/change/close，后两者关联bool响应。
- [Workspace code navigation schema](../specs/workspace-rpc/spec.md#requirement-workspace-code-navigation-schema)：代码导航 SHALL 声明按file/line/col跳转definition/references、按symbol和可选context_file查找，以及index status五种请求，均允许可选root。
- [Workspace discovery wire mirrors](../specs/workspace-rpc/spec.md#requirement-workspace-discovery-wire-mirrors)：发现接口 SHALL 使用严格空请求声明agents md、skills、hook registry；AgentConfigFile、SkillInfo及HookRegistryWire/HookSpecWire拒绝未知字段。
- [Workspace identity and GitHub export schema](../specs/workspace-rpc/spec.md#requirement-workspace-identity-and-github-export-schema)：workspace.info SHALL 关联空严格请求与任意JSON Value；export_github关联project_dir及可选repo_full_name/branch/commit_message请求与repo_full_name/repo_url/branch/commit_sha/no_changes响应。

## 边界

- 提供fs、git、worktree、hunks、search、code navigation、agents md、skills、hooks、workspace info与GitHub export方法；完整METHOD和response映射登记逐包review。
- 分隔常量为双下划线；default和default-bazel均为空feature。
- 只接受created/modified/removed/renamed。
- 本枚举不提供prompt/tool/compaction/agent生命周期事件；不凭类型定义推断watcher已发送事件。
- 包含path/ok及可选error/hash，None省略；此类型不执行hash计算、root containment或事务处理。
- 包含path/exists、缺省false的matched和可省略content/hash/size/error；UTF8范围与缓存命中行为须由实现包验证。
- depth=1、limit=1000、offset=0、hidden/follow_symlinks/respect_git_ignore=true，globs为空。
- 读取max_bytes=1048576、encoding=utf8、offset/length=None且拒绝未知字段；写入create_dirs=true。预算clamp、symlink安全及字节编码并不在DTO中实现。
- list depth=1、limit=1000、offset=0、三个过滤布尔true；read maxBytes=1MiB、utf8、offset/length=None，类型本身不拒绝超大正预算。
- size/hash/type必需，content/contentBase64可省略；两者互斥和hash有效性只是调用方契约，struct本身不强制。text/binary及utf8/base64为闭合枚举。
- include_untracked和ignore_submodules=true，stats/patches=false，format=structured；GitFiles version=HEAD，GitDiff from=HEAD/to=working，patch/content/merge_base=false。
- structured必须data，prompt必须prompt，未知额外字段和无format的平铺状态拒绝；into_structured对prompt返回None。
- discard scope=both且include_untracked=false；commit amend/signoff/push/sync/stage_all/seed_default_excludes=false且expected_branch=None；checkout create、stash include_untracked、sync abort=false。
- CommitResult含data/warning及可省略outcome，PushStatus为not_requested/skipped/ok/conflict/failed；sync outcome以kind表示up_to_date/merged/conflicts/aborted。类型不执行分支保护、push或merge。
- 按files顺序过滤None并用单换行连接；没有Some返回None，Some空串仍参与。
- 默认git，其他wire值jujutsuColocated/none；is_jj仅colocated为true，is_repo仅none为false，不访问文件系统。
- 携带base64命名patch、stats、binary条目和作者/提交者时间字段，空binary数组省略；DTO不验证base64或数据总量一致性。
- UNTRACKED_CONTENT_THRESHOLD为1048576；UntrackedFileData携带binary/size/truncated/contentIncluded及可选contentBase64，实际过滤和截断仍由实现决定。
- WorktreeCreateSyncReq透明展开；CreateWorktreeFromWorktreeSyncReq保留inner，wire mirror不携带cancellationToken/resolvedDestPath。
- status为creating或exists，sessionId/worktreePath必需，exists还含commit，sourceGitRoot=None时省略；METHOD的部分Response仍为Value，不强制此enum。
- remove拒绝未知字段且force/dryRun=false；apply mode=overwrite，另有merge；list把types写作type数组，include_all=false；gc dry_run/force=false且max_age_secs可为负数。
- apply以status区分success(files/gitRoot)与conflicts(files/conflicts)，冲突含base/ours/theirs可选文本；remove含removed和可省略resolvedPath，db path为可选path。
- 外层action包含hunk_id和小写action；HunkActionResponse空struct编码为对象，不等于注释中的null。
- HunkWire使用camelCase且无selected，source以type编码agentEdit/externalEditOnAgentFile/external；agentEdit内部仍为prompt_index。汇总包含accepted/rejected统计、turns和pending数量，类型不保证数值相互一致。
- Default为missing，未知状态拒绝；derive Default不意味着JSON省略status可用。
- status、byteLen和content之间没有额外一致性校验，内容读取或大小过滤在hunk-tracker实现核查。
- JSON默认true，derive Default为false；其他搜索flags=false、globs为空、limits可选。ContentMatchFile::new从路径末段取name，取不到则用原路径。
- root/request_id/session_id可选，hidden=false，target为null或instanceId/connId对象；change dirs_only=false、limit=None，DTO不把注释100直接写入字段。
- 默认false；line/col使用usize，不在类型层强制坐标起点或文件存在。
- locations包含path/line及可省略symbol；status包含active、可选file_count和files/definitions/references统计。
- name/description/path/scope必需，enabled/user_invocable默认true，has_user_specified_description/disable_model_invocation=false；可选展示、paths、来源JSON、plugin身份/目录、allowed_tools/model/effort/body省略None。scope排序local<repo<user<server<bundled<plugin，未知scope拒绝。
- hooks映射15种snake_case闭合事件到spec列表，spec含name/event/handler_type/enabled/timeout_ms/on_failure/source_dir/extra_env/layer及可选matcher/command/raw/url；不包含编译matcher，也不执行发现和hook。
- 覆盖repo_not_specified/invalid_repo_name/project_dir_invalid/auth_failed/push_rejected/timeout/git_failed七个gh_export前缀码，未知返回None。
- 不访问GitHub、不校验仓库名或授权；成功提交和推送需实现包独立证据。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 全部RPC关联

以下从已逐行阅读的实现声明提取，覆盖METHOD与Response关联；不是服务端已实现或已运行的证明。

| 模块 | 请求 | METHOD | Response |
| --- | --- | --- | --- |
| agents_md.rs | `DiscoverAgentsMdReq` | `"workspace.discover_agents_md"` | `Vec<AgentConfigFile>` |
| code_nav.rs | `CodeGotoDefinitionReq` | `"workspace.code_goto_definition"` | `CodeNavResponse` |
| code_nav.rs | `CodeGotoReferencesReq` | `"workspace.code_goto_references"` | `CodeNavResponse` |
| code_nav.rs | `CodeFindDefinitionsReq` | `"workspace.code_find_definitions"` | `CodeNavResponse` |
| code_nav.rs | `CodeFindReferencesReq` | `"workspace.code_find_references"` | `CodeNavResponse` |
| code_nav.rs | `CodeIndexStatusReq` | `"workspace.code_index_status"` | `CodeIndexStatusResponse` |
| export_github.rs | `ExportGithubReq` | `"workspace.export_github"` | `ExportGithubResponse` |
| fs.rs | `PutFilesReq` | `"workspace.put_files"` | `PutFilesRes` |
| fs.rs | `GetFilesReq` | `"workspace.get_files"` | `GetFilesRes` |
| fs.rs | `FsListReq` | `"workspace.fs_list"` | `FsListData` |
| fs.rs | `FsExistsReq` | `"workspace.fs_exists"` | `FsExistsData` |
| fs.rs | `FsReadFileReq` | `"workspace.fs_read_file"` | `FsReadFileData` |
| fs.rs | `FsWriteFileReq` | `"workspace.fs_write_file"` | `()` |
| fs.rs | `FsDeleteFileReq` | `"workspace.fs_delete_file"` | `()` |
| fs.rs | `ClientFsListReq` | `CLIENT_FS_LIST_METHOD` | `ClientFsListRes` |
| fs.rs | `ClientFsStatReq` | `CLIENT_FS_STAT_METHOD` | `ClientFsStatRes` |
| fs.rs | `ClientFsReadFileReq` | `CLIENT_FS_READ_FILE_METHOD` | `ClientFsReadFileRes` |
| git.rs | `GitStatusExtReq` | `"workspace.git_status_ext"` | `GitStatusExtResponse` |
| git.rs | `GitFilesReq` | `"workspace.git_files"` | `GitReadFilesData` |
| git.rs | `GitDiffReq` | `"workspace.git_diff"` | `GitDiffsData` |
| git.rs | `GitStageReq` | `"workspace.git_stage"` | `StageData` |
| git.rs | `GitStageContentReq` | `"workspace.git_stage_content"` | `()` |
| git.rs | `GitUnstageReq` | `"workspace.git_unstage"` | `()` |
| git.rs | `GitDiscardReq` | `"workspace.git_discard"` | `()` |
| git.rs | `GitCommitReq` | `"workspace.git_commit"` | `CommitResult` |
| git.rs | `GitSyncBaseReq` | `"workspace.git_sync_base"` | `GitSyncBaseResult` |
| git.rs | `GitCheckoutReq` | `"workspace.git_checkout"` | `()` |
| git.rs | `GitStashReq` | `"workspace.git_stash"` | `()` |
| git.rs | `GitInfoReq` | `"workspace.git_info"` | `GitInfoData` |
| git.rs | `GitBranchesReq` | `"workspace.git_branches"` | `GitBranchListData` |
| git.rs | `GitResolveRootReq` | `"workspace.git_resolve_root"` | `Option<std::path::PathBuf>` |
| git.rs | `GitCurrentCommitReq` | `"workspace.git_current_commit"` | `Option<String>` |
| git.rs | `DetectVcsKindReq` | `"workspace.detect_vcs_kind"` | `VcsKind` |
| git.rs | `GitCheckoutCommitReq` | `"workspace.git_checkout_commit"` | `CheckoutCommitResponse` |
| git.rs | `GitBranchInfoReq` | `"workspace.git_branch_info"` | `Option<GitInfoData>` |
| git.rs | `GitMetadataReq` | `"workspace.git_metadata"` | `Value` |
| git.rs | `GitCollectChangesReq` | `"workspace.git_collect_changes"` | `GitCollectChangesResponse` |
| hooks.rs | `HookRegistryReq` | `"workspace.hook_registry"` | `HookRegistryWire` |
| hunks.rs | `HunkSingleActionReq` | `"workspace.hunk_action"` | `HunkActionResponse` |
| hunks.rs | `HunkFileActionReq` | `"workspace.hunk_file_action"` | `BulkHunkActionResponse` |
| hunks.rs | `HunkTurnActionReq` | `"workspace.hunk_turn_action"` | `BulkHunkActionResponse` |
| hunks.rs | `HunkAllActionReq` | `"workspace.hunk_all_action"` | `BulkHunkActionResponse` |
| hunks.rs | `HunkGetStagedFilesReq` | `"workspace.hunk_get_staged_files"` | `Vec<String>` |
| hunks.rs | `HunkGetFileSummariesReq` | `"workspace.hunk_get_file_summaries"` | `Vec<FileSummary>` |
| hunks.rs | `HunkGetAllHunksReq` | `"workspace.get_all_hunks"` | `Vec<HunkWire>` |
| hunks.rs | `HunkGetAllFileContentsReq` | `"workspace.hunk_get_all_file_contents"` | `Vec<FileContentEntryWire>` |
| hunks.rs | `HunkGetSessionSummaryReq` | `"workspace.get_session_summary"` | `SessionSummaryWire` |
| hunks.rs | `HunkGetFilteredHunksReq` | `"workspace.hunk_get_filtered_hunks"` | `FilteredHunksResponse` |
| search.rs | `ContentSearchRequest` | `"workspace.ripgrep"` | `ContentSearchData` |
| search.rs | `FuzzyOpenReq` | `"workspace.fuzzy_open"` | `String` |
| search.rs | `FuzzyChangeReq` | `"workspace.fuzzy_change"` | `bool` |
| search.rs | `FuzzyCloseReq` | `"workspace.fuzzy_close"` | `bool` |
| skills.rs | `DiscoverSkillsReq` | `"workspace.discover_skills"` | `Vec<SkillInfo>` |
| workspace.rs | `WorkspaceInfoReq` | `"workspace.info"` | `Value` |
| worktree.rs | `CreateWorktreeRequest` | `"workspace.create_worktree"` | `Value` |
| worktree.rs | `WorktreeCreateSyncReq` | `"workspace.worktree_create_sync"` | `Value` |
| worktree.rs | `RemoveWorktreeRequest` | `"workspace.remove_worktree"` | `Value` |
| worktree.rs | `CreateWorktreeFromWorktreeSyncReq` | `"workspace.worktree_create_from_worktree_sync"` | `CreateWorktreeFromWorktreeResponse` |
| worktree.rs | `ApplyWorktreeRequest` | `"workspace.apply_worktree"` | `Value` |
| worktree.rs | `WorktreeShowReq` | `"workspace.worktree_show"` | `Value` |
| worktree.rs | `WorktreeGcReq` | `"workspace.worktree_gc"` | `Value` |
| worktree.rs | `WorktreeListReq` | `"workspace.worktree_list"` | `Value` |
| worktree.rs | `WorktreeDbRebuildReq` | `"workspace.worktree_db_rebuild"` | `Value` |
| worktree.rs | `WorktreeDbPathReq` | `"workspace.worktree_db_path"` | `WorktreeDbPathResponse` |
| worktree.rs | `WorktreeDbStatsReq` | `"workspace.worktree_db_stats"` | `Value` |

## 公开结构字段清单

字段命名是Rust声明，wire命名、默认值和省略规则以上述规范与源码serde属性为准。空结构、tuple wrapper和闭合枚举分别列明。类型声明不证明服务端功能已执行。

### events/workspace.rs

- 枚举 `FsEventKind`：变体和tag规则见本模块对应规范及源码。
- 枚举 `WorkspaceEvent`：变体和tag规则见本模块对应规范及源码。

### rpc/agents_md.rs

- `DiscoverAgentsMdReq`：空结构。
- `AgentConfigFile`：`file_name: String`；`file_path: String`；`content: String`。

### rpc/code_nav.rs

- `CodeGotoDefinitionReq`：`root: Option<std::path::PathBuf>`；`file: String`；`line: usize`；`col: usize`。
- `CodeGotoReferencesReq`：`root: Option<std::path::PathBuf>`；`file: String`；`line: usize`；`col: usize`；`include_definition: bool`。
- `CodeFindDefinitionsReq`：`root: Option<std::path::PathBuf>`；`symbol: String`；`context_file: Option<String>`。
- `CodeFindReferencesReq`：`root: Option<std::path::PathBuf>`；`symbol: String`；`context_file: Option<String>`。
- `CodeIndexStatusReq`：`root: Option<std::path::PathBuf>`。
- `CodeIndexStatusResponse`：`active: bool`；`file_count: Option<usize>`；`stats: Option<CodeIndexStats>`。
- `CodeIndexStats`：`files: usize`；`definitions: usize`；`references: usize`。
- `CodeNavLocation`：`path: String`；`line: usize`；`symbol: Option<String>`。
- `CodeNavResponse`：`locations: Vec<CodeNavLocation>`。

### rpc/export_github.rs

- `ExportGithubReq`：`project_dir: String`；`repo_full_name: Option<String>`；`branch: Option<String>`；`commit_message: Option<String>`。
- `ExportGithubResponse`：`repo_full_name: String`；`repo_url: String`；`branch: String`；`commit_sha: String`；`no_changes: bool`。
- 枚举 `ExportGithubError`：变体和tag规则见本模块对应规范及源码。

### rpc/fs.rs

- `PutFileEntry`：`path: String`；`content: String`；`create_dirs: bool`；`append: bool`。
- `PutFilesReq`：`files: Vec<PutFileEntry>`。
- `PutFileResult`：`path: String`；`ok: bool`；`error: Option<String>`；`hash: Option<String>`。
- `PutFilesRes`：`results: Vec<PutFileResult>`。
- `GetFileEntry`：`path: String`；`if_none_match: Option<String>`；`offset: Option<u64>`；`length: Option<u64>`。
- `GetFilesReq`：`files: Vec<GetFileEntry>`。
- `GetFileResult`：`path: String`；`exists: bool`；`content: Option<String>`；`hash: Option<String>`；`matched: bool`；`size: Option<u64>`；`error: Option<String>`。
- `GetFilesRes`：`results: Vec<GetFileResult>`。
- `FsListNode`：`name: String`；`path: String`；`node_type: String`；`is_symlink: Option<bool>`；`size: Option<u64>`；`modified_at: Option<String>`。
- `FsListData`：`nodes: Vec<FsListNode>`；`truncated: bool`。
- `FsExistsData`：`exists: bool`。
- `FsReadFileData`：`content: String`；`content_base64: Option<String>`；`size: u64`；`line_count: Option<u64>`；`content_type: String`。
- `FsListReq`：`path: String`；`cwd: Option<PathBuf>`；`depth: usize`；`limit: usize`；`offset: u64`；`include_hidden: bool`；`follow_symlinks: bool`；`respect_git_ignore: bool`；`include_globs: Vec<String>`；`exclude_globs: Vec<String>`。
- `FsExistsReq`：`path: String`；`cwd: Option<PathBuf>`。
- `FsReadFileReq`：`path: String`；`cwd: Option<PathBuf>`；`offset: Option<u64>`；`length: Option<u64>`；`max_bytes: u64`；`encoding: FsReadEncoding`。
- `FsWriteFileReq`：`path: String`；`cwd: Option<PathBuf>`；`content: String`；`create_dirs: bool`。
- `FsDeleteFileReq`：`path: String`；`cwd: Option<PathBuf>`。
- `ClientFsListReq`：`path: String`；`depth: u32`；`include_hidden: bool`；`limit: u32`；`offset: u64`；`follow_symlinks: bool`；`respect_git_ignore: bool`；`include_globs: Vec<String>`；`exclude_globs: Vec<String>`。
- `ClientFsListNode`：`name: String`；`path: String`；`node_type: FsNodeType`；`is_symlink: Option<bool>`；`size: Option<u64>`；`mtime_ms: Option<i64>`。
- `ClientFsListRes`：`nodes: Vec<ClientFsListNode>`；`truncated: bool`。
- `ClientFsStatReq`：`path: String`。
- `ClientFsStatRes`：`exists: bool`；`node_type: Option<FsNodeType>`；`size: Option<u64>`；`mtime_ms: Option<i64>`；`hash: Option<String>`。
- `ClientFsReadFileReq`：`path: String`；`offset: Option<u64>`；`length: Option<u64>`；`max_bytes: u64`；`encoding: FsReadEncoding`。
- `ClientFsReadFileRes`：`content: Option<String>`；`content_base64: Option<String>`；`size: u64`；`hash: String`；`content_type: FsContentType`。
- 枚举 `FsNodeType`：变体和tag规则见本模块对应规范及源码。
- 枚举 `FsReadEncoding`：变体和tag规则见本模块对应规范及源码。
- 枚举 `FsContentType`：变体和tag规则见本模块对应规范及源码。

### rpc/git.rs

- `GitStatusExtReq`：`git_root: Option<std::path::PathBuf>`；`include_untracked: bool`；`include_stats: bool`；`ignore_submodules: bool`；`include_patches: bool`；`format: GitStatusFormat`。
- `GitFilesReq`：`git_root: Option<std::path::PathBuf>`；`paths: Vec<String>`；`version: String`。
- `GitDiffReq`：`git_root: Option<std::path::PathBuf>`；`paths: Option<Vec<String>>`；`from: String`；`to: String`；`include_patch: bool`；`include_content: bool`；`merge_base: bool`。
- `GitStageReq`：`git_root: Option<std::path::PathBuf>`；`paths: Option<Vec<String>>`。
- `GitStageContentReq`：`git_root: Option<std::path::PathBuf>`；`path: String`；`content: String`。
- `GitUnstageReq`：`git_root: Option<std::path::PathBuf>`；`paths: Option<Vec<String>>`。
- `GitDiscardReq`：`git_root: Option<std::path::PathBuf>`；`paths: Option<Vec<String>>`；`scope: DiscardScope`；`include_untracked: bool`。
- `GitCommitReq`：`git_root: Option<std::path::PathBuf>`；`message: String`；`amend: bool`；`signoff: bool`；`push: bool`；`sync: bool`；`stage_all: bool`；`seed_default_excludes: bool`；`expected_branch: Option<String>`。
- `GitSyncBaseReq`：`git_root: Option<std::path::PathBuf>`；`base_ref: Option<String>`；`abort: bool`；`expected_branch: Option<String>`。
- `GitSyncBaseResult`：`outcome: GitSyncBaseOutcome`。
- `GitCheckoutReq`：`git_root: Option<std::path::PathBuf>`；`branch: String`；`create: bool`。
- `GitStashReq`：`git_root: Option<std::path::PathBuf>`；`include_untracked: bool`。
- `GitInfoReq`：`git_root: Option<std::path::PathBuf>`。
- `GitBranchesReq`：`git_root: Option<std::path::PathBuf>`。
- `GitResolveRootReq`：`cwd: std::path::PathBuf`。
- `GitCurrentCommitReq`：`git_root: std::path::PathBuf`。
- `DetectVcsKindReq`：`path: std::path::PathBuf`。
- `GitCheckoutCommitReq`：`git_root: std::path::PathBuf`；`head_commit: String`；`head_branch: Option<String>`；`stash_if_dirty: bool`。
- `CheckoutCommitResponse`：`checked_out: bool`；`stashed: bool`；`fetched: bool`；`error: Option<String>`。
- `GitBranchInfoReq`：空结构。
- `GitMetadataReq`：空结构。
- `CommitData`：`commit_hash: Option<String>`；`output: Option<String>`。
- `CommitResult`：`data: CommitData`；`warning: Option<String>`；`outcome: Option<CommitOutcome>`。
- `CommitOutcome`：`sha: Option<String>`；`clean: bool`；`pushed: bool`；`push: PushStatus`。
- `StageData`：`paths: Vec<String>`。
- `GitFileChange`：`path: String`；`old_path: Option<String>`；`change_type: ChangeType`；`staged: Option<bool>`；`additions: u64`；`deletions: u64`；`patch: Option<String>`；`patch_bytes: Option<u64>`；`patch_lines: Option<u64>`；`old_text: Option<String>`；`new_text: Option<String>`。
- `GitStatusData`：`root: Option<String>`；`main_root: Option<String>`；`is_worktree: Option<bool>`；`branch: Option<String>`；`commit: Option<String>`；`upstream: Option<String>`；`remote_url: Option<String>`；`ahead: Option<usize>`；`behind: Option<usize>`；`staged: Vec<GitFileChange>`；`unstaged: Vec<GitFileChange>`。
- `GitError`：`path: Option<String>`；`code: String`；`message: String`。
- `GitReadFilesData`：`files: Vec<GitReadFile>`；`errors: Vec<GitError>`。
- `GitReadFile`：`path: String`；`version: String`；`content: String`；`is_binary: Option<bool>`。
- `GitDiffsData`：`files: Vec<GitFileChange>`。
- `GitInfoData`：`root: String`；`remotes: Vec<String>`；`current_branch: Option<String>`；`default_branch: Option<String>`；`vcs_kind: Option<VcsKind>`。
- `GitBranchEntry`：`name: String`；`current: bool`；`remote: bool`。
- `GitBranchListData`：`current_branch: Option<String>`；`repo_root: String`；`branches: Vec<GitBranchEntry>`。
- `GitCollectChangesReq`：`repo_path: String`；`include_commits: bool`；`include_uncommitted: bool`；`base_ref: Option<String>`；`max_file_bytes: u64`；`force_include_paths: Vec<std::path::PathBuf>`。
- `GitCollectChangesResponse`：`repo: RepoInfo`；`head: String`；`public_base: PublicBaseData`；`commits: Vec<CommitWithPatchData>`；`uncommitted: Option<UncommittedChangesData>`；`untracked: Vec<UntrackedFileData>`；`warnings: Vec<String>`；`total_size_bytes: u64`。
- `RepoInfo`：`root: String`；`git_dir: Option<String>`；`head: Option<String>`；`branch: Option<String>`；`is_detached: bool`；`upstream: Option<String>`；`upstream_head: Option<String>`；`remote_url: Option<String>`；`ahead: Option<usize>`；`behind: Option<usize>`。
- `DiffStatsSummary`：`files_changed: usize`；`insertions: usize`；`deletions: usize`。
- `PublicBaseData`：`commit: String`；`refs: Vec<String>`。
- `CommitWithPatchData`：`id: String`；`parents: Vec<String>`；`author: IdentityData`；`committer: IdentityData`；`summary: Option<String>`；`message: Option<String>`；`patch_base64: Option<String>`；`stats: DiffStatsSummary`；`binary_files: Vec<BinaryFileInfoData>`。
- `IdentityData`：`name: Option<String>`；`email: Option<String>`；`time: Option<String>`；`time_seconds: i64`；`offset_minutes: i32`。
- `BinaryFileInfoData`：`path: String`；`status: String`；`size_bytes: u64`；`blob_included: bool`；`truncated: bool`；`exclude_reason: Option<String>`；`content_base64: Option<String>`。
- `UncommittedChangesData`：`staged_patch_base64: Option<String>`；`staged_stats: DiffStatsSummary`；`unstaged_patch_base64: Option<String>`；`unstaged_stats: DiffStatsSummary`；`staged_binary_files: Vec<BinaryFileInfoData>`；`unstaged_binary_files: Vec<BinaryFileInfoData>`。
- `UntrackedFileData`：`path: String`；`is_binary: bool`；`size_bytes: u64`；`truncated: bool`；`content_base64: Option<String>`；`content_included: bool`。
- 枚举 `GitSyncBaseOutcome`：变体和tag规则见本模块对应规范及源码。
- 枚举 `VcsKind`：变体和tag规则见本模块对应规范及源码。
- 枚举 `ChangeType`：变体和tag规则见本模块对应规范及源码。
- 枚举 `GitStatusFormat`：变体和tag规则见本模块对应规范及源码。
- 枚举 `PushStatus`：变体和tag规则见本模块对应规范及源码。
- 枚举 `GitStatusExtResponse`：变体和tag规则见本模块对应规范及源码。
- 枚举 `DiscardScope`：变体和tag规则见本模块对应规范及源码。

### rpc/hooks.rs

- `HookRegistryReq`：空结构。
- `HookRegistryWire`：`hooks: HashMap<HookEventNameWire, Vec<HookSpecWire>>`。
- `HookSpecWire`：`name: String`；`event: HookEventNameWire`；`handler_type: String`；`configured_matcher: Option<String>`；`enabled: bool`；`command: Option<PathBuf>`；`command_raw: Option<String>`；`url: Option<String>`；`url_raw: Option<String>`；`timeout_ms: u64`；`on_failure: String`；`source_dir: PathBuf`；`extra_env: HashMap<String, String>`；`layer: String`。
- 枚举 `HookEventNameWire`：变体和tag规则见本模块对应规范及源码。

### rpc/hunks.rs

- `HunkActionReq`：`hunk_id: String`；`action: HunkActionKind`。
- `HunkSingleActionReq`：`action: HunkActionReq`。
- `HunkFileActionReq`：`path: String`；`action: HunkActionKind`。
- `HunkTurnActionReq`：`prompt_index: usize`；`action: HunkActionKind`。
- `HunkAllActionReq`：`action: HunkActionKind`。
- `HunkGetStagedFilesReq`：空结构。
- `HunkGetFileSummariesReq`：空结构。
- `HunkActionResponse`：空结构。
- `BulkHunkActionResponse`：`affected: Vec<String>`。
- `FileSummary`：`path: String`；`hunk_count: usize`；`is_agent_file: bool`。
- `HunkGetAllHunksReq`：空结构。
- `HunkGetAllFileContentsReq`：空结构。
- `HunkGetSessionSummaryReq`：空结构。
- `HunkGetFilteredHunksReq`：`path: Option<String>`；`source: Option<String>`。
- `HunkWire`：`id: String`；`path: PathBuf`；`line_info: HunkLineInfoWire`；`source: HunkSourceWire`；`old_text: Option<String>`；`new_text: String`；`patch: Option<String>`；`created_at: DateTime<Utc>`。
- `HunkLineInfoWire`：`old_start: usize`；`old_count: usize`；`new_start: usize`；`new_count: usize`。
- `FileContentViewWire`：`status: FileContentStatusWire`；`byte_len: Option<usize>`；`content: Option<String>`。
- `FileContentEntryWire`：`path: PathBuf`；`baseline: FileContentViewWire`；`current: FileContentViewWire`；`is_agent_file: bool`；`staged: bool`。
- `SessionStatsWire`：`accepted_hunks: usize`；`rejected_hunks: usize`；`accepted_lines_added: usize`；`accepted_lines_removed: usize`；`rejected_lines_added: usize`；`rejected_lines_removed: usize`。
- `TurnSummaryWire`：`prompt_index: usize`；`files: Vec<PathBuf>`；`pending_hunks: Vec<HunkWire>`；`lines_added: usize`；`lines_removed: usize`。
- `SessionSummaryWire`：`stats: SessionStatsWire`；`turns: Vec<TurnSummaryWire>`；`files_modified: usize`；`files_with_pending: usize`；`pending_hunks: usize`；`pending_lines_added: usize`；`pending_lines_removed: usize`；`unattributed_pending: usize`。
- `FilteredHunksResponse`：`hunks: Vec<HunkWire>`；`total: usize`。
- 枚举 `HunkActionKind`：变体和tag规则见本模块对应规范及源码。
- 枚举 `HunkSourceWire`：变体和tag规则见本模块对应规范及源码。
- 枚举 `FileContentStatusWire`：变体和tag规则见本模块对应规范及源码。

### rpc/search.rs

- `ContentSearchRequest`：`pattern: String`；`case_insensitive: bool`；`whole_word: bool`；`is_regex: bool`；`include_globs: Vec<String>`；`exclude_globs: Vec<String>`；`max_files: Option<usize>`；`max_matches: Option<usize>`；`respect_gitignore: bool`；`cwd: Option<std::path::PathBuf>`；`context_id: Option<String>`。
- `ContentMatch`：`line: usize`；`content: String`；`match_start: Option<usize>`；`match_end: Option<usize>`。
- `ContentMatchFile`：`name: String`；`path: String`；`matches: Vec<ContentMatch>`。
- `ContentSearchData`：`files: Vec<ContentMatchFile>`；`total_matches: usize`；`total_files: usize`；`truncated: bool`。
- `ClientId`：`instance_id: String`；`conn_id: String`。
- `FuzzyOpenReq`：`root: Option<std::path::PathBuf>`；`request_id: Option<String>`；`hidden: bool`；`session_id: Option<String>`；`target_client_id: TargetClientId`。
- `FuzzyChangeReq`：`search_id: String`；`query: String`；`dirs_only: bool`；`limit: Option<usize>`。
- `FuzzyCloseReq`：`search_id: String`。
- 枚举 `TargetClientId`：变体和tag规则见本模块对应规范及源码。

### rpc/skills.rs

- `DiscoverSkillsReq`：空结构。
- `SkillInfo`：`name: String`；`display_name: Option<String>`；`description: String`；`has_user_specified_description: bool`；`paths: Option<Vec<String>>`；`when_to_use: Option<String>`；`short_description: Option<String>`；`author: Option<String>`；`argument_hint: Option<String>`；`license: Option<String>`；`compatibility: Option<String>`；`metadata: Option<std::collections::HashMap<String, String>>`；`path: String`；`scope: SkillScope`；`config_source: Option<Value>`；`plugin_name: Option<String>`；`plugin_version: Option<String>`；`plugin_root: Option<String>`；`plugin_data: Option<String>`；`allowed_tools: Option<Vec<String>>`；`model: Option<String>`；`effort: Option<String>`；`user_invocable: bool`；`disable_model_invocation: bool`；`enabled: bool`；`body: Option<String>`。
- 枚举 `SkillScope`：变体和tag规则见本模块对应规范及源码。

### rpc/workspace.rs

- `WorkspaceInfoReq`：空结构。

### rpc/worktree.rs

- `DirtyStateSummary`：`staged_count: u32`；`modified_count: u32`；`deleted_count: u32`；`untracked_count: u32`；`has_partially_staged: bool`；`skipped_dirs: Vec<String>`。
- `CopiedChangesSummary`：`staged_copied: u32`；`modified_copied: u32`；`untracked_copied: u32`；`deletions_applied: u32`；`warnings: Vec<String>`。
- `CreateWorktreeRequest`：`session_id: String`；`source_path: String`；`worktree_path: Option<String>`；`copy_mode: WorktreeCopyMode`；`git_ref: Option<String>`；`copy_ignored_in_background: bool`；`ignored_skip_patterns: Vec<String>`；`worktree_type: Option<WorktreeType>`；`label: Option<String>`。
- `RemoveWorktreeRequest`：`id_or_path: String`；`force: bool`；`dry_run: bool`。
- `RemoveWorktreeResponse`：`removed: bool`；`resolved_path: Option<String>`。
- `CreateWorktreeFromWorktreeResponse`：`status: String`；`new_session_id: String`；`worktree_path: String`；`commit: Option<String>`；`copied_changes: Option<CopiedChangesSummary>`；`source_git_root: Option<String>`。
- `CreateWorktreeFromWorktreeRequestWire`：`source_worktree_path: String`；`new_session_id: String`；`copy_mode: WorktreeCopyMode`；`git_ref: Option<String>`；`worktree_type: Option<WorktreeType>`；`label: Option<String>`。
- `CreateWorktreeFromWorktreeSyncReq`：`inner: CreateWorktreeFromWorktreeRequestWire`。
- `PrepareWorktreeFromWorktreeResponse`：`spawn_task: bool`；`response: Option<serde_json::Value>`；`error: Option<String>`。
- `ApplyWorktreeRequest`：`session_id: String`；`worktree_path: String`；`mode: ApplyMode`。
- `FileConflict`：`path: String`；`change_type: ChangeType`；`base: Option<String>`；`ours: Option<String>`；`theirs: Option<String>`。
- `WorktreeShowReq`：`id_or_path: String`。
- `WorktreeGcReq`：`dry_run: bool`；`max_age_secs: Option<i64>`；`force: bool`。
- `WorktreeListReq`：`repo: Option<String>`；`types: Vec<String>`；`include_all: bool`。
- `WorktreeDbRebuildReq`：空结构。
- `WorktreeDbPathReq`：空结构。
- `WorktreeDbPathResponse`：`path: Option<String>`。
- `WorktreeDbStatsReq`：空结构。
- `WorktreeCreateSyncReq`：tuple wrapper `pub CreateWorktreeRequest`。
- 枚举 `WorktreeType`：变体和tag规则见本模块对应规范及源码。
- 枚举 `WorktreeCopyMode`：变体和tag规则见本模块对应规范及源码。
- 枚举 `CreateWorktreeResponse`：变体和tag规则见本模块对应规范及源码。
- 枚举 `ApplyMode`：变体和tag规则见本模块对应规范及源码。
- 枚举 `ApplyWorktreeResponse`：变体和tag规则见本模块对应规范及源码。

## 审阅与测试限制

15个Rust文件及manifest全部读取，65个WorkspaceRpc关联、160个公开struct/enum声明已枚举。依赖只含chrono、serde与serde_json，不包含实际workspace操作实现。

- 当前测试失败位于rpc/hooks.rs的hook_registry_wire_round_trips_server_json：fixture没有on_failure，而HookSpecWire字段String无serde default。解析返回missing field on_failure，与规范中的必需字段一致。没有删除该要求或把失败算作通过。
- ContentSearchRequest派生Rust Default使respect_gitignore=false，但JSON缺字段的serde default函数给true。已明确区分，未猜测调用方期望。
- HunkActionResponse空struct序列化为{}，注释说operation返回null；需在workspace dispatcher核对其wire转换，不能只凭该注释认定实际返回null。
- FsReadFileReq严格拒绝未知字段；多数其他文件/Git/导航请求默认忽略未知字段。未将局部deny_unknown_fields推广到全包。
- ClientFsReadFileRes的两个Option内容字段不在类型中互斥；FileContentViewWire的status/content/byteLen也没有关系约束。类型可构造不一致值，并不证明当前handler实际产生该值。
- 路径包含检查、Git命令效果、commit单写者约束、同步恢复、内容编码与大小clamp、搜索默认limit、AGENTS/skills发现优先级和镜像漂移测试都要在对应实现包继续确认。此处记录字段与确定的serde行为，不把注释描述当运行事实。
