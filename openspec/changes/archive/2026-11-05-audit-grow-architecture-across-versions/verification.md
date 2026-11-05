# 审计验证记录

本文由架构 owner 汇总亲自核对的证据。分区各自的源码审视、历史矩阵与局部实验保留在 reviews；没有执行的运行场景不写成通过。当前跨版本审计不改变业务行为，缺陷修复另立 change。

## 全仓与版本身份

- `cargo metadata --locked --no-deps --format-version 1`：57 个 workspace package，清单与分区映射见 inventory。
- `git rev-parse --is-shallow-repository`：false。
- 本地46 tag与只读 `git ls-remote --tags --refs origin` 一致，无远端独有/本地独有标签。Commit身份按 `<tag>^{commit}`；annotated tag object不能标作源码commit。
- 对全部tag解析 Cargo.toml `[package].name`，按blob复用：126个历史包名，69个不在当前workspace名字中，分为前缀迁移、已合并/删除模块和现有非workspace vendor/fuzz。历史独立模块逐项分配，见inventory。
- 根HEAD `d6128138c` 与 v2.6.0 `5475dff997f0` 不同；各分区只有核对scope相同后才复用该区证据。

## 父侧真实公共 API 补验

环境：macOS，`$HOME/.cargo/bin/rustc` 固定1.93.1。以 `-L dependency=target/debug/deps`、原构建native路径链接共享Cargo已编译的生产rlib；没有抽取或替换被测函数，没有运行另一个Cargo构建。测试程序和所有Git仓库、无关目录、文件均自建于系统临时目录，不访问或清理真实项目数据。程序只使用public APIs，退出码0表示成功复现当前机制，**不表示缺陷修复通过**。两份临时驱动源分别为 `/tmp/grow-architecture-parent-public-api.rs` 和 `/tmp/grow-architecture-parent-hunk-api.rs`；下述输入、API、输出和断言永久保留，不依赖临时文件长期存在。

### WS-01 — IndexManager 真实事件合并

链接 `libcodebase_graph-295800061b11121e.rlib`。新建 audit.rs：`pub fn audit_unique_symbol() {}`。`IndexManager::spawn(IndexManagerConfig::new(root).without_cache_load().without_cache_save())` 后通过阻塞query等待初始索引完成，has_definition为true。删除磁盘文件，`send_events([FileEvent::modified(path),FileEvent::removed(path)])` 作为同一个生产batch；随后query是actor顺序屏障。

实际输出：

```text
modified-removed: before=Some(true) after=Some(true) indexed_files=Some(1) source_exists=false
removed-only-control: after=Some(false) indexed_files=Some(0)
```

对照再单独发送Removed，symbol消失且file_count为0。该真实生产API证据把问题定位到事件归约，而非tree-sitter不识别删除。尚未逐语言遍历reference/alias细节；修复验收需同时断言definitions/references/file metadata清除。

### WS-03 — fast_worktree 真实删除入口

链接 `libfast_worktree-90f1501fe2df800f.rlib`。公共入口为 `fast_worktree::remove_worktree`，三组均只用自建临时对象：

| 对照/触发 | 输入 | 输出与断言 |
| --- | --- | --- |
| 合法 linked | git init/empty commit；git worktree add --detach linked HEAD | result_ok=true，checkout_exists=false，registration_exists=false |
| separate Git dir | git init --separate-git-dir separate-metadata separate；metadata含自建marker | result_ok=true，checkout_exists=false，metadata_exists=false；metadata不是linked registration |
| 指向无关目录的gitfile | adversarial/.git=`gitdir: <tmp>/unrelated-owned-fixture`；目标有must-survive.txt | result_ok=true，checkout_exists=false，unrelated_exists=false |

确认底层公共删除API递归删除未核对所有权的目标并返回Ok。上层workspace/shell的可达调用已亲自读源码；没有对上层ACP执行真实项目删除，也不把局部API实验称为完整UI复现。工作树删除和source/common-dir/backlink身份核对应由现有删除owner收敛。

WS-01/03同一驱动耗时4.10s、exit0。

### WS-06 — HunkTrackerActor 真实Reject入口

链接 `libhunk_tracker-c9b0d6773f041173.rlib`、`libtokio-37d7496dfccbc6a0.rlib`、`libtokio_util-9b742e3e78ce8475.rlib`。初始化并提交 `base\nkeep\n`，spawn真实HunkTrackerActor，AgentOnly；写 `agent\nkeep\n`并用record_agent_write记录prompt_index1，get_all_hunks等待actor处理。随后仅修改磁盘为 `agent\nkeep\nexternal-unique\n`（模拟watcher尚未交付），调用public `hunk_action(id,Reject)`。

