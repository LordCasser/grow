# Final core regression

## Environment and original batch

固定 Rust 1.93.1、locked/offline、JOBS2、INCREMENTAL0、STACK16MiB、test-threads4。HOME/USERPROFILE/GROW_HOME 使用各自 `/tmp/grow-262-core-home.*` 隔离目录，RUSTUP_HOME/CARGO_HOME仍指向已核工具链缓存，防止用户发现目录影响测试。八库统一 batch task `01a1b4fc-1e43-7f51-8b29-138522e5e8b0` 编译成功，完整执行后 **exit101**，680.8s，原始 `/tmp/grow-262-core-final.log`；不将其重写为一次全绿。

| Library | Original actual result | Final evidence |
| --- | --- | --- |
| chat-state | 650 passed / 3 ignored | 原 batch 20.06s，0 failed |
| memory | 321 passed | 原 batch 19.64s，0 failed |
| pager | 7637 passed / 15 ignored | 原 batch 141.61s，0 failed |
| pager-minimal | 101 passed / 1 failed | 夹具修正后完整102 passed，见下节 |
| sampler | 295 passed | 原 batch 11.15s，0 failed |
| sampling-types | 329 passed | 原 batch 0.02s，0 failed |
| shell | 4789 passed / 1 failed / 13 ignored | 修正后完整4786 passed / 13 ignored，见下节feature区别 |
| workflow | 69 passed | 原 batch 1.22s，0 failed |

原 batch 的 ignored 未擅自启用或删除，不称为全平台测试。各功能另有明确显式执行的 ignored CLI/PTY 场景证据。

## Two necessary fixture corrections

1. Minimal transcript 的实际可见label已经为 `src/main.rs`，raw输出的OSC8 target正确保留绝对session路径。原断言对整个raw禁止cwd误拒合法target。只改测试：先严格断言绝对OSC8opening，再剔除此精确opening，对剩余display层检查相对label与无cwd前缀。无通用parser/helper/依赖或renderer修改。父亲读diff后实际 `cargo test -p pager-minimal --lib --locked --offline -- --test-threads=4` task `01a1b507-4b1e-7b41-b421-55a45dbfac8f` **exit0、102/102**；日志 `/tmp/grow-262-minimal-final.log`。
2. Cold authority fixture 在父MCP initialize通知后假定root control handle已发布；实际actor可以先进入initialize，load owner随后等待coordination ACK才插入sessions map。原失败没有区分map缺失与100ms响应超时，不能证明这一次的具体分支；隔离同binary单项实际1/1通过，不能以此吞掉并发失败。修正仅在gate保持关闭时同时推进load与等待root发布，再在原10s阶段总期限内验证即时snapshot活性；保initialized snapshot的pending/cancel100ms、load pending100ms、原gate释放与所有birth authority断言。生产代码与五秒退出期限未动。详见 `verification-authority-fixture.md`。

父亲读完整cold fixture diff后，用隔离HOME和相同四线程实际完整重编/运行Shell：task `01a1b50b-29c5-7c73-a634-8c1a99db589e` **exit0，4786 passed / 0 failed / 13 ignored**，240.96s，原始 `/tmp/grow-262-shell-core-rerun.log`。复跑是独立Shell默认feature；原八库批次因feature unification启用了test-support而多四项，这四项在原batch已经通过。本记录不把不同feature的计数混为同一次运行，也不将既有四项的通过声称为后续重跑。两次改动均限测试观察，无产品行为变化。
