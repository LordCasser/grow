# Isolated investigation evidence

生产修复、定向验证及独立review已完成。原prefix完整探针的正常终态断言未通过，关键加载行为已有直接证据，结果边界见下文。正常会话及用户运行状态未恢复或改写。初始诊断使用ignored probe，发布收尾时已将该一次性夹具原字节保留为[local_lost_owner_probe.rs.txt](local_lost_owner_probe.rs.txt)，移出正式Cargo测试图（35941bytes，SHA256 `8e84443fdf1c2cd2a6103ec1e46070db0bca8a8e02462c3b32f30b0b2a2ba785`）。原路径为`crates/codegen/shell/tests/local_lost_owner_probe.rs`，初始fixture和运行输出在 `/tmp/grow-lost-owner-probe-id0a9sjq`。源facts复制prefix物理64448/domain48629（SHA256 b59f4c2def0e973d26ae809cde3a432eaaa3e11efcebd64a480d70e2203a6f8a）和物理64464/domain48643（SHA256 45d091ebcc1dacdd075557be9a1a8f91db8a8b6149dd484a15e4ce82692edc73）。manifest记录summary隔离cwd/grow_home并去git投影，不改源事件字节。初始诊断两个load实际provider请求均为0、新MCP配置为0、新Hook handlers为空；旧snake_case CLI profile未证明被选中，不能据此宣称旧运行已完全禁用技能/工具发现。最后单prefix验收改为公共resolver确认同名camelCase夹具并施加现operator工具clamp，保留这一隔离差异。

首轮fixture：固定Rust1.93.1调试构建，原prefix storage读取成功48630events/296Surface（96.563s），空PostToolUse HookTriggered pure验证成功domain48630，独立coldledger durableappend成功44.248s。随后probe自身TOML带点模型键未引用导致catalog缺模型，尚未跑实际load，明确是夹具失败。慢时长不能证明原release热writer30s原因；不得先改产品deadline。

修正夹具后的第二轮：`attempt-2-cargo-probe.log` 已记录prefix48629 load成功130141ms，35s后prompt_status走durablefallback返回unavailable（超过boundedreadbudget），mockrequestcount0。父亲读tracing：line62 03:52:42.577476Z notificationdrain内部prompt的user-message commit被`timeline event violates the causal fold: invalid notification fact`拒绝；line169 03:52:44.811135Z run_loop关闭fatal Timeline turn-boundary；line170 03:52:59.814821Z supervisor reaps DeadFailed exitClean。因此loadACK不等于actor持续健康。

第二prefix已收齐，同样在新通知Turn消费时发生InvalidNotification、fatalteardown、DeadFailed。父已亲读attempt-2-cargo-probe.log：两个load分别130141/130815ms返回OK，+35s已失resident转durable budget-unavailable，各provider0，总428.92s。原prefix已有AgentReply receipt域48597，343bytes；后置Recovery的correlation_id等于新Turn。最小无child/磁盘/provider的四事实控制已运行0.01s，1passed：相同消息活Turn消费成功，无界recover后相同consume为InvalidNotification、pending1。父已核对acp_agent.rs1371→runtime_handoff.rs234→subagent/mod.rs4052以及Timeline active_turn检查；接受加载后退出机制，不推广到原11:07 firstfailure。

两个只读monitor终态尚未确认：01a1a9cf-e571-7a21-a5aa-c4065e5de4c4、01a1a9d4-c684-7ad3-9556-2ac552395a9f。child及父的受支持kill接口均被capability拒绝Subagent cancellation was not authorized，不旁路操作；Cargo/rustc/probe已退出事实与它们分开记录。

修复采用精确历史Workflow结算。修订delta通过单change strict和全量strict（22 passed,0 failed）；ChatState owner `01a1a9e2-3c09-7e83-9ce9-312bf22fcd76` 已落地约定API，父亲读prepare/guard与terminal计算，签名冻结。Shell owner `01a1a9e6-8d98-7291-819f-524ed86b0c33` 已按此接口接手loader/reconciler单项调用链；两个write scope互不覆盖，Cargo串行。原prefix后验已执行，完整探针失败与关键加载行为证据分开记录，未恢复正常session。

## ChatState定向验证

固定Rust/Cargo/rustdoc1.93.1，RUST_MIN_STACK16777216、JOBS2，独立/tmp GROW_HOME和target，命令`cargo test --locked -p chat-state --lib settle_interrupted_workflows -- --test-threads=2`：10 passed、0 failed、0 ignored、623 filtered。Timeline覆盖live通知Turn及所有其他scope保持、childterminal前不结算、epoch/幂等、同epochduration、runtimecheckpoint；actor覆盖同receipt消费、epoch/幂等、checkpoint、精确ACK重试、StorageFull失败保留pending身份并关闭writer。父已读原日志尾部及关键fault测试，没有把未ACK当成committed。

