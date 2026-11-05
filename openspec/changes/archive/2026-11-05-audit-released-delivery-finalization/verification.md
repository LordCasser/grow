# Verification

## Before repair: actual public API reproduction

环境为仓库Rust1.93.1；直接链接本轮已编译 `target/debug/deps/libtool_types-93939267f2585f3c.rlib`，调用原生产LocalPlan::validate、mark_link_terminal与validate_transition_from，没有复制验证算法。驱动在 `/tmp/grow-plan-release-public-api.rs`，程序退出0表示反例与对照符合预期，**不表示修复通过**。

最小输入：LocalPlan::new(parent,scope)，一个普通step、一个required且非terminal、result_id=None的link；Released(None)带非空reason且reviewed_source_revision合法。真实形式ref为 `{"timeline_id":"child","first_seq":42,"last_seq":42}`，结果到达操作与shell生产路径相同：设link.result_id、mark_link_terminal，然后校验整份计划转移。输出：

```text
result_without_release=Ok(())
release_none_then_result=Err("delivery review is not bound to the current link result/revision")
result_then_release_bound=Ok(())
```

说明问题在历史review与新link结果的状态契约，不是Result真实性或结果原文。shell::accept_planning_task_result在设置ref前仍有完整canonical验证；此实验没有绕过或声称复现所有runtime入口，只复现其最后纯状态转移。

## Current host observation

旧reviewer `01a1a77a-72ff-7692-90cb-dbbcfd304e7e` 最初是ChildResult/Unknown，之后状态推进到Receipt/Unknown（local deliverable publication unconfirmed），30秒wait后仍为finalization_pending。父link.result_id仍null、terminal=false，parent replan revision3未确认，旧历史Released(None)仍在。PATH中binary为 `/Users/lordcasser/.local/bin/grow`，`grow --version`返回2.6.0(5475dff)；这只证明磁盘命令身份，没有单独证明运行中进程映像完全一致。当前代码与v2.6.0的review绑定条件相同。

这组现场证据与公共API反例一致，但没有从宿主读出某次失败提交的完整内部错误或canonical resultRef，不能用它伪造parent terminal。现有kill返回already_exited，send返回target_inactive；父不能代child处理replan。重复release不解决引用冲突，会留下更多同类历史review。源码修复和当前宿主恢复分别验收，仍需宿主采用修正构建。

## Candidate and regression

只改 `crates/common/tool-types/src/planning.rs`，最终blob `dd2783987fcab9d0893b4a09d7fe5e0c1d3d8676`，+123/-1；生产条件仅对Released(None)豁免引用匹配，其他review条件和不可变转移规则未改。

coder使用固定Rust/Cargo1.93.1：`RUST_MIN_STACK=16777216 rustup run 1.93.1 cargo test --locked -p tool-types planning::tests -- --test-threads=1`。同组旧逻辑26 selected中25 passed、1 failed（late-result绑定错误）；修复后26 passed、0 failed、104 filtered out。覆盖release不终止live执行/scope、迟到ref处置pending replan且review原文不变、Accepted/NeedsRevision空/错引用及Released(Some(wrong))拒绝、结果先到的控制。单文件rustfmt与git diff --check通过。父已亲自核对完整diff；独立review和CLI/publicAPI最终证据待补。

首次公共API修复后复验在CLI build结束前仍链接旧 `libtool_types-93939267f2585f3c.rlib`，因此保持旧binding错误，不能算修复后失败结论。unit-test产物不是新普通rlib；后续仅在默认CLI build完成后再链接生产库验证。

## Parent verification

父已直接读取完整diff，生产修改仅同一owner的一处条件，未增加公共API。固定1.93.1默认 `cargo check --locked -p cli` 和 `cargo build --locked -p cli --bin grow` 均通过，无warning，分别1m13s/44.01s，串行链117.97s、exit0。

CLI完成后链接新生产 `libtool_types-3cc93f71f23ef6ec.rlib`，同一驱动使用 `--repaired` 断言模式，输出：

```text
result_without_release=Ok(())
release_none_then_result=Ok(())
result_then_release_bound=Ok(())
```

late case额外断言整个历史review数组等于到达前、link已terminal且ref与control一致。驱动必须与CLI策略一致使用 `rustc --edition=2024 -C panic=abort`；一次未设置panic策略的编译被rustc拒绝（没有执行程序），补齐后exit0。首次修复后驱动误链旧rlib的失败已在前节保留。

独立magpie/gpt-6-luna审查同一最终blob，结论无阻塞：精确review绑定、reason/revision/预算、append-only历史、link结果不可改写及shell canonical校验保持；完整状态转移用例足以覆盖该LocalPlan修复，Goal actor全流程不属于本次运行证据。父接受此静态review，并以真实生产库/CLI复验作为运行证据。

最终cargo fmt --all -- --check和git diff --check通过，归档前全量strict17/17。`openspec archive audit-released-delivery-finalization --yes`成功合入task-planning（+1 requirement）；归档动作之后完成最后checklist，归档后全量strict16/16、archived849/849通过，本change和backlog/开发者说明的本地链接有效。源码修复已验收并归档，以下现场恢复仍未完成。

## Remaining runtime boundary

`target/debug/grow --help`确认恢复指定会话使用 `--resume <SESSION_ID>`，而 `--continue`不接ID。当前尚未改变运行存储、安装binary或重启用户会话。源码/构建/公共API通过也不能证明旧宿主已经采用修正或现场finalization已恢复。

隔离新构建的完整收尾验证已在后续[独立GROW_HOME change](../2026-11-05-verify-released-delivery-goal-completion/verification.md)实际通过：提前release/live拒绝→native取消等待→canonical结果进入Planning→Goal Complete与报告冷读。它不改变当前正常会话旧任务状态。

可用的局部采用方式是在旧会话正常退出、释放writer之后，从本仓库运行：

```sh
./target/debug/grow --resume 01a1a756-ed0a-73a1-9c23-56548602b030
```

这使用项目内已验证构建，无需替换全局安装入口；它仍是需要用户结束当前交互进程后才能执行的恢复步骤，不能并发抢占同一session writer。恢复后必须重新观察旧child的实际terminal/resultRef与pending replan，才可关闭runtime-finalization或完成Goal。若canonical恢复仍失败，应按新具体错误继续诊断，不手改存储。
