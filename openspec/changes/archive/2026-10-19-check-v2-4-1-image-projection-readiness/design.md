## Boundary

编译与静态检查在现有干净隔离 worktree 的精确 main 候选执行，避免把共享目录脏状态或另一个任务的临时文件当作发布源码。复用同一源码的已完成图片投影回归；本轮不因纯提交与文档改动重复完整运行千项功能用例。

使用 Rust/Cargo 1.93.1，先默认 CLI check，再 workspace/all-targets check，通过 JSON diagnostics 与 stderr 同时核对 warnings。独立 Cargo target 验证结束后清理。核对 locked metadata/版本继承、相对 Markdown 目标/锚点、发布脚本语法、工作流 DAG/不可变 SHA 与既有精确 runner 测试。没有版本/tag/notes 时只记录完整发布预检的真实阻塞，不伪造 tag 或放宽校验。

托管平台门禁、十平台产物、签名、attestation、checksum 与实际 provider/PTY 的边界明确记录；静态检查和 macOS 编译不代替它们。新增无关债务不在本轮实施。

## Production import correction

Pager Cargo.toml 的 sampling-types 仅在 dev-dependencies 中。上一轮 lib tests 会编译 dev dependencies，因而掩盖 AgentMessageBlock 中三处生产引用的 E0433；默认 CLI check 在此确切候选上失败，另有两条由类型未知引起的 E0282。已有 shell::sampling::conversation 是 sampling-types 的原样 re-export，也是 Pager 的生产依赖。改为从该出口导入同一共享常量，测试与生产共同使用；不复制标记、不移动依赖、不增加 lint suppression。以同一默认 CLI 命令验证红绿，再运行 workspace/all-targets 检查。现有 core 门禁已有默认 CLI build，保持其与 lib tests 分开执行。
