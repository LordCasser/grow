# LSP document admission verification

验收基线为 `cbbc751a8d75d60094388f36ed52609d5fde5408`，固定 Rust 1.93.1 与标准原生工具 PATH。生产改动仅在 Documents/client：一次写锁覆盖 plan、同步队列准入与成功 version/end 发布；错误返回不改状态，didSave/pull 在释放锁后执行。旧生产 plan/commit 入口已移除。没有跨 await、等待 server 响应或新增事务实体。

受控测试经过实际 notify_file_change 和 async-lsp 队列，在成功入队后暂停 sender，另线程执行 router 同样的 Documents.version → 真实 DiagnosticsStore.record_push。channel 固定时序，try_read 确认写锁边界；5 秒 watchdog 仅防挂死。旧路径四项全部按预期失败：didOpen covers=0、应为1；didChange covers=1、应为2；versioned/versionless 均复现。相同测试源码在修复后四项全部通过。最终格式化仅改变两处换行，父已核 diff，不改变回归内容。

| 实际命令 | 结果 |
| --- | --- |
| `cargo +1.93.1 test --locked -p tools --lib implementations::lsp::client::document_admission -- --nocapture --test-threads=1` | 旧路径 exit101，0/4；新路径 exit0，4/4；warning0 |
| `cargo +1.93.1 test --locked -p tools --lib implementations::lsp:: -- --nocapture --test-threads=4` | 最终 exit0，105 passed / 0 failed / 2 existing ignored；warning0 |
| `cargo +1.93.1 check --locked -p tools --lib` | exit0；warning0 |
| `cargo +1.93.1 fmt --all -- --check` | exit0 |
| `git diff --check` | exit0 |

定向组首轮为 exit101，104 passed / 1 failed / 2 ignored：新增 Unicode 失败发送/range 用例揭示共享 Python mock 按文本字符读取字节 Content-Length。mock 改用 sys.stdin.buffer 和 byte headers 后重跑通过，保留原 Unicode 输入及全部 range 断言。发送端 json.dumps 默认 ensure_ascii，现有输出长度仍与字节数一致。此项属于本回归所需测试支持，不改生产协议或超时。

最终组保留原 mixed push/pull 用例和2秒 drain，覆盖 stale/preopen/future clamp、UTF-16、全量/增量同步、didOpen失败保持unknown、didChange失败保持版本/language/end与下次真实range。两个既有真实 Roslyn/TypeScript 用例仍 ignored，没有宣称执行。受控测试直接使用 router 的读版本/写store序列，组内 Python 集成另外覆盖真实传输/router；不声称受控测试实际跑过 wire response。

父已读取最终7文件 diff，核对生产锁/错误/释放顺序、red和green原始日志、最终105项结果及零warning，并独立重算全部7文件 SHA256。client `10c4c1ad42ce48523271143b86fb0b27c9d4e43c8a3d19601ba28579b5f39781`，Documents `abe6bc527d14a978e06218a7164d9224c0f84efc69a664ead68fcfe4e5ae293d`，受控测试 `132db2f951b7ebf7ee156f3a1e32a4267680dda3caa77406f48f0d53ad6b7f90`。原始命令、失败/成功日志与完整源码manifest保留在 `/tmp/grow-lsp-admission-01a1ac9a/`；仓库记录独立保留验收结论和限制。

此修复已获父级特性验收，不新增独立审查。它证明并修复源码竞态，不证明该调度是 CI42058981176 的唯一根因。归档前 strict 19/19，归档后现行 specs/changes strict 18/18、全部 archive 864/864，均 exit0。最终2.6.1精确提交的完整Core、跨平台和发布门禁由版本 change 记录。
