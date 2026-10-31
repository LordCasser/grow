# 验证记录

## 提交范围

按四个闭环拆分既有修改：子 Agent 授权与目录热更新；Plan 反馈、确认、终端旅程及未派发重规划；压缩失败、挂起排空与中性取消标记；默认自动更新与真实 PTY 提示。对应本日 13 个归档随功能提交。既有三项本地提交（等待子任务指导、跨路由 reasoning、上下文交接）同属本次发布源。

新增 backlog 保留 workspace 路径别名、出生 MCP 集合与外部 ACP Plan 边界的后续工作，不将它们混入本次修复。

## 本地检查

- `cargo fmt --all -- --check`：通过，Rust 1.93.1。
- `python3 -m unittest discover -s scripts/tests`：34 项，0 失败，1 项既有平台跳过。
- `openspec validate --all --strict --no-interactive`：17/17 通过。
- `openspec validate --archived --strict --no-interactive`：760/760 通过。
- `git diff --check`：通过。

首次环境 PATH 指向 Homebrew Rust 1.99.1，已中止该次编译并将 rustup 代理置于 PATH 首位。历史 fetch_update 弃用诊断来自该较新编译器；发布固定 Rust 1.93.1，不以更改 API 或放宽 lint 绕过工具链契约。

版本维护只更新 workspace 与 lockfile 中 33 个本地包的 2.5.4→2.5.5，依赖来源和校验值不变。尝试完整离线重生成 lock 时被既有锁定 bisync 0.3.0 的 yank 阻止，未采用重新解析结果；以现有 lock 为准同步本地版本，并用 locked 构建验证。

编译、远端同 SHA 门禁、平台构建及最终发布结果待完成后补记。日志位于 /tmp/grow-release-*.log，不将未执行结果记为通过。

## Warning 清理与默认生产图

macOS 实际诊断确认 jemalloc 无条件请求不支持的 unprefixed 模式。已将该 feature 限定到 Apple/Android/DragonFly 以外目标，沿用依赖原有目标分类，Apple 仍为 prefixed，未开启 zone override。Cargo feature tree 核对 macOS 不含 unprefixed、Linux 含 unprefixed，两者仍启用 profiling/stats。Linux tree 首次 offline 缺 ashpd 缓存，改为正常拉取后通过，不修改锁定依赖。

使用 `PATH=/Users/lordcasser/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin CARGO_BUILD_JOBS=2 cargo check --locked -p cli --bin grow`，2.5.5 默认 CLI 通过，编译与 build-script stderr 没有 warning。原 jemalloc 缺头文件失败已定位为 DevEco SDK diff 对不存在输入返回 0，系统 diff 及固定工具链下重新配置后成功。

对抗性复核发现初始目标条件缺少 Unix 限制，可能令 Windows 新增 allocator 依赖；打标签前修正为 all(unix, not(...))，保持既有 Windows 依赖边界。

## 首轮发布取消与补充修复

最终 workspace all-targets 检查已通过（3m21s），没有编译诊断。第一轮正式流水线 https://github.com/LordCasser/grow/actions/runs/41165859929 绑定 e91cf333bddcccb722f279134c52da18ae34ec8d。Shell 全库 4454 passed、0 failed、9 ignored，OpenSpec、Windows storage、三平台 native coordination/export 均通过，Linux 五个平台和 Windows 两个平台构建通过；Core 到达真实 PTY 选择。

下载已完成 RISC-V 构建的完整日志后发现两条真实 C maybe-uninitialized warning，本地 host 和其他已核对平台没有这些诊断。取消整个 run，确认 completed/cancelled 且 Release 不存在；不能把该轮成功产物当作最终发布。另有 setup-node@v4 的 Node runtime 弃用 annotation，属于 CI 维护，与编译诊断分开记录。

独立修复已归档为 2026-10-31-fix-native-vector-error-initialization：共享 parser 初始化错误输出一行、真实 SQLite 分配失败回归先红后绿、严格 C 与 ASan/UBSan、GCC16 严格编译通过。具体证据和限制见该 change。重新绑定未公开 v2.5.5 annotated tag，下一轮必须重建全部平台和门禁，不复用旧资产。

首轮本地构建后已执行 cargo clean -p shell -p pager -p cli -p tools -p tool-types，删除 6719 文件、27.2 GiB；保留共享依赖缓存。
