## Evidence and ownership

LocalPlan是持久化状态契约；SessionActor接收经过canonical child引用校验的PlanningTaskResult，设置link.result_id并mark_link_terminal，再提交整份计划。公开todo接口允许终态前以空resultId释放交付，历史review保留。已用真实libtool_types-93939267f2585f3c.rlib公共API确认：普通结果到达Ok；Released(None)后同样结果到达报review绑定错；结果先到再Released(Some(ref))为Ok。运行宿主最新状态也从ChildResult/Unknown推进到Receipt/Unknown，但尚无可供当前父tool使用的终态引用。

## Decision

修正在纯LocalPlan review校验owner，不增加actor旁路或人工终态action。result绑定规则：

| review | 约束 |
| --- | --- |
| Released(None) | 保留有界非空reason/revision约束，可与后来任意真实link result共存；不创造result/terminal |
| Released(Some(ref)) | ref必须精确等于当前link.result_id |
| Accepted/NeedsRevision | ref非空、精确匹配且link已terminal |

保持原review append-only、不修改旧Released(None)为新ref。late canonical结果仍必须通过shell::validate_planning_result_reference；mark_link_terminal原有规则处理pending replan，保持Goal/scope对活跃或未知执行的门槛。放宽所有review的绑定或在发布时改写旧review均损害原事实，不采用。

## Coder Task Spec

只改crates/common/tool-types/src/planning.rs：修正此单项条件并添加有意义状态转移回归。输入为合法带required link plan；先Released(None)再真实形式ref到达/mark terminal/validate_transition_from应通过，review原文不变且未误记Accepted；pending replan处置和live scope门槛保持。反例为Accepted(None)、NeedsRevision(None)、Released(Some(wrong ref))必须继续失败。不改变公共schema/API，不改shell/工具实现或运行存储。父统一build/文档/OpenSpec，coder只跑tool-types定向回归并留命令证据。

## Review Plan

父核对精确diff和公共API对照；luna独立检查绑定条件未宽到其他decision、历史review及canonical边界未被改写。tool-types相关单测通过后父构建当前CLI并复验真实公共API转移。当前宿主2.6.0进程需要采用修正构建并正常恢复才可能解除现场；不能声称cargo测试等于现场已恢复，不覆盖GROW_HOME或手改Timeline。
