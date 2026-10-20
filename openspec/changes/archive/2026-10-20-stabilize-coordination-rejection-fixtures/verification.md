# 验证

## RED

正式 v2.4.3 首轮 run 39422108840、Windows native job 118625552333：Shell coordination 56 passed、2 failed；inquiry_uses_one_info_request_sideband_without_mutating_parent_surface 与 cross_workspace_approval_offers_only_one_shot_choices 均在原五秒 deadline 返回 Elapsed。夹具使用 support::create_test_actor 默认 http://localhost，并没有控制该端点或其失败耗时。

## 改动与审查

仅改 coordination.rs 的 cfg(test) 模块。复用既有 MockInferenceServer 在随机 127.0.0.1 端口就绪后排队 HTTP 400，并把两个 actor 的采样 route 指向该端点。两项均持有 server 至测试结束并断言 request_count==1；保留原 Failed、单 attempt、InfoRequest、Surface 不变和 allow-once/reject-once 审批断言，五秒期限未变。生产代码、网络策略及权限语义不变。

## GREEN 与执行边界

`cargo test --locked -p shell --lib coordination -- --test-threads=4` 完成编译后，默认测试线程栈在新 mock/actor 调试 future 上溢出，保留日志，未计为通过。

按仓库已有 docs/development.md 及 Core/native workflow 的固定 `RUST_MIN_STACK=16777216`，在 Shell package cwd 直接运行本次 Cargo 输出的同一 libtest 产物：

`RUST_MIN_STACK=16777216 <built-shell-libtest> coordination --test-threads=4`

结果 58 passed、0 failed、0 ignored，3.04s。两个原失败用例均实际通过；这沿既有 CI 测试栈配置执行，没有增加业务 deadline。指定文件 rustfmt --check 和 git diff --check 通过。Windows 的实际 GREEN 以新候选的正式 native 门禁为准；不拿本地测试代替跨平台通过。

归档后严格校验：Totals: 16 passed, 0 failed (16 items)；Totals: 664 passed, 0 failed (664 items)。

## 正式候选 Windows GREEN

源码 `2069c43db5a3756bf65d1a275e8ed16bef3a86fa` 的官方 [run 39425379616](https://github.com/LordCasser/grow/actions/runs/39425379616)、Windows native job `118636674253` 已 completed/success。日志确认两个原失败用例均 `... ok`；Shell coordination 为 58 passed、0 failed、0 ignored，3.15s。该 job 的实际独立进程发现回归亦通过。此记录仅确认该门禁，整版发布以发布准备验证中的最终流水线及公开产物证据为准。
