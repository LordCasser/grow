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

## 修复后的最终源与验证

最终发布源为 `599873595c943bcce2ec5bcc5fef891820e372c9`，重新绑定的 annotated `v2.5.5` 通过本地 validate-release.sh 后以旧 tag object 为 lease 推送。正式第二轮为 https://github.com/LordCasser/grow/actions/runs/41168064853 。

Rust 1.93.1 的默认 CLI 检查（52.81s）与 workspace all-targets（2m08s）再次通过，诊断与 build-script stderr 无 warning。修改后的 C native suite 在 Linux Core 中分别通过 -O2 -Wall -Wextra -Werror 和 ASan/UBSan/LeakSanitizer 验证。独立只读对抗复核确认错误输出、成功时 vector/cleanup 转移、无诊断失败和真实 SQLite scalar/连接恢复边界，无阻塞项。

最终 SHA 的 Core 完整通过：Shell 4454 passed、0 failed、9 ignored；Settings 235 passed；三个真实 session recovery exact tests 与全部选定 PTY exact tests 通过，包括 plan_two_revisions_complete。三平台 native coordination/export、Windows session storage 和 OpenSpec 门禁全部通过。不累加嵌套 test result 伪造总体数量。

已完成的九个平台完整日志无 Rust/C/MSVC/linker warning/error，RISC-V 原两条 maybe-uninitialized 已消失。适用的二进制版本 smoke 输出/核对 grow 2.5.5 (5998735)，两个 macOS 的 build/staged codesign verify 均通过；musl/RISC-V 的 shared smoke=false 保持既有配置，不声称执行过运行 smoke。已完成平台 attestation 与 artifact 上传成功。Windows smoke HTTP fixture 有客户端断开后的 Python ConnectionAbortedError traceback，场景与 job 均正常成功；不把它当作编译诊断或悄悄放宽断言。OHOS 与最终公开资产结果待完成后补记。

期间本机代理产生 TLS EOF，GitHub connector 也无法连通；仅对查询命令移除代理环境变量后直连恢复，不修改全局代理或用户网络设置。

本轮本地 cargo clean 再删除 1019 文件、4.4 GiB；两轮累计约 31.6 GiB，保留共享依赖缓存。

## 发布完成

2026-10-31 13:29:08（Asia/Shanghai）公开 [v2.5.5](https://github.com/LordCasser/grow/releases/tag/v2.5.5)；第二轮正式流水线 20/20 jobs success，tag 与全部构建绑定源 `599873595c943bcce2ec5bcc5fef891820e372c9`。GitHub Release 的 isDraft=false、isPrerelease=false，releases/latest 返回 v2.5.5。

OHOS 最后完成：完整日志无 Rust/C/linker 编译 warning，容器内输出 grow 2.5.5 (5998735)，session/doctor smoke 通过。Python/C 自签实现字节一致、签名增长与 section/归档大小门槛通过，attestation 和 artifact 上传成功。OHOS 日志中有 Cargo 网络重试及 Harmonybrew/rustup 的环境提示（安装器明确标明 -y 下可忽略后继续），最终构建成功；这些不作为产品编译诊断，也不隐去其耗时和环境边界。十个平台完整构建日志均完成编译 warning 复核。

公开资产恰好为十个平台归档与 SHA256SUMS 共 11 项，校验和列表精确覆盖十个归档；publisher 已逐项校验来源证明与远端字节。独立下载 macOS ARM64 归档与 SHA256SUMS：SHA-256 为 `490e4b81e9d823795a7d689c639194c38f846584ac1e82c7d64e77a365d43f4f`，与校验和及构建 attestation 一致；归档只含 grow。以独立 GROW_HOME 实际执行下载二进制，输出 `grow 2.5.5 (5998735)`。

首次本地 smoke 未隔离用户缓存，因此按既有 channel_label 契约附加 [alpha]：compiled version 比本机缓存 stable pointer 更新；不是 GitHub prerelease 或源身份错误。核对 update::channel_label 后在独立临时 GROW_HOME 重跑通过，没有修改用户缓存或配置。临时下载与二进制在检查后自动删除，没有执行全局安装。

发布后的收尾只更新验证、归档和 backlog 说明，运行时代码与发布 tag 保持一致。

## 最终归档校验

发布资产与源身份核对后勾选所有 tasks，并归档 release-2-5-5。归档后全量 strict 16/16、archive strict 762/762 通过。归档时 Why 字数提示已补足具体协调原因。`git diff v2.5.5 -- crates third_party .github Cargo.toml Cargo.lock` 为空，收尾不改发布代码或构建输入。`git diff --check` 通过；最终 Git 提交仅包含归档、backlog 与验证记录。