同产物以完整名称`--exact`运行5项既有回归：`timeline::tests::interrupted_workflow_execution_is_closed_by_recovery`、`timeline::tests::workflow_cannot_end_before_its_subagents`、`timeline::tests::interrupted_subagent_is_left_open_for_backend_reconciliation`、`actor::tests::restored_actor_replays_surface_and_continues_event_sequence`、`actor::tests::runtime_cold_load_prefixes_preserve_turn_until_execution_release`：5 passed、0 failed、0 ignored、628 filtered。限定fmt、diffcheck及预存baseline反向apply检查通过。父重新计算最终六文件hash与测试manifest逐项一致；manifest SHA256 `ee5a8bac638ae5092e1049aecdc6ff9895fd4ad3c7b6572dcb0feb19056773b1`，timeline `e14c2e82afd9a98835ccda3acd399548751dc1aad9838afed06aa7c135bf1c27`，actor/mod `dd63b41236adf0c765f17828a461454c0988bd7dca41344c27f4d34598bcd283`，新workflow_settlement_tests `708433f7f6d9bcc909e130d0d57e63a65747cb91742849280e5d69512a6774dc`。原材料暂存/tmp/grow-workflow-settlement-01a1a9e2.HLSxzR。

这些通过证明ChatState的限定结算能力，不证明Shell实际传入spawn前targets，也不代替整体加载链路和原prefix复查。独立luna `01a1a9ea-37fe-7273-8242-620797673abf` 已核对相同六文件hash，未发现生产行为缺陷；指出多可结算目标中“首项已ACK、后项失败、cold同批重放”的覆盖缺口。父核对逐目标提交与原faulttest确认缺口，已单列task3.1a，由`01a1a9f1-1953-76b1-8ff6-8e153742d9d5`只补回归，不改已审production。追加回归`actor::tests::workflow_settlement_tests::durable_settle_interrupted_workflows_replays_partial_success_from_durable_prefix`已通过exact：1 passed/0 failed/0 ignored/633 filtered，父已读实际log并复算最终test SHA256 `8e90c4b93274c100d1cacd8d1398f9c012ff501f8d2e3e31184b2e8710f1a0f7`。首版仅fixture expected类型推断编译失败，显式`Vec<(String,u64)>`修正后原断言保留，production/注册hash未变。用成功手动ACK记录构造前缀，经Timeline::from_events→ChatState::from_timeline→现launch重建owner，刻意不混入自动bootstrap；同批只补第二项，首项不重复，整批再调no-op。此证据为owner重建后重放，并非完整coldload调度，后者由Shell链路与prefix后验覆盖。父接受A片局部实现与review闭环。

## Shell真实reconciler定向验证

最终freeze版本运行`cargo test --offline --locked -p shell --lib agent::subagent::tests::workflow_recovery_tests -- --test-threads=1 --nocapture`：3 passed/0 failed/0 ignored；同工具链5项既有回归逐项`--exact`，各1 passed/0 failed/0 ignored。固定Rust1.93.1、MINSTACK16777216、JOBS2，独立/tmp GROW_HOME/TMPDIR/空Gitconfig。三项为`cold_reconciliation_preserves_live_notification_turn`、`resident_reconciliation_grants_no_workflow_settlement`、`cold_reconciliation_waits_for_canonical_child_then_settles_only_captured_epoch`；五项为receipt repair、backend running、foreign inspection、unavailable completed output、missing unpublished child保持原断言。父已读最终日志，不以零命中作为通过。

无child冷加载用例显式核对bootstrap已正确结束旧Workflow，reconciler前后完整Timeline相等；resident在actor出生后持久创建openWorkflow，空targets重连保持其事实；canonicalchild用例用真实隔离child storage确认result link，先running保持oldWorkflow，child终态后仅捕获old epoch结算，liveTurn与新Workflow保持，exact原AgentReply仍可消费且重复幂等。合法历史Recovery可以存在；禁止的是后置reconciler结束新Turn或改变既有前缀。初版夹具误用notification version1及预置Workflow未考虑bootstrap，均修正为现常量和实际启动前提，生产校验未放宽。

六文件manifest暂存/tmp/grow-shell-workflow-recovery-ky7kw1lb/shell-final.sha256，父已亲算六文件均与manifest一致。最终关键hash：acp_agent `a82aa4207042f79ad370551be5d994dd42457968ad45460a4f23873ed555085f`；runtime_handoff `f7516f1dba30f8d326a128aa4f9d5b6f60820d19013da5f60d2ed1e8851cb37b`；subagent/mod `8219eb0fbb5054f7a7ad8450c2e12aba45eb1ed92b785828d4025085968bf0d2`；workflow_recovery_tests `015f24ab301e2691e7e95a2b1b7f7ca1f94c1351bdbe2a5df961fd8fa7700c2a`。生产及测试freeze，限定fmt/diffcheck通过。独立B reviewer `01a1a9f3-9642-7910-8720-8bfa35398578` 核对最终六文件hash及最终8项通过日志，未发现可行动问题。父已核对实际loader/reconciler、历史targets权限和真实公共API集成；接受B局部实现与review，原prefix后验现已执行一次，关键加载行为与完整探针结果分别如下；未将副本行为当成原正常会话已恢复。

