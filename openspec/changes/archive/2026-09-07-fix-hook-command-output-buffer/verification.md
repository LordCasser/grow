# Verification

- 源码确认旧实现 wait_with_output 全量收集后 truncate_output，显示长度限制不约束读取缓存。
- 新 capture_output 回归通过 duplex 128-byte 管道传递 0、64 KiB、64 KiB+1、512 KiB 数据，验证保留长度/容量上限、前缀内容、截断标记；写端完整完成证明超出部分持续排空。
- 既有真实子进程大 stdin/不读 stdin 且大 stdout 回归、超时和 process-scope 测试通过。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：210 单元测试、13 集成测试、1 doctest 全部通过，0 失败/忽略。
- 首次编译 IO error 推断不明确，已为合并输出显式指定 std::io::Error。
- 本次限制命令输出原始字节缓存；未限制 HTTP 响应、envelope 序列化或进程自身内存，相关 HTTP 读取问题已另记 backlog。
