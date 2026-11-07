# Verification: pass-envrc-paths-as-arguments

- Exact command（父 Shell 共享 target）：`env PATH=... RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1 test --locked --offline -p workspace --lib envrc -- --test-threads=1 --nocapture`
- 结果：8 项测试 7 pass / 1 fail 之前的唯一失败为 `test_non_utf8_dir_name_loads` 的 fixture 创建失败（`fs::create_dir_all` 返回 errno 92，macOS APFS 拒绝非 UTF-8 目录名，`EILSEQ`），失败发生在调用 loader 之前，非 loader 行为缺陷。特殊字符 6 case 与既有 normal/path/expansion 全过。
- 修复：`workspace/src/envrc.rs` 该测试仅在 `raw_os_error() == Some(libc::EILSEQ)` 时 `eprintln!` 提示 "filesystem rejected non-UTF-8 name; loader scenario not exercised" 并 return；其他错误仍 panic。目录创建成功时原 loader/变量断言照常执行（未 skip loader 错误、未吞 load 结果）。
- 最终实际运行：task `01a1af2d-16b0-7d33-b842-7b468c460944`，18.41 秒，**8 passed / 0 failed，exit 0**。父亲取完整输出，确认六类特殊字符目录实际调用 loader 并验证导出值，非 UTF-8 case 明确输出 filesystem 前提失败提示，未将其当作 loader 场景通过。`docs/development.md` 已同步固定相对名、进程工作目录和原 folder-trust 分工。
- 限制：本 host 文件系统无法构造非 UTF-8 fixture；该路径的 loader 行为在支持该文件名的 Unix 文件系统上仍待实跑。目录字节不进入脚本的保证已核源码，不声称本机执行了该场景。
