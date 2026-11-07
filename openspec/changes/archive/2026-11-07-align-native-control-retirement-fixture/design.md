# Design

父已亲核`handle_request.rs:2513`先handoff child Result，再`2543`请求RecordSubagentUsage；`run_loop.rs:3127`的CommitSubagentResult同时接管Graceful shutdown，收尾执行结束后actor退役。现测试`session_model_tests.rs:465`只扣usage，`623`却要求child.cmd_tx未关闭，违反既有顺序。已归档`finalize-child-hooks-before-result`要求结果是SessionEnd后的最后canonical事实，不能为测试改回早期runner顺序。

原`finished-during-lock`有生产hostintent补丁撤回时相同失败证据，两个mailbox case在当前源码同样卡于前提；尚无两者的独立旧行为基线。修复时保留此证据强度差异，不称已经证明所有失败来自旧版本。

## Owner与不变量

仅Shell测试所有者调整`agent/mvp_agent/tests/session_model_tests.rs`以及必要`session/actor/model_switch.rs`现模块测试。自然完成后可合法拒绝于Shell target检查、关闭的精确命令通道或仍开放actor内execution gate；每条测试必须证明自己实际进入的边界，不能将另一边界错误冒充actor拒绝。

保留三个明确触发：①已进入准备且等待catalog锁的调用，target自然完成后不能接纳；②model命令已按原handle准备，延迟到执行结束后不能得到queued；③effort命令同样不能得queued或复活执行。任何命令send失败/receipt drop必须明确观察，不能unwrap假定mailbox仍活，也不能把连接关闭当尚未发送的成功。后续持久事实须核唯一原Result、无新model.changed控制、provider原尝试不重跑、父选型不变，最后释放夹具持有的合法usage职责并确认收尾。

若自然完成路径无法稳定保留可分派的mailbox，则用现actor fixture另行证明在相同native Model/Effort admission入口、真实closed execution状态下返回明确拒绝且不发Pending/不改route；集成路径仍保留退休通道的拒绝验证。优先复用现helper，不增加生产注入点。局部排程由coder选择，但不能仅删除前提及后续场景、降低失败语义为任意error或空结果。

## Review Plan

先保现三条实际红；只改测试、运行原native wrapper全部21cases与owner10cases，并核额外actor两类拒绝控制。测试JSON/持久证据沿正式Timeline loader，不能读退役ChatStateHandle并unwrap。退出/cleanup有界，所有命令收actualexit；父亲核diff、触发边界与最终21/21结果。随后pure-maintenance strict/archive，不同步伪造delta或修改行为文档。Shell/Pager共用Cargo按唯一owner串行，P验证期间不得写Shell依赖。
