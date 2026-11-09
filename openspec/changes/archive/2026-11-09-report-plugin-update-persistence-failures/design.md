# Update durable outcome

父核 plugin.rs update_plugins_by_selector：load()把坏文件当空，ordinary apply_update_to_registry只改内存，末尾save_registry_or_warn吞失败；marketplace updater现已在自己的事务保存registry。AC-07 actor改为该application后，本项只收紧同一owner的错误边界，不另建updater。

入口用try_load_from(resolve_install_dir)，失败必须先于任何repo更新。维持selector/批次顺序和RepoUpdateOutcome；跟踪本次ordinary真实changed的repo keys。只在ordinary登记变化需提交时保存，失败返回现UpdateError新增的operation failure（detail含保存原因、已更新checkout的repo/path、可能已完成的其它outcomes）。不把Err装成warnings或Updated。无需公共阶段码/新DTO；现调用方通过Display明确错误即可。marketplace既有每repo事务仍负责其已保存/已回滚事实，终端save失败不宣称这些成果已撤销，也不重发它们。

actor在AC-07结果映射基础上将非NotFound错误映射InternalError、reload/restart=false；CLI公开run返回Err，不输出本次整体成功。保同tip、pinned/local和空选择无需save。低层Git已推进后原登记save失败不承诺rollback或自动重试，错误说明检查实际状态；故障解除后的显式重试仍按实际HEAD与登记决定，须验证能收敛且不丢plugin map。

Coder单项写scope：plugin.rs Update及actor Update/CLI必要错误分支和测试；等其它plugin owner释放，禁止改Install/Uninstall或marketplace transaction实现。Tier3：本地bare remote推进+现save fault，真实ACP与CLI公开dispatch错误，HEAD/旧registry字节/消息部分完成对照；坏registry无Git副作用；故障解除显式重试正常采用，第二次同tip no-op；既有marketplace transactional回归保持。优先旧红新绿，不用共享helper替代真实caller覆盖。父核error传播/成功边界及partial事实；独立数据完整性review与最终插件产品链里程碑。父维护docs/backlog/spec/archive。

## 实施时的数据完整性复核

Marketplace 事务保存的是完整 registry；ordinary 结果若在循环中提前采用，会被后面的 marketplace save 顺带发布，使末尾重复 save 失败时错误地报告“尚未登记”。ordinary UpdateResult（含完整 plugin map）因此保留到所有 repo 操作完成，再采用到 marketplace 已更新的同一 registry 并保存一次。Git 操作顺序、选择器与 marketplace 自身 save/rollback 均不改变。混合批次回归借既有 `GIT_BIN_PATH` seam，在排序最后的 repo pull 前保存 marketplace 已提交的 registry 并阻塞最终 rename，验证 ordinary 记录仍旧、checkout 已推进、marketplace 仍已提交，解除故障后显式重试正确合并。

既有 `/plugins update` 和 `/plugins uninstall` 也消费新的 operation failure 为 slash-command error；NotFound/confirmation 的原交互保留。
