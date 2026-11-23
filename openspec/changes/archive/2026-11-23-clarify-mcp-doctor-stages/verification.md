# CLI verification

在 macOS 上使用仓库锁定的 Rust 1.93.1、默认 CLI features 和隔离的临时 Cargo target 构建。所有探针使用临时 Grow 配置、本地 HTTP 或 stdio 服务，并直接检查子进程退出码。没有访问 Arc 或使用真实凭证。

`cargo test --locked --offline -p cli --test mcp_doctor` 通过。HTTP 回归启动真实 Grow 进程，覆盖下列情况。

- 两页完整目录在文本和 JSON 中均报告客户端已创建、协商协议和 2 个工具，退出 0。
- 握手挂起时显示 1 秒握手超时，省略目录阶段。第二页挂起时保留成功的握手，显示 7 秒初始化预算耗尽，服务端也观察到该页请求。两个单服务命令分别退出 1。
- 混合健康和失败服务时，JSON 报告 1 healthy、2 failing，进程退出 1。

| stdio 场景 | 结果 |
| --- | --- |
| 两页正常目录 | 文本与 JSON 均报告子进程已启动、握手成功、发现 2 个工具，退出 0。 |
| 握手成功但目录为空或 cursor 重复 | 两种输出均保留成功的握手，目录失败带具体原因，退出 1。 |
| 第一页返回工具，第二页始终不回复 | 已安装的 2.6.16 在 9 秒后仍没有报告，由测试端终止。修复后的 CLI 在 7 秒初始化预算用完时标明工具发现超时，两种输出均退出 1。 |
| 一个正常服务和三个失败服务 | JSON 汇总 1 healthy、3 failing，退出 1。 |
| 握手耗时 2 秒，两页各延迟 5 秒，启动超时配置为 3 秒 | 实际请求了两页，11 秒的总预算在工具发现阶段耗尽，退出 1。该预算包含握手和第一页的用时。 |

其余定向验证通过。

- `cargo test --locked --offline -p shell --lib mcp_doctor::tests` 通过 4 项，覆盖阶段名、超时时长和底层错误详情。
- `cargo test --locked --offline -p mcp --test tool_list_pagination` 通过 5 项，覆盖多页、空中间页、后续页错误和 cursor 循环。
- `cargo test --locked --offline -p shell --lib mcp_catalog_lifecycle_tests -- --test-threads=4` 通过 17 项。
- `cargo fmt --all -- --check`、`git diff --check` 通过。
- OpenSpec 归档前严格检查通过 19 项，归档后严格检查通过 18 项，历史归档检查通过 986 项。
