# 验收索引

当前状态：功能与实际产品链已验收。Shell 控制反馈、Pager 父工具行、SessionActor publication receipt 和 child bootstrap 顺序均通过模块审查；freshbuild v10 的原 exact PTY 全链通过，父已亲读三帧、正式 receipt 与实际 wire 摘要。旧 v9 故障和失败诊断保留，未放宽产品断言。严格规范校验和归档已完成，契约已合入主规范。

| 场景/边界 | 已执行证据 | 验收状态 |
| --- | --- | --- |
| 父工具 Dispatching→Queued 原位更新、目标/参数保留、重复完成 | [父工具行记录](verification-parent-row.md)：Shell 公开 producer 生成完成事件，七项 feature 测试及 tracker 180 项回归均 actual exit 0，EntryId 与单行数量核对 | 模块通过 |
| 拒绝和未确认完成不冒充 Queued | 同上：Failed 原错误保真，无 typed 输出/目标不符为 Unconfirmed，仅标题 lookalike 不取得工具身份 | 模块通过 |
| child busy admission、closed-Step 应用、下一 provider、effort-only | [Shell 记录](verification-child-control.md)：真实 provider/tool/coordinator/actor 旧缺 Pending 红，host identity 修后绿；固定 actor namespace 与现序列，未增加公共 schema/counter | 模块通过 |
| no-op、supersede、失败持久化、hydration | 六项 actor 控制 actual exit 0；无效 effort 消耗序位、不重复使用；terminal UI append 失败保实际 Applied receipt 但 ui_terminal_durable=false，finish 报错且不发成功提示 | 模块通过 |
| 目标身份、权限及自然退役竞态 | 原 owner 十项通过；原 native 全部 21 项通过。三个旧夹具前提错误由[独立维护](../2026-11-07-align-native-control-retirement-fixture/verification.md)闭合，另有 Model/Effort closed-execution actor 拒绝 | 回归通过 |
| 局部格式 | [格式记录](verification-format.md)：本 feature 函数与七项测试区为 Rust 1.93.1 rustfmt 原文结果，父亲核 diff；既有 advisory dirty 区未动 | 通过 |
| 同 CLI 的 parent Queued→child pending→child Applied 可见链 | [产品记录](verification-product.md)：v9真实旧红及诊断保留；freshbuild v10原exact 1pass，compile/enumerate/execute/runner/CLI均0。父亲读三帧、唯一Applied replay与下一目标model/high请求，2402项源码和产物前后未变 | 产品里程碑通过 |
| SessionActor publication 回执 | [模块记录](verification-publication-receipt.md)：Some 经 Hook、durable ACK、gateway completion 才成功，独立30秒 watchdog；四项握手与十八项原通知顺序回归均 actual exit 0。None 原策略保持 | 模块通过 |
| child bootstrap 发布顺序 | [消费方记录](verification-startup-order.md)：原native21、新启动五场景、Subagent模块252项均通过，父接收原串行命令actual exit 0；回执失败/取消/超时禁止快照和provider，真正结果和actor退役已核 | 模块通过；待新CLI产品验证 |

故障基线 CLI 构建日志 `/tmp/grow-subagent-control-cli-final-build.log` actual exit 0；SHA-256 `52dce97918b50136391395c61372a309ca6427c46298b1e4e1f5efc784f89463`，424578920 bytes，mtime_ns 1793994920533794682；完整身份 `/tmp/grow-subagent-control-cli-final-identity.json`。v9 前后2172项源码/manifest/script不变；基线产物另保存在 `/tmp/grow-subagent-control-pre-startup-fix`，父核SHA-256相同。生产顺序修复完成后必须重新构建 CLI，不用旧产物声称新行为通过。

最终新 CLI 构建日志 `/tmp/grow-subagent-control-startup-cli-build.log` actual exit 0；SHA256 `c5d28154e428a411a8bdd2bddee85cc871a8e3daf9e129b22ac2a601b8f49e7c`、424843624 bytes、mtime_ns `1793999428699276719`。产品 runner日志 `/tmp/grow-subagent-control-product-v10.log` actual exit 0，证据 `/tmp/grow-subagent-control-product-evidence-v10/`；核对 `/tmp/grow-subagent-control-product-v10-verification-state.json` 证明2402项源码/manifest/script内容与mtime、CLI身份均前后相同。

父已读取屏幕和持久证据后完成跨模块里程碑验收，不把编译、枚举或模块 helper 通过当产品显示通过。不声明 Windows 现场或任意真实 provider 部署验证；本项使用隔离 HOME/project 与 loopback mock，不更改真实用户配置、会话或 Goal。

归档：`openspec archive show-subagent-sampling-controls --yes` actual exit 0，合入 client-surfaces 两条 Requirement，目录 `2026-11-07-show-subagent-sampling-controls`。归档前 strict 21/0；归档后 all strict 20/0。归档时保留最后一个“归档/校验”任务未勾选，因此首次 archived strict 仅因该任务5/6返回907/1；完成该实际步骤的任务状态后复查，最终 archived strict **908 passed / 0 failed**、all strict **20 passed / 0 failed**，命令均exit0。日志 `/tmp/grow-subagent-control-{archive,final-prearchive-strict,postarchive-archived,final-all,final-archived}.log`。开发说明已切换到主规范链接，迁移后的两处相邻归档链接已修正，backlog闭合索引已同步；未发布2.6.2。
