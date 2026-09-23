# Verification

## Diagnosis

- 用户会话 `01a0ae68-474d-73b1-94fd-878274dec03b` 的 Timeline 中有 138 个 request、138 个 response、7 个 retry 和 1 个 recovery_stop。
- 第 2621 行的 recovery_stop 为零 bytes、零 chunks，名称匹配；它只因解码允许列表漏项被拒绝。sampler 的 `apply_retry_decision` 正常产生该类型。
- Atlas 的局部查询确认 `decode_record` 被 `verify` 与 `referenced_hashes` 调用；再用源码核对两者分别接入会话完整性校验及导入导出 blob 集合校验。未将 Atlas 的同名通用方法候选作为调用事实。

## Regression

- 初次测试编译发现 light load 返回 Timeline 而非 timeline_events，修正测试访问字段后重跑。
- 修复生产代码前，`session_load_preserves_sampling_recovery_stop_evidence` 在真实 `load_session_without_updates` 入口失败，错误为 `InvalidData: invalid sampling evidence reference`，与用户故障一致。
- 修复后运行 `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib sampling_ -- --test-threads=4`：47 passed，0 failed。
- 场景「Load a session after recovery stops」：新增存储测试覆盖独立观察加载、full load、释放旧 writer 后的 cold writer load，保持原 Timeline 字节和空模型 Surface。
- 场景「Transfer mixed sampling evidence」：扩展既有二进制 sampling blob 测试，将 recovery_stop 与 response 混合，经 `verify`、`read_entity_blobs`、`validate_blobs_column` 校验；仍拒绝缺失和篡改的 body。此测试覆盖证据传输边界，不是网络 ACP 端到端传输。
- 场景「Invalid evidence remains invalid」：表驱动测试覆盖未知类型、名称不符、chunk 数量/长度/hash 不合法、超限，均返回 InvalidData。
- macOS 测试链接器提示现有大型测试二进制 `__eh_frame` 超 16 MiB，测试运行成功。
- `rustfmt --check --edition 2024 crates/codegen/shell/src/session/sampling_evidence.rs` 与 `git diff --check` 通过。

## Local session and build

- 临时 `shell` example 仅调用 `JsonlStorageAdapter::with_root`、`load_session_without_updates` 和 `load_session`，未发布 actor、未申请 writer、未调用 provider。用用户提供的 session ID 和本地 jarde cwd 运行后，light load 成功读取 2757 个 Timeline events；full load 成功读取 2757 个 events、833 个 updates、4 个 rewind points。运行后已删除临时 example 源文件，私人正文未写入仓库。
- 读取前后对会话目录内全部 1124 个文件计算 SHA-256，文件集合及哈希完全相同。
- `cargo build --locked -p cli --bin grow` 成功，产物 `target/debug/grow`；链接器同样给出 `__eh_frame` 大小提示，无构建错误。未替换 `~/.local/bin/grow`，未停止现有 leader。
- `target/debug/grow --version` 成功输出 `grow 2.1.11 (3024dad1) [stable]`。

## OpenSpec

- 归档前 `openspec validate --all --strict --no-interactive`：20 passed，0 failed。
- `openspec archive fix-recovery-stop-evidence-load --yes` 已合入 session-timeline 规范并归档。
- 归档后 `openspec validate --all --strict --no-interactive`：19 passed，0 failed；`openspec validate --archived --no-interactive`：345 passed，0 failed。
