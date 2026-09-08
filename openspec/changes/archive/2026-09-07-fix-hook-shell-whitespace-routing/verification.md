## 失败证据
- 旧 shell_whitespace_commands_execute 失败：printf\tok 被解析为临时目录下文件路径，返回 command not found。
- 旧 relative_command_dedup_uses_execution_base_only 失败：相同 tab 命令跨来源保留 2 项而非 1。

## 最终验证
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet：230 单元 + 13 集成 + 1 doctest 全通过。真实 Unix 子进程覆盖 printf tab 参数和 cat LF true，多目录去重矩阵覆盖二者。既有直接路径、绝对路径、空格及变量命令回归维持。rustfmt 与 git diff --check 通过。

## 边界
本轮仅修复空格/tab/LF 分隔符识别，不声称实现完整 shell 语法分析。运行平台 macOS，未运行 Windows shell。未执行用户配置中的 Hook。
