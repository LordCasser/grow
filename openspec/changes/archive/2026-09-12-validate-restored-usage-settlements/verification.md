# 验证记录

## 实现与审阅

用量结算沿用原 typed schema。Settlement 共用 exact duplicate / conflict 检查，恢复 fold 返回 TimelineWriteError，actor launch 之前传播失败。scope/name 未知的诊断 Observation 不受影响，已知结算缺失、类型错误或额外字段不能静默漏账。控制标记按其现有无数据格式核对。

主 agent 核对 realtime mutations、from_timeline、spawn launch 与 MockTimelinePersistence；低成本 subagent 只读复核归属与漏构造点，没有代替主审。没有改变已存在的 prompt/session 计费归属、Goal 或 Sideband 账本。

## 已执行

`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --lib -p chat-state -- --test-threads=4`：483 passed、1 ignored。新增真实恢复场景覆盖 attempt/child 相同与冲突结算、跨 resume 去重、已知载荷损坏、marker 异常、未知诊断；拒绝路径断言 actor 事件和 persistence records 均未发布。

shell 全量库回归：3802 passed、1 failed、3 ignored。唯一失败是 `subagent_usage_fold_attribution_gate` 使用同一 child 重复结算四次却期待 160 tokens；实际账本正确为 40。这与身份幂等契约矛盾。保留三个 prompt 归属场景，改为不同 child，另补旧 child 重复不累加断言；未通过修改产品接受重复计费来消除失败。

定向复验 `cargo test --locked -p shell --features test-support --lib subagent_usage_fold_tests -- --test-threads=4`：16 passed，包含修正后的 attribution gate 和新增重复结算断言。归档前 `openspec validate --all --strict --no-interactive`：21 passed，0 failed。测试期间使用同一 target，没有新建副本或读取真实用户会话。

归档完成后：`openspec validate --all --strict --no-interactive` 为 17 passed、0 failed；`openspec validate --archived --no-interactive` 为 327 passed、0 failed。`git diff --check` 通过。原有三个进行中的 change 保持不动。
