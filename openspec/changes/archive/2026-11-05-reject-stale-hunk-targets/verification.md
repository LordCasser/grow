# Acceptance

WS-06a实现、本crate定向运行及独立Luna审查已获父验收，Workspace/Shell传播里程碑随后通过。规范归档与全量校验结果在下文单独记录。父已亲读共同ensure_reject_target_matches及single/filebatch两个入口、StaleContent、原WS06b成功后commit边界及权限回归。Full最多读cached字节数+1并比较内容/EOF，Missing仅真NotFound；symlink/目录及已变内容拒绝，读错误保留ReadError。没有将mtime/size作为内容版本，也没有增加跨非合作writer原子CAS承诺。

固定Cargo/Rust/rustdoc1.93.1、MINSTACK16777216、JOBS2、offline、macOS UID501，独立`/tmp/grow-ws06a-verify.ZFQALE` Grow/TMPDIR、空全局/系统Git配置及hooks。实际命令`cargo test --offline --locked -p hunk-tracker --lib actor::tests -- --test-threads=1`首次构建与运行exit0，137 passed、0 failed、1 ignored、73 filtered；唯一ignored是既有refresh_storm_scan_count_during_rebase性能复现，未将其作为本片验证。权限和symlink场景实际执行，未跳过。父已读回actor-tests.log全138项枚举及actor-tests.exit=0，未只依赖完成通知。

全部10项指定回归均为ok：stale content/stale creation-deletion、matching selected hunks/matching creation-deletion-empty、refresh后retry保外部hunk、turn分组、permission failure、symlink replacement、原I/O failure投影、partial batch/retry。actor全模块同时覆盖原WS06b的normal accept/reject、creation/deletion、重算与summary/turn等既有对照。父读权限fixture：single/file/turn/all×read/missing-read/write/delete/restore共20组精确错误类型+PermissionDenied，恢复权限后核disk与完整pending投影；无宽泛Err放宽，未用目录Stale代替原真实写/删除失败。新stale矩阵覆盖append、同大小不同内容、missing、empty、binary、too-large及存在性变化，比较前的缓存状态与实际磁盘分离。

本轮未修源码或fixture，测试前后和父最终重算SHA256相同：actions.rs `e8a225ca251ecd49aaedc1f9b50df47037da55979f3261a67513b425de88b5c0`；types.rs `21986935afa00750afd8a047f2efa710a64bc218b4aa37aca04c405d08f0d4e8`；tests.rs `9d6df8be3b2c5721fee81e1531f2ca6de88853bb5d1d9738c22e839654937da5`。原始日志SHA256 `f00370339da5f2c295a11a524fe36f7e6b97b4cd677cb8a75eb5f636a9b236f8`。限定rustfmt/diff检查通过，Cargo和三文件写scope已释放；没有Cargo.lock变化。

独立Luna `01a1aa69-8b41-7581-9014-154d4dd53f8f`完成shared helper/错误/公共Handle/I/O矩阵及命令分派审查，未发现可行动问题；它读回同一日志和退出码，未自行执行测试，精确区分WS06b既有改动、Windows及Shell未验证范围。父接受本模块实现与review；后续新增测试和跨层集成由父亲自审查及验收，未重复未变化的actor测试。

## Shell/Workspace合成里程碑

仅新增`crates/codegen/shell/tests/test_hunk_stale_rejection.rs`，经过实际`hunk_extension::handle → WorkspaceOps → HunkTrackerHandle → actor`，不伪造中间错误或response。MvpAgent使用明确本地模型和预置remote settings，无认证/prompt/provider调用；真实actor无watcher，公共查询形成展示缓存barrier后再外部改磁盘。父已完整读取新测试fixture、四method实际dispatch、响应解析及全部断言。

实际`cargo test --offline --locked -p shell --test test_hunk_stale_rejection -- --test-threads=1`，固定相同1.93.1/MINSTACK/JOBS2/UID501与`/tmp/grow-ws06a-shell.pkvg6u`隔离Grow/TMP和空Git/hooks，首次编译及运行exit0：3 passed/0 failed/0 ignored/0 filtered，耗时0.50s。原始integration-tests.log全三项枚举与integration-tests.exit=0父已读回；编译2m46s，不计入test耗时，不将命令观察超时当作失败或完成。无源码或fixture修正，monitor与原命令均真实退出，写scope/Cargo已释放。

1. `shell_reject_stale_content_preserves_disk_and_pending_for_every_scope`：single/file/turn/all实际产品响应success:false，明确stale和path，affectedCount缺省；外部bytes、完整file state、turn index、session stats/summary、pending IDs/展示与事件均保持。
2. `shell_reject_matching_content_reports_actual_count_once`：四scope实际反转及正确affectedCount，single只处理选中hunk且保留另一处；重复请求single报告HunkNotFound、bulk返回0，均不重复统计或发送成功事件。
3. `shell_bulk_reject_preserves_edited_creation_and_recreated_deletion`：file保护外部编辑的creation，all保护重建的deletion，真实存在性冲突仍保磁盘和全投影。

父执行source-after.sha256全四文件OK：新test SHA256 `e4c39a530fd0a5e5f16f65b6ae7a2f644e9b0018b5f380b261c96bc148be1a75`，原actor三指纹保持137pass版本；原始integration日志SHA256 `f07be546d4600032591af117c2a2c8352105bdec5dd521d5becdb9c1fda998fa`。限定rustfmt/diff检查通过，未更改src/manifest/lock。结合原137项actor及已归档WS06b故障/部分提交证据，父接受06a/06b合成里程碑。该入口覆盖真实Shell handler，不含ACP传输层；没有改变传输契约，不据此宣称ACP、Windows权限或任意writer原子CAS已验证。

## 场景与证据核对

| delta场景 | 最终证据 |
| --- | --- |
| External edit precedes a reject | actor stale矩阵及四scope实际Shell失败，磁盘/投影/事件保持 |
| Creation or deletion is stale | actor全部scope存在性矩阵及Shell file creation/all recreated deletion |
| File, turn or all reject includes a conflict | actor partial batch/retry、turn分组与Shell四scope错误传播，原WS06b先成功文件保真实结果 |
| Target still matches | actor matching/empty/正常对照及Shell四scope反转与重复请求countonce |

Windows权限行为未执行；只承诺比较前的变化被检测。未操作正常Grow或用户真实文件，未安装/重启/提交。

## 归档收尾

归档前`openspec validate --all --strict --no-interactive`20/20通过；四场景已按上表核对后执行`openspec archive reject-stale-hunk-targets --yes`，主规范增加一个requirement/四scenario并归档成功。最后收尾任务在执行前保持未勾，因此archive明确提示4/5，未把尚未运行的归档当成已完成。归档后全量规范19/19通过，首次archive校验857通过、仅本change的末项未勾而失败；实际主规范/开发说明/backlog均已同步后补勾这一项，最终`openspec validate --archived --strict --no-interactive`实际exit0，858/858全部通过，父读回终态与本change通过行。