```text
stale-reject: result_ok=true external_preserved=false content="base\nkeep\n"
```

确认普通成功Reject丢失未被所选hunk展示的外部追加内容。

第二文件建立同样cache、prompt_index2；查询确认有一个hunk后，将自建磁盘文件换成目录触发真实WriteError。hunk_action返回IsADirectory错误，但session summary的rejected_hunks从1变2；get_turn_hunks(2)为空，而get_all_hunks仍保留该hunk。summary的turns又仍显示一个pending hunk。这证明错误返回之前统计和turn_index已提前改变，派生查询互相不一致。完整actor/public API驱动耗时12.83s、exit0，最后取消actor。

两个后果需各自独立修复验收：磁盘并发编辑核对/冲突语义；失败时统计与索引的提交顺序。不能以只修其中一项就称WS-06全部关闭。

## 静态结论与尚未执行项

WS-02的摘要memo已亲自核对`(size,mtime_ms)`命中条件与RPC使用，当前只有分区机制实验，未运行真实ClientFsStat/ReadFile；WS-04身份归一和WS-05路径求值也保留分区证据限制。Session F1/F2/F3为实际冷恢复/接纳静态链，F4/F5为原journal源码注入存储实验；未制造真实掉电。Agent/config的Hook/LSP/config各项以调用链/条件和有界路径实验核实，不宣称全平台进程集成通过。UI容量/Markdown复制只作独立风险/债务，不给未经负载测量的失效阈值。

## 最终校验

七个分区全部完成并经父侧接受；57个workspace package与最终Cargo metadata逐项比对，每个恰有一个主要报告。父重新解析七报告的版本表，全部覆盖46个tag；Client/UI与工具补审的commit列均按peeled commit核对，一致，报告内局部链接无缺失。重要发现和限制已合成到report、backlog。

活动特性最终复核：两功能实际diff经独立luna review无实现阻塞；父运行当前默认CLI check/build、工具/runner/checkpoint与最终真实CLI3用例通过，CLI组合覆盖child调用set_session_model后最终effort=None再省略resume。功能验证和审计范围分别记账。

归档前 `openspec validate --all --strict --no-interactive` 18 passed，0 failed；`cargo fmt --all -- --check`、`git diff --check`无输出通过。

本地归档成功：`openspec archive allow-session-model-effort-control --yes`更新model-sampling/session-timeline/tool-authorization（+2 requirements，~3）；`openspec archive audit-grow-architecture-across-versions --skip-specs --yes`不改业务契约。目录均为2026-11-05前缀。归档动作本身的最后checklist项当时保留未勾选，CLI提示8/9、11/12；初次archived检查因此仅这两项失败（846通过），归档与链接核对完成后勾选，重新验证通过。

最终 `openspec validate --all --strict --no-interactive`：16 passed，0 failed；`openspec validate --archived --no-interactive`：848 passed，0 failed，含本轮两项，串行链exit0。搬迁后修复报告root/spec/peer/backlog相对引用，25份本轮相关文档本地目标全部存在。没有实施backlog无关修复或真实发布。

## 宿主Goal收尾观察

代码、测试、两项本地归档和全部计划任务已完成，所有实际交付已accepted/released；调用update_goal Complete时，宿主仍拒绝完成标记：旧失败reviewer `01a1a77a-72ff-7692-90cb-dbbcfd304e7e` 有未处理parent update，released task仍要求execution/wait disposition（错误重复列出两次）。该reviewer已由独立补审完全替代，不提供本报告发现证据。

当前工具观察：wait_subagent返回finalization_pending，错误 `ChildResult/Unknown: timeline persistence acknowledgement was lost`；kill返回 `already_exited: Subagent already finalization_pending`；send(interrupt=true)返回target_inactive。父尝试reasoned no-change确认replan被todo_write拒绝 `only the receiving child may acknowledge replanning`。因此不能在现有公开工具下同时提交child终态与确认残留replan。父没有删除link/重写Timeline/伪造验收或改变主Goal状态以绕过检查。该运行态限制不改变已归档代码、功能和审计验收结果；Goal完成标记尚未成功。需核对宿主实际构建身份与受支持恢复路径，作为独立backlog观察，不直接归因本轮源码某版本。
