# 验证

2026-11-03，隔离临时GROW_HOME，RUST_MIN_STACK=16777216，Cargo使用--locked。以下为最终实现对应的真实运行结果，不将静态review视为测试。

| 入口 | 结果与边界 |
| --- | --- |
| agent --lib 全量 | 484 passed；selector/plugin匹配helper及原发现/构建回归。 |
| shell --lib dependency_admission_tests | 1 passed / 5 OS进程阶段；同目录symlink、异实体拒绝、实际目标变化拒绝、缺Agent拒绝、原role/rules及显式重选、旧key改指向但新alias精确匹配。 |
| shell --lib subagent_restore_authority_tests | 1 passed / 30 OS进程阶段；empty/narrow/removed/alias/changed-model × suspend/缺RWX/无效RWX/缺MCP/plugin替换/restore；真实原call权限恢复、same-manager失败修复重试、当前模型取checkpoint而非birth、旧birth profile移除、出生grant/clamp及exact once。 |
| shell --lib agent::models::tests | 14 passed；原key优先、改指向时exact alias、saved effort。 |
| shell --lib runtime_suspended_waiter_tests | 5 passed；真实route漂移、foreign operation、post-checkpoint观察不能替代checkpoint目标。 |
| shell --lib runtime_restore_tests | 2 passed；accepted final response/原Turn、Workflow冻结来源与原child rebind。 |
| shell --lib actor::suspension:: | 14 passed；receipt、等待重绑、第一次失败、再次挂起及原预算/continuation边界。 |
| shell --lib restored_pre_tool_hook_tests / restored_stop_gate_tests | 3 + 3 passed；缺失/冲突与Unknown side effect拒绝。 |
| shell --lib restore_retry_tests | 3 passed；诊断digest不同可恢复但不重写、prepared/running owner保留、退休取消/异常不转logical teardown。 |
| shell --lib agent::subagent::tests | 177 passed。 |
| shell --lib project_rules_recovery_tests / project_instructions_idempotence_tests | 2 + 4 passed；真实cold规则与新child规则、有效typed投影。 |
| shell --lib model_switch::tests | 31 passed；同Agent更新、重复选择、busy boundary、role shadow及控制回执。 |

原仅篡改aggregate digest的负向fixture按新契约变为positive；实际失败后同进程retry移到缺MCP出生事实的公共fixture，未删除失败恢复语义。初次运行发现Seeded路线baseline缺失；修复生产初始化后全部30阶段通过。独立review另外发现fixture retry再次挂起改变child checkpoint，现每个新进程恢复原始root/child备份字节，不补造positive出生事实。

早期按源文件名过滤suspension_receipt_tests/retry_wait_tests得到0 tests，不计覆盖；改用实际模块actor::suspension::执行14项。OpenSpec归档前全量strict18/18通过；git diff --check通过。测试有既有atomic deprecation和macOS大型测试runner unwind warning，不影响结果。

执行型恢复缺少原路线、出生MCP等必要事实的旧持久任务明确拒绝，不迁移或用当前配置猜测补证；只读历史不触发执行。本机synthetic provider与OS进程fixture，不代表外部真实provider故障测量。

归档后：全量strict17/17、archive tasks785/785通过。
