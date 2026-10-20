# 验证与复现

## 环境与基线

- macOS / Apple ARM64；Rust/Cargo 1.93.1，通过 PATH 前置 ~/.cargo/bin。
- v2.4.0 `cd5d88ec`，v2.4.1 `5fcbd12b`，HEAD `02432cb6`；包含本会话上一轮未提交的挂起修复。
- 本轮审计未修改生产行为；测试通过临时 cfg(test) path module 接入，执行后删除接入点。没有修改或强退原会话。

## 执行结果

首次执行 Goal/节点挂起探针，2 passed / 0 failed。随后执行：

```sh
PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p shell -p tools --lib audit_post_240 -- --nocapture --test-threads=1
```

- Shell：3 passed / 0 failed / 4305 filtered out，0.39 秒。
- Tools：1 passed / 0 failed / 2433 filtered out，0.01 秒。
- 编译 1 分 04 秒；无完整应用/全仓/跨平台测试结论。

这些 probe 的断言是确认当前缺陷或协调契约，**通过不表示产品正确**。输出：

```text
AUDIT tree freeze: first=01a1580f-6169-7990-945d-74a25bdbe660 retry=01a1580f-6169-7990-945d-74b21696a0d3, durable phase still Running
AUDIT Goal activation: live submission=1, durable submission=0, live retained=true, durable retained=false
AUDIT pre-intent Suspended: unknown result committed; retry checkpoint has no human wait
AUDIT real coordinator: new owner rejected, original owner retry accepted
```

树级首版 probe 注入首次 freeze ACK 丢失，调用方为真实 MvpAgent；接收方冲突契约由真实 coordinator 独立探针验证。另用“child 初始化超出首轮期限”替换 ACK 故障注入，验证无需假设 coordinator 异常退出也能到达相同缺陷，结果补在下节。

## 接入位置

归档中的源码不是常驻产品测试，不会默认编译。需要复现时，临时以 `#[path = "<绝对 probe 路径>"] mod audit_post_240_probe;` 接入对应父模块，并运行上面的定向命令；最后删除接入点。

| Probe | 临时父模块 |
| --- | --- |
| goal_probe.rs | crates/codegen/shell/src/session/actor/goal.rs，cfg(test) |
| suspension_probe.rs | crates/codegen/shell/src/session/actor/suspension.rs，cfg(test) |
| tree_probe.rs | crates/codegen/shell/src/agent/mvp_agent/tests.rs |
| coordinator_probe.rs | crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs |

## 证据边界

- Goal probe 跑完整 initialize_goal_runtime 和最新 durable Control 查询；没有用手工构造坏快照替代生产调用。
- 节点 probe 使用真实 pending guard、ChatState、owner drain、suspend_node 和同 op 重试。前置 runtime_suspend/interaction 顺序由树协调和真实 worker 源码交叉核对；没有实际权限对话或 provider。
- 树 probe 以可控 coordinator 传输回复驱动真实 caller，另跑真实 coordinator 接收方。没有声称同时运行完整树的真实 subprocess。
- 既有 release 全量、图片/Skill/平台回归只读其归档证据并做差异审查，本轮没有重新运行。
- Atlas 仅用于局部符号定位；suspend_session_tree 查询定位成功，Goal helper 查询未匹配，后者以原始源码核对。

## 普通初始化超时复验

`cargo test --locked -p shell --lib audit_post_240_tree -- --nocapture --test-threads=1`：**1 passed / 0 failed / 4305 filtered out**，0.07 秒。当前 tree_probe.rs 保存此版本：coordinator 回复 initializing child，首轮 50ms 期限内保持相同 owner，调用方因初始化未完成返回；下一次请求却生成另一个 owner。输出：

```text
AUDIT tree freeze: first=01a15813-4400-7e13-8dcf-ad01fed881d7 retry=01a15813-4432-7833-bc9b-fe1e650e2339, durable phase still Running
```

所有临时接入点已删除；goal.rs、MvpAgent tests.rs、coordinator_tests.rs 与 HEAD 无差异，suspension.rs 仅保留上一轮已有修复。

## 文档校验

归档前全量严格校验 17 项通过；以 skip_specs 归档后，全量严格校验 **16 passed / 0 failed**，archive 校验 **652 passed / 0 failed**；git diff --check 通过。基线源码再次核对：v2.4.0 已在 root SuspendRequested 前关闭 runtime_suspend、按 Running 生成新 operation；权限 observer 已用 runtime_suspend 写 Suspended；节点仍从当时 registry 取快照。因此三个发现均可追溯到既有机制，不错误归因为新补丁引入。

构建清理：`cargo clean -p shell -p tools` 删除本地相关包的 2,076 个编译文件，共 9.4 GiB；未清空整个 target。
