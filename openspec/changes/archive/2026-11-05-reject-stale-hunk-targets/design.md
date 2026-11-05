# Design

父核对single reject_hunk和filebatch reject_hunks_batch：restore/delete/patch都来自state.current_content旧快照并直接write/remove，batch同样有缺口。Workspace Handle/Shell action错误已经传播到success:false，不需新UI协议。WS06b成功后提交的行为先交付，保证新增冲突Err不会计成功。

## Decision

同一hunk action owner在决定本文件的reject动作前读取实际文件完整内容/存在性，与生成当前展示hunk的FileContentState比较。Full要求内容相同；Missing要求真实缺失；读失败保留ReadError。不同版本返回清晰的stale conflict错误，原磁盘与file hunks/turn索引/统计保持待处理，不自动用新行号套旧hunk、不把缺失当空文本、不三方合并。正常匹配时沿现patch_lines/restore/delete及原成功提交边界。single/filebatch共用语义helper，file/turn/all通过原batch自然复用；不能只在Shell入口加check。

局部算法由coder选。新增错误variant如需跨crate穷尽适配由父先核定，不加平行HTTP/RPC错误协议。当前可最小采用HunkActionError::StaleContent { path:PathBuf }，已有Display转WorkspaceError/Shellsuccessfalse保持可诊断；确有穷尽caller同时适配属于本行为。IO提交与统计依赖前项，不重复改事务。

该检查兑现watcher滞后时已发生变更的拒绝，不提供对所有非合作外部writer的内核原子CAS。跨read/write的新并发写是现系统没有声明的能力，不能靠时间metadata或任意新全局锁虚假宣称已隔离；若真实操作链要求更强原子性，回父决定实际文件能力协议，不偷偷改全部文件写语义。

## Coder Task Spec / Review Plan

WS06b已冻结验收并归档。父重新核对当前hunk-tracker差异只有其成功后提交的actions/tests改动；StaleContent和实际磁盘比较尚未存在，被取消的WS06a执行未留下可验收实现。后续由单owner继续修改hunk-tracker reject/actions/types/必要tests，其他地方只做同错误适配；禁止再次混WS06b统计修复或全局watcher重构。真实Handle baseline/cache→外部append→reject：Err/StaleContent、磁盘外部行保持，pendinghunk/turn/stats不变。覆盖normalpatch、creation后额外修改、deleted后外部重建，以及single/file/turn/all；正常内容相同则只回退指定hunk，无关既有行保留。read/write/delete错误语义继续正确，batch先成功file保留实况、冲突file不计成功。

Tier3独立lunareview、父合成06a+06b数据里程碑公共API并检查Shell结果，无真实用户文件/目录。临时repo/GROW_HOME、Rust/Cargo/rustdoc1.93.1和父串行Cargo窗口。父同步docs/spec/backlog并仅真实验收后关项。
