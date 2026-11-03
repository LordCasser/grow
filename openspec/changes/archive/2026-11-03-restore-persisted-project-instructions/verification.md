## 实施与证据
Source 从 ChatState 当前有效 Surface 取得 typed 规则，单块保留原字节；spawn 从相同投影设置自有 classifier，旧任务不读新磁盘规则。新 child Seeded 初始化保持原行为，mirror 不改变。未重写 checkpoint、未绕过 source/operation/权限验证。

所有 Cargo 运行使用隔离临时 GROW_HOME 与 RUST_MIN_STACK=16777216，不访问真实用户 session。

- `cargo test --locked -p shell --lib project_instructions -- --test-threads=1`：6 passed（规则投影、root所有权、cold恢复；当时尚未加入新child测试）。
- 最终 `cargo test --locked -p shell --lib project_rules_recovery_tests -- --test-threads=1`：2 passed；cold内部 none/single/multiple 各 suspend/restore 独立进程，公共 load恢复、原operation/checkpoint身份和真实分类器请求均核对；new/normalized/mirror 真实Spawn后捕获首个provider请求，核对typed规则和wire规则，随后挂起排空。
- 最终 `runtime_suspended_waiter_tests`：5 passed，真实来源漂移与foreign operation仍拒绝；原未知结果不重跑。
- 最终 `permission_auto_mode_tests`：22 passed；`agent::subagent::`：239 passed。
- `openspec validate --all --strict --no-interactive`：18 passed。

第一次新增child测试用echo等待permission，但只读命令被合法放行后重复tool ID，fixture超时；已改为provider请求处同步阻塞。不把该timeout记作有效旧实现失败证据。Cargo测试编译出现既有fetch_update弃用警告和大型测试二进制unwind表linker警告，不影响上述结果。

独立审查发现并修正fresh child盲区，见review.md；审查与实际运行证据分开。

归档后全量 strict 17/17、archive strict 781/781 通过；`git diff --check` 通过。
