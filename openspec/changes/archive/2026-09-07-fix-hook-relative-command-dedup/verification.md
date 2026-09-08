# Verification

- 旧实现定向回归失败：不同 source_dir 的 check.sh 只保留 1 项，预期 2。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：222 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 新矩阵覆盖 check.sh、bin/check.sh 跨目录分别保留，同目录重复合并；相同绝对路径、shell命令和变量shell表达式跨目录仍去重，首项优先保持。
- 执行入口与去重共用原 shell 判定，未修改判定字符集。实际 direct 分支仍 source_dir.join(command)，shell 分支仍使用 workspace cwd。
- 新回归验证注册集合，不声称启动了两个目录里的真实脚本；既有命令执行、来源优先级及缺失raw回归全通过。
