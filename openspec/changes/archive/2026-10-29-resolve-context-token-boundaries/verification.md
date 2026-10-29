# 四项验证与修复结果

日期：2026-10-29，Rust 1.93.1，macOS arm64。环境优先系统工具 PATH，`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`。

## 四项闭合

| 项目 | 反例与改动 | 实测 |
| --- | --- | --- |
| 低于 heuristic 的完整 provider 回执 | 旧规则10002 vs2500仍取10002；新增滚动校准参考。另复现固定比例在缩小后重新增长时低估，修复为每次投影推进同一参考。 | [原始 red](low-anchor-before.txt)、[regrowth red](regrowth-before.txt)。覆盖低值、0、最大值、追加→压缩→重复构建→再追加、route reset、用量独立；Shell同一响应用量测试同时检查150000与低值1000。 |
| 冷恢复首次请求前 Goal scope | 显式 bootstrap 参数在首次估算前初始化；共享控制授权谓词只允许Goal+Active，恢复协调后 acknowledged 同步最终范围。 | [2项测试](cold-goal-results.txt)：active/completed/paused/absent/normal矩阵，旧revision+当前revision+reasoning；首个AvailableCommands与session info在请求之前正确，历史/调用数不变；RuntimeUnavailable暂停后首快照正确。 |
| 输出预留统一 | calibrated input + post-clamp explicit output <= 总窗口，普通/技能共用最终入口，移除技能特有2048/256。None不猜隐式输出上限。 | [5项准入测试](admission-results.txt)：零派发拒绝、实际wire task clamp为1000、精确边界/None/超出1token。另小技能完整持久化后明确因100k输出+非零输入超100k而零派发，随后按既有SkillLoadSettlement撤下未消费上下文，[技能回归](skills-results.txt)。 |
| restore→AdvertiseCommands→Pager→usage→旧快照 | 物理journal的真实usage事实经MvpAgent::load_session恢复，捕获生产快照、调用实际grow/session/usage，经真实ACP handler和状态/报告formatter；旧帧使用生产共享builder从旧ledger生成。 | [完整跨层用例](pager-resume-results.txt)：初始1500输入/150输出→新回合后2400/240，cached read1420；partial cache、incomplete保持；两个run分段；空resume同总量旧帧和新调用后低值旧帧均不回退，旧帧后再次查询usage与报告不变。 |

第四项用真实可达的旧快照已通过现有Pager过滤，因此没有添加猜测性的排序状态。新增共享通知builder与测试dev依赖用于复用实际producer，不改变wire payload。async-trait本来已存在于锁定依赖图，仅为Pager dev-dependency登记一条依赖。

## 最终验证

测试产物通过 `cargo test --locked -p pager -p shell -p chat-state --lib --no-run` 生成，再直接执行Cargo返回的二进制路径，区分构建和测试阶段。以下集合存在交集，不相加宣称总覆盖数。

- ChatState 全量：**599 passed / 0 failed / 3 ignored**，22.05s，[输出](chat-state-results.txt)。
- Shell compaction：**118 passed / 0 failed**，24.06s，[输出](compaction-results.txt)，包含前轮真实文件/SIGKILL恢复回归。
- Shell skill loading：**15 passed / 0 failed**，2.60s，[输出](skills-results.txt)。
- Shell response token usage：**13 passed / 0 failed**，0.23s，[输出](response-usage-results.txt)。
- Shell cold Goal：**2 passed / 0 failed**，0.69s，[输出](cold-goal-results.txt)。
- Pager usage相关：**86 passed / 0 failed**，0.97s，[输出](pager-usage-results.txt)，包含新跨层用例；单独跨层1项也通过。另transient_context_pressure用例通过。
- 默认生产配置 `cargo check --locked -p cli --bin grow`：通过，1m25s，[输出](production-check.txt)，未用测试feature替代默认生产图。
- 相关Rust文件格式检查和 `git diff --check`：通过。

## 夹具与审查

跨层测试初版没有stable System head，被生产加载校验正确拒绝；补入合法seed。实际加载后首回合还触发标题辅助调用，多出10输入/5输出属于真实消费；改为持久化User来源标题隔离该可选行为，不放宽总数断言。技能失败后上下文会被正常撤下，测试以 durable Input::Consumed 和终态settlement证明先发布再拒绝，不要求错误后保留未消费技能。

独立审查确认单一滚动参考、零/最大值算术、route/rewind reset、重复投影与账本独立；bootstrap只读恢复授权、不从历史tag猜状态。缩小的ceil可能保守舍入，不声称启发式等于模型tokenizer。未知隐式输出上限无法凭空保证；已明确为统一准入契约，不留下两个路径的任意默认值差异。

未访问线上provider、未发布二进制；模型交互均为本地脚本化协议。所有已有无关脏修改保留。

## 收尾

- OpenSpec 已归档：修改model-sampling的3项契约，新增client-surfaces恢复usage契约。归档后全量严格校验16 passed / 0 failed，archive校验722 passed / 0 failed。
- 最终diff检查通过；cargo clean删除15,555个构建文件，回收6.6 GiB。
- 四项原backlog已更新为闭合并链接本验证，历史审计结论保留作追溯。未提交、推送或发布产物。
