# 验证记录

主代理使用 Rust 1.93.1 的 cargo 并显式指定同工具链的 RUSTC/RUSTDOC，执行：

```sh
cargo test --locked -p tools --lib implementations::grow_build::task::coordinator::tests -j 2
```

结果：**61 passed，0 failed，0 ignored**。该命令覆盖最终新增的两个回归，以及已有取消、panic、等待者、挂起、finalization 和完成缓存测试。

新增回归 `retiring_owners_share_unique_identity_capacity_with_finalizations`：62 retirement-only + 一个同时持有 finalization/retirement 的 child 后，仍能接纳第 64 个唯一身份；再多一个立即反压且 runner 不被调用。retirement 观察错误与未就绪都保留名额；释放后准入恢复，join 计数不重复；物理退休后未提交 finalization 仍可查询为 Pending。

新增回归 `retiring_identity_cannot_be_reused_after_completed_cache_eviction`：一个退休 owner 保留期间完成 1,024 个普通任务，确认其 completed 缓存已淘汰后，重用 ID 仍被拒绝；解除观察错误后原 owner 正常 join，没有被新任务覆盖。两个拒绝分支均与 runner dispatch 竞速，错误放行会立即失败，不靠挂住或超时作判断。

主代理对实现做交叉对抗性审查，发现并补齐淘汰后的身份重用路径；归属及前端独立审查见 review.md。没有变更 child result、writer drain、回执或收尾完成条件，也没有通过超时丢掉责任。

最终指定文件的 Rust 1.93.1 rustfmt、`git diff --check` 与归档前 OpenSpec 全量严格校验通过。更早的 Homebrew 工具链单测不计为此最终验收。

已执行 `openspec archive account-retiring-subagent-capacity --yes`，合入 session-timeline 的一个 MODIFIED 条款，保留原有三个场景并扩展为七个。归档后 `openspec validate --all --strict --no-interactive` 为 17/17，通过；`openspec validate --archived --no-interactive` 为 804/804，通过。归档清单检查发现手工创建的 capacity change 缺少 schema/created 元数据，已补齐 `.openspec.yaml`；tasks、文档和主规范合入内容均核对。
