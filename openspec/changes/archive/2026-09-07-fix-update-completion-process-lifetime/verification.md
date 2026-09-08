## 验证记录

- 旧执行语义：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib test_completion_generation_terminates_on_cancellation --quiet`，0 passed / 1 failed；取消后真实直接子进程仍存活，测试清理后报告 leaked child。
- 修复后：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib --quiet -- --test-threads=2`，131 passed / 0 failed，20.14s。
- timeout 回归另设 12 秒测试等待上限，夹具子进程正常寿命为 60 秒；因此移除生产 timeout 不能靠等待自然退出通过。取消与超时均验证 PID 不再存活及旧文件保留。
- 正常输出回归验证 completions/shell 参数、目标目录创建、成功内容写入、非零退出和空输出不覆盖、无法启动命令不覆盖。
- 所有补全目标使用临时目录，未修改真实 bash/zsh/fish 补全文件；Unix 子进程夹具运行在 macOS。Windows 未执行该夹具，不承诺任意后代树终止或文件系统等待时限。
- `rustfmt --edition 2024 --check crates/codegen/update/src/auto_update.rs`、`git diff --check` 和 OpenSpec 严格检查通过。

开发者解释同步到 `crates/codegen/update/README.md`。此次产物是 main 工作区源码和回归测试，没有替换用户已安装程序。