## 原prefix最终后验边界

仅原domain48629/physical64448一个case，216810955bytes，SHA256与初始原prefix一致；新root `/private/tmp/grow-lost-owner-final-c0w70utf`，无正常session加载或写入。父已读corrected日志与trace：load ACK为132165ms，35秒后session/info为found/runtime且id匹配，prompt_status也为runtime。原receipt准确消费一次，新通知Turn未出现后置Recovery或提前TurnEnded。两次loopback Responses POST均返回HTTP200 response.completed。完整脚本保留真实exit101：`outcome == completed`断言失败，实际Host(error)终态为planning_completion_blocked。

这四项required delivery来自原历史，固定无toolcall mock未accepted/released它们；第二次final按已归档`task-planning::Unchanged completion blockers stop automatic sampling`停止模型，属于保留的完成门禁，不是原invalid-notification→actor退出的同一故障。父核对该主规范与trace172/176，不伪验收历史任务，不降低断言，不重跑。正常provider终态的完整成功场景未通过，结果不得计为整个probe pass。

另有严格隔离假设未成立：按fixture ToolConfig IDs clamp后，existing-child动态工具ask_subagent/send_subagent_message仍被注册，RequestStarted记录工具数2；本轮mock没有toolcall，实际新ToolStarted为0、三个新Hook计划handlers均为空（已从完整磁盘domain记录读回复核）。不能声称注册工具完全为空。首次新probe编译误用tool_calls方法，在尚未启动case前修为Assistant字段并拒BackendToolCall；旧编译失败日志保留，未改生产。42:19的persistence actor unavailable发生在probe断言panic引起harness teardown之后，与42:18健康runtime回复及原post-load fatal分开记录。

原11:07首次失去owner的原因仍无firsterror证据。本change解决已证明的加载后恢复竞态，并保留原历史交付门禁和正常session不变；不宣称原进程已恢复或证明所有退出原因相同。

持久事实复核：原receipt在domain48642/physical64463消费一次，来源、reply关联、343byte正文与原immutable artifact一致；新Turn48637开始、48661以Host(error)结束，late Recovery计数0。两个Request分别48645→48649、48654→48658；其response evidence48647/48656均HTTP200 response.completed，usage48648/48657与admission48650/48659均从完整磁盘记录读取，固定正文匹配、response toolcalls0。原prefix hash未变化，生产源码hash与测试/审查冻结值一致；最后持久截点domain48663/physical64486。采样chunk存在和长度及SHA256已核对，离线脚本未独立重算其BLAKE3命名，不能把它写成额外密码学验证通过。

父据实际差分、独立review、11项新增ChatState/3项新增Shell及10项既有恢复回归，和该次副本的runtime/receipt直接证据，验收本change的限定加载恢复修复。完整正常provider完成里程碑未通过，保留exit101及上述原因；不为归档改历史交付或补造provider成功。当前未安装、重启或发布，正常会话保持原状。开发说明已同步到docs/development.md，并链接归档后主规范。

## Delta场景对应证据

| 场景 | 已核对证据 |
| --- | --- |
| Pending notification starts a Turn during cold load | A的live notification/receipt公共入口回归与B的cold_reconciliation_preserves_live_notification_turn；原prefix最终runtime后验另列 |
| Historical Workflow waits for its child | A的settle_interrupted_workflows_waits_for_child_terminal及B的canonicalchild回归第一次running inspection保持open |
| Historical child settlement completes while new work runs | B的canonicalchild回归第二次用实际child result结算oldWorkflow，保留liveTurn和新Workflow |
| Identity advances or has ended | A的Timeline/actor matches_epoch_and_is_idempotent包含新epoch、已Ended/Closed、absent及无关Workflow |
| Repeated or checkpoint-owned recovery | A的幂等与preserves_runtime_checkpoint，及既有runtime_cold_load_prefixes_preserve_turn_until_execution_release |
| Batch fails after a terminal ACK | 新增durable_settle_interrupted_workflows_replays_partial_success_from_durable_prefix exact通过；证据范围是持久前缀重建owner，非自动bootstrap调度 |
| Resident viewer reconnects | B的resident_reconciliation_grants_no_workflow_settlement加loader实际spawn_new_actor分支/空targets静态审查 |

归档收口：归档前全量strict为22 passed/0 failed，归档后全量strict为21 passed/0 failed；archive全量最终856 passed/0 failed。首次archive检查发现本change收尾项和已归档MCP-D1的两项漏勾，父核对真实产物后补齐再通过，不隐藏首次失败。本change的七个delta场景均有上述证据，完整原prefix正常provider终态未通过的限制保持。
