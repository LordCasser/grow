# Design

## Current owner and evidence

`turn/mod.rs`以`sampling_continuation.borrow().clone().unwrap_or_default()`开始模型循环，并把共享RecoveryBudget存回SessionActor；每个已接纳步骤后重置。正常Turn终态在`turn/admission.rs`确认`emit_turn_ended`之后take turn_continuation和sampling_continuation。`tasks_cancel.rs::cancel_running_task`在step_control_gate内abort原producer，独立提交取消terminal，但略过这段退役。因此下一Turn会读到原counter，原计数和deadline跨Turn泄漏。

真实证据为`/tmp/grow-sampler-cli-verify.0znoP9/03-test.log`和`04-test.log`，二次actualexit101。04同session 01a1ab40-1ece-7ae2-bae2-ef300c328ff5：取消A=`8ae1d7e7-ca68-49b1-9bdf-640ff4c94076:1`；新Turn B=`d1d6ba34-8543-48ef-951a-49c56b3ee65f`的503直接B:2、成功B:3，不存在B:1；后续C恢复:1。真实HTTP为5次，四个逻辑RequestId，旧工具未执行、候选未进入history、resident epoch/PID不变，取消和503均以未知用量结算。测试及2337项Rust/manifest/lock快照稳定，显式binary SHA256 `0cb021c4e645fc662464f5b679ca738e3e3733c3632464b89f9c27c3d3bce7d8`。

## Decision and invariants

职责留在已拥有实际取消终态的SessionActor。真正Turn terminal durable ACK成功后、下一Turn准入前，退役该结束Turn的turn/sampling continuation，与现正常epilogue一致。退役只释放actor引用，不取消或重造旧attempt，也不重置它已有的RecoveryBudget；其task、结算和evidence owner照常完成。单纯cancel请求、停止Compaction/Idle/Settling、terminal提交失败不算成功退役，不靠输入请求或重新读取设置重置共享预算。

同一未接纳模型步骤的传输重试/session修复/挂起恢复仍共享原RecoveryBudget、原绝对期限和replay_safe；真正新用户Turn必须独立。不得在每次sampler提交或runloop循环入口无条件归零，否则破坏这条合同。没有必要给预算增加全局nonce、第二张Turn map或新schema；现terminal owner已有线性边界。是否将现两处take提炼局部helper由coder按实际重复必要性判断，不为两行复用建层。

## Coder Task Spec and review

单个Shell功能：native cancel结束Turn后，新Turn具有完整独立恢复额度。生产写面仅`actor/tasks_cancel.rs`的真实终态退役及同一行为必要既有局部测试；可同步该owner注释。新真实CLI测试`test_sampler_request_lifecycle.rs`已由原verifier释放，作为本项回归，保持原取消/工具不执行/epoch/history/原[1,2,1]和5结算集合断言，修正已确认的title Sideband evidence过滤方式。原实现red已取得，不必再跑一次等价red；若先补更小owner测试，必须真实取消路径和终态确认，不只测试take helper。

生产禁止改Sampler、Timeline validator/关闭条件、suspension恢复、真实provider、Hook执行逻辑、协议/schema及其他审计问题。若证据推翻terminal owner边界，先报父修约。父先按索引任务当前独占Cargo窗口串行交接；写Rust/编译需父明确release后开始。

Tier3验收：有界session owner测试应证明取消terminal成功后continuation退役、新Turn首attempt1和一次503后2；terminal失败不发布成功/不重新准入，非Turn stop不改变保留续跑状态。已有suspension定向回归核同一步恢复预算/期限保持，选择相关项不跑全量历史。修复后以固定1.93.1/offline/locked/defaulttarget构建新defaultCLI，显式GROW_BINARY跑现同一exact产品场景；新源码hash、新二进制来源、真实exit/日志、请求/settlement/evidence三者身份一致才通过。不能拿旧0cb021二进制证明新生产修复。

原Review Plan安排Luna独立review。实施在途时，用户于2026-11-05明确“停止新审查，当前作为一个里程碑，记录。然后准备收尾，以发布新版本”；因此不再派新的review。当前单项由父亲核最终精确diff、取消/正常/挂起边界及全部实际证据后验收，不能称为独立review。已在途WS01的Luna review只完成原范围，不扩到本项。生产模块接受与CLI里程碑分别报告，二者均通过后才勾任务、归档该change并继续Sampler change归档。
