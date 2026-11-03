## 最终实现
Spawn记录可区分缺事实和已知空集合的出生MCP名称；同一个在admission冻结的server/client-ID样本供父ceiling核对、持久名称和actor初始ceiling使用。冻结scope不接纳live同名新ID/新增服务；当前移除立即不可用。cold恢复在require原名字后与当前许可求交，重新冻结本进程incarnation，不持久旧client ID或凭据。

## 实际验证
所有Cargo使用独立临时GROW_HOME和RUST_MIN_STACK=16777216，无真实用户session读写。

- `cargo test --locked -p mcp --lib -- --test-threads=1`：179 passed，包括原普通pool live rebind/addition及新初始化竞态测试。新测试从旧pool map已失效、冻结后移除、重复freeze、同名replacement/新增服务、restrict/exclude保持ID到cold重新绑定均作断言。
- Shell `frozen_child_mcp_grant`：1 passed，直接父旧ID ceiling拒绝当前新ID，原已冻结grant不被重采样。
- `suspended_child_cold_restore_retains_birth_authority_end_to_end`：1 passed，内部empty/narrow/removed三场景，各经过suspend、missing-RWX、invalid-RWX、missing-MCP、restore五个独立进程阶段（共15阶段）。真实HTTP MCP、公开Spawn/Agent切换/suspend/load/后代；核对实际持久出生值、pool、当前client-ID ceiling、真实MCP调用和后代出生集合，extra server调用计数为0。
- Shell `mcp_catalog_lifecycle_tests`：12 passed；`subagent_bash_permission_tests`：12 passed。
- `openspec validate --all --strict --no-interactive`：18 passed；`git diff --check`通过。

最初错误版本仅传同一pool、记录旧map，经独立审查修正；初版正向fixture被补值也已修正，不把其首轮绿当最终同源证明。修复fixture备份路径时第一次错误使用parent测试进程的全局GROW_HOME，未找到文件；已改为显式child temporary home路径，最终15阶段全部通过。

## 契约影响
新Spawn始终记录已知集合。旧挂起child缺少出生MCP事实时执行型cold restore明确受阻；不猜当前配置、不改写checkpoint或补造历史授权，历史只读投影仍可读。普通root/fork不因该child事实字段新增而增加授权。此项有意选择事实完整性，不实现无依据迁移。

归档后全量strict 17/17，archive 784/784通过。
