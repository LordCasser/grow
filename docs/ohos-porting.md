# Grow 适配 OpenHarmony（OHOS）arm64 — 深入分析（修订版）

> 状态：分析 + 实施规格（含容器实测记录）。日期：2026-08-02。修订说明见文末。
> 参考：[Harmonybrew 文档](https://atomgit.com/Harmonybrew/docs)、[DockerHarmony](https://github.com/hqzing/dockerharmony)、[ohos-ripgrep](https://gitcode.com/OpenHarmonyPCDeveloper/ohos-ripgrep)、本地 [atomcode](https://github.com/hqzing/dockerharmony)、[Rust Platform Support](https://doc.rust-lang.org/rustc/platform-support.html)。

## 0. 结论：按阻塞优先级排序

Grow 适配 `aarch64-unknown-linux-ohos` **理论可行**（Rust 官方已将其列为 Tier 2 with host tools），但**不能只增加 Cargo target 和 release matrix**。真正的阻塞点依次是：

1. **持久化 Shell 依赖 Bash/Zsh** —— 基础 OpenHarmony 只有 mksh（`/bin/sh`），grow 的终端会话、shell 状态快照、bash 工具全部以 bash/zsh 为前提。这是**首要源码阻塞**。
2. **工具链与签名必须进入构建链** —— OHOS SDK clang/sysroot、ohos 宿主 Rust（≥1.93）、链接器签名（`lld --code-sign`）或二进制签名工具，缺一不可。
3. **内嵌 `rg` 必须在嵌入前独立签名** —— 运行时解包到 `~/.grow/vendor` 的 rg 是独立 ELF，只签 grow 不够。
4. **HMDFS、PTY、SQLite、沙箱只能在真机确认** —— DockerHarmony/ci-runner 能验证编译与基础运行，不能替代 HarmonyOS PC 真机验收（对应 Harmonybrew 的 Tier 1 / Tier 2 分层）。

## 1. 已核实的关键事实

### 1.1 Rust 目标与工具链（实测）

| 事实 | 证据 |
|---|---|
| `aarch64-unknown-linux-ohos` 是 **Tier 2 with host tools**（官方文档有独立页面 `platform-support/openharmony.html`） | [Rust Platform Support](https://doc.rust-lang.org/rustc/platform-support.html) |
| rustc 对 `aarch64-unknown-linux-ohos` 报告 `target_os="linux"`、`target_env="ohos"`、`target_family="unix"` → 所有 `cfg(target_os="linux")` 分支**编译期原样生效** | 本机 `rustc --print cfg` 实测 |
| OHOS 宿主工具链（`rust-<ver>-aarch64-unknown-linux-ohos.tar.xz`）**从 1.93.0 起**发布（1.92.0 = 404） | `curl -I` 实测；Harmonybrew rust formula 用 1.97.1 |
| rustup 在 OHOS 上把宿主误判为 `aarch64-unknown-linux-musl`，其 rustc 依赖 `libgcc_s.so.1` + `_Unwind_*`，OHOS musl 没有 → **不能用 musl 宿主工具链** | 容器实测（加载失败） |
| 官方 ohos dist 的 `cargo` 动态链接 `libssl.so`/`libcrypto.so`/`libz.so`（系统只有改名版 `.z.so` 且符号不全）→ 必须按 rust formula 的 rpath 方案（openssl@3 + zlib-ng-compat） | 容器实测（LD_LIBRARY_PATH 直链改名库仍因 `SSL_get0_group_name` 缺失失败） |
| OHOS SDK（ci-runner 内 `/opt/ohos-sdk/ohos/native`）提供 `aarch64-unknown-linux-ohos-clang` 包装（`-target aarch64-linux-ohos --sysroot=... -D__MUSL__`）、`sysroot/usr`、`build/cmake/ohos.toolchain.cmake`、`build-tools/cmake/bin/cmake` | 容器实测 |
| 容器 SDK 的 `ld.lld` 是裸 lld（无 `--code-sign`）；wrapper 只在 Harmonybrew `ohos-sdk` formula 里 | 容器实测 + formula 源码 |
| rustc/clang 侧目标名：Rust 用 `target_env="ohos"`；Clang 侧用 `--target=aarch64-linux-ohos` | 实测 |

### 1.2 平台环境（Harmonybrew docs / DockerHarmony）

- 用户态 = musl libc + toybox + mksh；**无 glibc、无 systemd、无 X11/Wayland、无 ncurses**；`uname` 返回 Linux。
- HarmonyOS PC：HiShell = zsh（`/usr/bin/zsh`），`/bin/sh` 是 mksh，**无 `/bin/bash`**；家目录 `/storage/Users/currentUser`（HMDFS，系统目录只读）。
- 代码签名：鸿蒙 PC 对 ELF 验签；链接器签名（`lld --code-sign`）或二进制签名工具；容器/开发板（Tier 1）不强制。
- ci-runner 容器**自带 zsh 5.9**（`/opt/zsh-5.9-ohos-arm64`），但 DockerHarmony 基础 rootfs 只有 mksh —— 容器验证与真机有环境差（时区、签名、HMDFS 同理）。

### 1.3 参考项目结论

- **ohos-ripgrep / lycium++**：`cargo build --release --locked --target "${OHOS_RUST_TARGET}"`（= `aarch64-unknown-linux-ohos`）可产出可运行二进制；C 依赖（pcre2）走 lycium 预编译；产物 hnp/tar.gz，**设备侧需自行签名**。
- **atomcode**：完整参照系 —— release 矩阵含 ohos 资产、updater 用编译期 `cfg(target_env="ohos")` 判定（避免运行时 `OS=="linux"` 误判）、Harmonybrew formula 用 `--features distro-pm` 关闭自更新、STATIC-musl 二进制在鸿蒙上不可运行（必须 `*-linux-ohos` 目标产物）。
- **Harmonybrew**：OHOS 视为"特殊 Linux 发行版"，业务全走 Linux 逻辑；`rust`/`ohos-sdk` 为特例 formula（官方分发 + lld wrapper）；流水线统一二进制签名；**只接受原生编译（不接受交叉编译）**。

## 2. 平台识别（架构边界 #1）

OHOS 在 Rust 中同时满足：`target_arch="aarch64"`、`target_os="linux"`、`target_env="ohos"`。

**规则：所有 OHOS 判断必须放在通用 Linux 判断之前，且用 `target_env` 而非 `target_os`。**

1. `crates/codegen/update/src/auto_update.rs` `detect_platform()`（:674 附近）：在现有分支**之前**增加
   `cfg!(all(target_os = "linux", target_env = "ohos", target_arch = "aarch64"))` → `Ok("ohos-aarch64")`。
   配套：同文件 `detect_platform` 相关测试的 cfg 门增加 `all(target_os = "linux", target_env = "ohos", target_arch = "aarch64")`。
2. **命名统一为 `ohos-aarch64`**（与 grow 现有体系 `macos-aarch64` / `linux-aarch64` / `windows-aarch64` 一致）：
   - release 资产：`grow-{version}-ohos-aarch64.tar.gz`
   - release.yml `asset_platform: ohos-aarch64`；updater 返回 `"ohos-aarch64"`
   - 不混用 `ohos-arm64`（atomcode 用 arm64 是其自身命名体系，grow 不沿用）
3. 所有新增 `cfg` 判断必须**先于**任何 `cfg!(target_os = "linux")` 分支，避免被普通 Linux 分支吞掉（atomcode 同款教训）。

## 3. 源码适配边界

### 3.1 Shell 后端 —— 首要源码阻塞

**现状（全部已核实）：**

- `crates/codegen/config/src/shell.rs:367`：`UnixShellKind` 只有 `Bash` / `Zsh`；注释明确 "Fish / dash / ksh users fall through to bash"；`detect_unix_shell_kind()` 对未识别 `$SHELL` 默认 Bash。
- `crates/codegen/shell/src/terminal/mod.rs:27`：`default_shell_path()` 固定请求 Bash。
- `crates/codegen/shell/src/terminal/pty_session.rs:266`：PTY 会话用该路径 `spawn_command`。
- `crates/codegen/tools/src/computer/local/static_shell.rs:56`：rc 快照脚本 —— bash 用 `builtin alias -p` + `builtin declare -f`，zsh 用 `builtin typeset -f`；rc 文件写死 `.bashrc`/`.zshrc`。
- `crates/codegen/tools/src/computer/local/shell_state.rs:73`：bash 状态 dump 脚本（`builtin shopt -p`、`builtin declare -f`），base64 重放。
- `crates/codegen/tools/src/computer/local/embedded_search_tools.rs`：多处 `Command::new(&bash)`。
- bash 工具（`grow_build/bash`）经 Terminal runner → bash。

**影响**：基础 OHOS（容器/开发板）无 bash 无 zsh → 终端会话无法启动、agent 的 bash 工具不可用、shell 状态快照失败（现已有"空快照降级"，但会话本身起不来是硬伤）。

**建议（三步走）：**

1. **近期（Harmonybrew / HarmonyOS PC 首版）**：声明 `zsh` 为运行依赖（HiShell 本身就是 zsh，`/usr/bin/zsh` 存在，grow 现有 Zsh backend 直接可用）。Harmonybrew formula `depends_on "zsh"`；ci-runner 已预置 zsh。这是"可用"，不是完整 standalone。
2. **中期（容器/开发板）**：增加真正的 **Posix（mksh/dash 兼容）backend**：
   - `UnixShellKind::Posix`，执行入口 `/bin/sh`；
   - wrapper 脚本只使用 POSIX 子集（`alias -p` 在 mksh 存在但 `declare -f`/`shopt -p` 是 bash 专有 → 快照能力明确降级：环境变量 + cwd + 有限 alias，function 快照在 Posix backend 返回"不支持"）；
   - `detect_unix_shell_kind()` 增加 `sh`/`mksh`/`dash`/`ksh` 识别；
   - Bash/Zsh backend 保持现有能力不变。
3. **不要到处加 `cfg(target_env = "ohos")`** —— 那是把平台差异散落各处的反模式；平台差异应收敛在 `config::shell` 的解析层和 wrapper 生成层。

### 3.2 Clipboard

OHOS 通常无 X11/Wayland clipboard。Linux backend（arboard/wl-copy/xclip）在 OHOS 上会**运行时随机失败**。建议在 `client-support/src/clipboard.rs` 的 linux 路径内，对 `cfg!(target_env = "ohos")` 显式返回结构化"不支持"错误（文案指引用户传文件路径），而不是让 arboard 探测失败后冒出晦涩错误。所有调用点需确认对"不支持"的降级路径（提示而非崩溃）。

### 3.3 Sandbox

`cli` 默认 feature 含 `sandbox-enforce`；sandbox 实现依赖 **bwrap（bubblewrap）可执行文件 + seccomp**（`sandbox/src/hook_write_deny.rs` 的 `build_bwrap_plan`、`child_net.rs` 的 prctl/seccomp BPF）。OHOS 无 bwrap；seccomp/landlock 能力取决于内核与设备策略。
**结论：编译成功 ≠ 可用。** OHOS 上应做**运行时 capability probe**：bwrap 是否存在、seccomp 是否生效（`PR_SET_NO_NEW_PRIVS` + filter 安装是否成功）、landlock ABI 是否可用；任一缺失 → 降级为不隔离并显式告警（保持 `sandbox-enforce` feature 关闭状态或 probe 后关闭 enforcement）。需在真机验证。

### 3.4 PTY

portable-pty forkpty/openpty 链路需真机验证：`/dev/ptmx` 存在性、窗口尺寸（TIOCSWINSZ）、信号与进程组（SIGWINCH/SIGCHLD）、`docker exec -it` 仅能验证基础 PTY 语义，不能代表 HiShell 终端行为。**容器通过 ≠ PC 可用。**

### 3.5 时区

ci-runner 最小 rootfs 无 `/usr/share/zoneinfo`、`/etc/localtime`（实测 `date` 仅能输出 UTC）。真机时区/`TZ` 行为需验证（grow 的 `time`/`chrono` 本地时间路径）。

### 3.6 登录与浏览器跳转

OAuth/浏览器打开（xdg-open 类）与 callback 在鸿蒙 PC 的可用性需真机验证；无桌面协议时可能需降级为"打印 URL + 手动粘贴 code"。

### 3.7 jemalloc

`crates/codegen/cli/Cargo.toml`：`default = ["jemalloc", "sandbox-enforce"]` —— jemalloc **默认开启**。
**建议第一版 OHOS 关闭 jemalloc**（系统 allocator）：CI 用 `--no-default-features --features release-dist,sandbox-enforce`（或 main.rs 中 `cfg(not(target_env = "ohos"))` 门控）。**不要假设 OHOS 页大小固定 4KiB** —— 未在真机确认前，不设 `AARCH64_UNKNOWN_LINUX_OHOS_JEMALLOC_SYS_WITH_LG_PAGE`；等 jemalloc 在 SDK 与真机验证后再启用。

### 3.8 sqlite-journal 与 HMDFS

`crates/codegen/sqlite-journal/src/lib.rs:66`：`is_network_fs(dir)` 决定 `Wal` / `Truncate`；注释明确 "Any detection failure returns false (treat as local) so unclassifiable filesystems keep the historical WAL behavior" —— **未知文件系统（HMDFS 极可能如此）→ WAL → mmap `-shm` → 该 crate 文档自述的 SIGBUS 风险场景**。已有逃生开关：`GROW_SQLITE_JOURNAL_MODE=wal|truncate`（:38）。
**真机门槛清单**：
- SQLite WAL、文件锁、mmap 在 HMDFS 上的行为；
- atomic rename / symlink swap（updater 替换、worktree）；
- updater 解包、替换、签名后执行；
- git worktree / 大量小文件性能；
- `~/.grow/vendor/rg` 是否允许执行（签名 + HMDFS exec 权限）。
若 HMDFS 不满足：`GROW_HOME` 指向本地非 HMDFS 数据区，或 OHOS 上强制 `GROW_SQLITE_JOURNAL_MODE=truncate`。

## 4. 构建与发布

### 4.1 CI 结构（推荐）

不要把整个 GitHub Actions job 直接放进 OHOS 容器（`actions/checkout` 等 Node action 需要 OHOS Node，见 DockerHarmony README 的 node 挂载方案）。更稳妥：

```text
GitHub Actions / ubuntu-24.04-arm
    └── docker run --rm -v workspace -v cargo缓存 Harmonybrew ci-runner
          ├── cargo build --profile release-dist -p cli --bin grow   # OHOS 原生
          ├── strip + 签名（见 4.3）
          ├── OHOS 容器冒烟（--version、PTY 基础、config 写入、updater 解包）
          └── 打包 grow-{version}-ohos-aarch64.tar.gz
```

- release.yml：新增 `asset_platform: ohos-aarch64` / `target: aarch64-unknown-linux-ohos` / `runner: ubuntu-24.04-arm` / `smoke: false`；**3 处** 9→10 资产清单（`Verify staged binaries`、`Verify and publish` 的 `required` 数组 + 资产校验段）。
- `.cargo/config.toml` 只加 `[target.aarch64-unknown-linux-ohos]` 的 **rustflags**（`force-unwind-tables` + `-Wl,-z,relro,-z,now,-z,noexecstack`，与 linux 段一致）；**不硬编码 SDK 路径** —— linker/sysroot/SDK 根由 CI 环境注入：`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER`、`CC_/CXX_`、`OHOS_NDK_HOME`、PATH 追加 SDK `llvm/bin` 与 `build-tools/cmake/bin`。
- rust 版本：OHOS 宿主工具链 ≥1.93（跟随 Harmonybrew rust formula，当前 1.97.1）；CI 内 `RUSTUP_TOOLCHAIN=system` 覆盖 `rust-toolchain.toml` 的 1.92.0 pin。
- 已实测的容器网络坑（重要）：
  1. cargo（libcurl+brew openssl）连 crates.io 索引间歇性 TLS `unexpected eof`；
  2. **USTC 镜像的下载端点（`mirrors.ustc.edu.cn/crates.io/api/v1/.../download`）从本网络 5/5 全失败**（curl CLI 同样失败，非 cargo 问题）；
  3. **rsproxy（`sparse+https://rsproxy.cn/index/`）下载 5/5 成功** → 当前推荐镜像；
  4. `CARGO_HTTP_MULTIPLEXING=false` + `CARGO_NET_RETRY=8` + 构建重试循环兜底。
  真实流水线（arm 服务器）网络可能不同，但镜像配置与重试机制建议保留。

### 4.2 `rg` 双渠道（各自独立处理）

| 渠道 | `rg` 处理 |
|---|---|
| 官方 standalone tar.gz | **从源码构建 → strip → 签名 → 嵌入** grow（tools/build.rs 的 `GROW_TOOLS_BUNDLE_RG_PATH` + `GROW_TOOLS_BUNDLE_RG_SKIP_EXEC_CHECK=1`；rg 15.0.0 可在同一容器内 `cargo build` 产出） |
| Harmonybrew Formula | `depends_on "ripgrep"`，运行时 PATH 查找，**不嵌入**（brew 的 ripgrep 由流水线签名；内嵌 rg 解包后无签名，鸿蒙 PC 无法执行） |

> 修正：上一版草稿把"Formula 不应嵌入"错误扩大成所有渠道都不能嵌入。standalone 渠道只要**签名顺序正确**（先签 rg 再嵌入）即可。

### 4.3 签名与压缩（严格顺序）

```text
build rg → llvm-strip rg → binary-sign rg → embed rg
→ build grow → llvm-strip grow → binary-sign grow → tar.gz
```

- 不使用 UPX 或其他可执行文件压缩；压缩只发生在传输层 `tar.gz`。
- **签名后绝不再次 strip / 修改 / 压缩 ELF**；外层 `tar.gz` 在签名后生成，不影响签名。
- 只签 grow 不够：运行时解出的 `rg` 是独立 ELF，必须单独签。
- 自发行（非 formula）若用链接器签名：需要 SDK lld 的 `--code-sign` wrapper（Harmonybrew `ohos-sdk` formula 的方式；当前容器 SDK 是裸 lld，需包装或构建后用二进制签名工具统一签）。

### 4.4 Harmonybrew formula（草稿）

```ruby
class Grow < Formula
  depends_on "rust" => :build
  depends_on "zsh"          # 持久 shell 后端（HiShell 本身是 zsh；Posix backend 落地后可改 optional）
  depends_on "ripgrep"      # PATH rg，不嵌入
  def install
    # --features distro-pm：编译期关闭自更新（/upgrade 引导 brew upgrade）
    system "cargo", "install", *std_cargo_args(path: "crates/cli"), "--bin", "grow",
           "--no-default-features", "--features", "release-dist,sandbox-enforce,distro-pm"
  end
end
```
（具体 feature 名与关闭自更新的机制由实现阶段定稿；`distro-pm` 参照 atomcode。）

## 5. 验证分层

| 层 | 能验证 | 不能验证 |
|---|---|---|
| DockerHarmony / ci-runner（Tier 1 等价） | 完整依赖图编译；ELF interpreter / NEEDED / 签名流程；`grow --version`、`rg --version`；Shell 工具（zsh 存在）、PTY 基础启动、配置写入、updater 解包；workflow 结构与 matrix（`act` 只能验证这层） | 真机行为 |
| HarmonyOS PC 真机（Tier 2） | HiShell 中 TUI/键盘/信号；HMDFS 文件语义；下载后签名执行；Clipboard 降级提示；Sandbox capability；登录与浏览器跳转；时区 | — |

容器成功 ≠ 鸿蒙 PC 可发布（Harmonybrew 自身就是 Tier 1 / Tier 2 分层的）。

## 6. 实验记录（2026-08-02，OrbStack arm64 + ci-runner）

| # | 实验 | 结果 |
|---|---|---|
| 1 | 宿主：`rustup target add aarch64-unknown-linux-ohos` | ✅ |
| 2 | 宿主：`cargo check --target ...-ohos`（无 SDK） | ❌ ring 缺 sysroot 头 —— 证明必须用 OHOS SDK clang |
| 3 | 容器：musl 宿主 rustc | ❌ `libgcc_s`/`_Unwind_*` 缺失 |
| 4 | 容器：官方 dist 1.92.0 ohos 宿主 | ❌ 不存在（404）；1.93+ 有 |
| 5 | 容器：rustup 1.97.1 ohos 宿主（`--force-non-host`） | ✅ rustc 可跑；cargo 缺 libssl/libcrypto/libz |
| 6 | 容器：LD_LIBRARY_PATH 指 OHOS 改名库 | ❌ 符号版本不全 |
| 7 | 容器：`brew install rust`（1.97.1，599MB，rpath 已修） | ✅ cargo 可运行 |
| 8 | 容器：cargo fetch | ⚠️ crates.io 索引 TLS 间歇失败 → USTC 镜像收敛；随后 USTC 下载端点全挂 → 切 rsproxy（下载 5/5 成功） |
| 9 | 容器：ohos-ripgrep 预编译 rg 15.1.0 直接执行 | ✅ 原生运行（ELF interpreter `/lib/ld-musl-aarch64.so.1`，DYNAMIC musl，NEON）—— 参考资料产物可用 |
| 10 | 容器：`cargo build --release -p cli --bin grow`（默认 feature） | ❌ nix 0.26.4 编译失败（libc 缺 `O_FSYNC`/`__fsword_t`/`XFS_SUPER_MAGIC`/`ST_RELATIME`）；sqlite-vec C 代码用 glibc 专有 `u_int*_t`；jemalloc configure 不认 ohos 三元组 |
| 11 | 容器：打补丁后重构建（**libc 补丁 + nix 0.26.4 补丁 + 关 jemalloc + CFLAGS 宏**） | ✅ **BUILD OK（9m20s）**，169MB release 二进制，interpreter `/lib/ld-musl-aarch64.so.1`，NEEDED：`libz.so` + `libtime_service_ndk.so`（clang wrapper 默认引入）+ `libc.so` |
| 12 | ci-runner 冒烟（有 zsh）：version / inspect / doctor / sessions / vendor-rg / zsh backend | ✅ 全部通过（doctor 优雅降级：container 剪贴板、颜色 unavailable、newline fallback） |
| 13 | DockerHarmony 最小容器（mksh-only）：同套冒烟 | ✅ 全部通过（非交互路径不依赖 bash/zsh） |
| 14 | PTY/TUI：python pty fork 启动 grow（zsh 与 mksh-only 两种 PATH） | ✅ TUI 正常启动渲染、键盘输入处理正常（portable-pty/forkpty 在 OHOS 容器可用） |
| 15 | Rust 官方平台页 | ✅ `aarch64-unknown-linux-ohos` = Tier 2 with host tools |
| 16 | **复核构建**（用户提交后：sqlite-vec vendor 0.1.10-alpha.4、nono musl 修复、v1.1.0）：**去掉 libc 补丁 + CFLAGS**，仅 nix 0.26.4 补丁 + 关 jemalloc | ✅ BUILD OK（10m37s），169MB；冒烟（version/sessions/doctor）✅ —— 最小补丁集收敛为 2 项 |
| 17 | **aws-lc-rs 移除**：rmcp 去 `reqwest` feature（reqwest 0.13 → rustls-no-provider）；vendored nono 删 sigstore 栈 | ✅ `cargo tree -i aws-lc-rs` 为空；rustls 仅 ring provider；host check（sandbox+mcp）通过；sigstore 链 ~10 crate 出图 |

## 7. 风险清单

1. **Shell**：Posix backend 是独立功能开发（新 backend + 降级语义 + 测试），不能当"适配补丁"塞进 OHOS 移植。容器实测确认：**TUI 启动/非交互命令不依赖 bash/zsh**，只有终端会话与 agent bash 工具依赖 —— 首版可声明 zsh 依赖（HiShell 自带）。
2. **nix 多版本**：cli 图中 nix 0.26.4（pprof 链）需 "ohos≈musl" 补丁、nix 0.28.0（portable-pty）原生支持、nix 0.30.1（grow 自家）原生支持。**libc 0.2.186 的 ohos 支持不完整**（缺 4 个常量）。建议向 libc/nix 上游贡献补丁，或仓库内 vendor（先例：`third_party/nono`）。
3. **jemalloc**：configure 不认 ohos 三元组 —— **第一版必须关**（`--no-default-features --features sandbox-enforce`），与文档建议一致；后续需 patch jemalloc 的 config.sub 或在真机验证。
4. **动态链接**：SDK clang wrapper 链接的二进制带 `libtime_service_ndk.so`（真机系统自带，最小 rootfs 无）；libz-sys 动态链 `libz.so`（真机是改名 `.z.so`）—— **发布前需确认动态链接策略**（静态链 zlib 或依赖 Harmonybrew 的 zlib 提供 soname）。
5. **真机不可替代项**：HMDFS/SQLite、PTY/终端（容器 PTY 已过，HiShell 行为未验）、沙箱、签名执行、登录跳转。
6. **签名链**：standalone 渠道的签名工具与密钥管理需单独设计。
7. **性能**：arm runner 容器内全量 release 构建约 10-30 分钟（本测试 9m20s 为增量）。
8. **依赖版本**：OHOS 工具链 ≥1.93 vs 仓库 pin 1.92.0 —— CI 显式处理。
9. **并发工作区**：本仓库存在并行改动（Cargo.toml、third_party/nono 等），移植工作需在其后 rebase/合并。
10. **镜像/网络**：USTC 下载端点在本网络全挂，rsproxy 稳定 —— 流水线需配置可用的镜像源 + 重试。

## 7b. 容器测试结论（2026-08-02 实测）

- **编译**：依赖图可编译 ✅（ring、libgit2-sys、libsqlite3-sys、zstd-sys、mimalloc 等 C 依赖经 OHOS clang 编译通过；rg 嵌入 + 构建期 exec-check 通过）。**aws-lc-sys 已从依赖图移除**（2026-08-02）：rmcp 的 `reqwest` feature 改为 no-provider（mcp crate 自有 reqwest 已是 `rustls-no-provider`，运行期由 cli 的 ring provider 安装覆盖），vendored nono 删除 sigstore 验证/签名栈（`trust/{bundle,signing}.rs` + sigstore 依赖，grow 只使用 nono 的 sandbox 面）—— TLS 栈统一为 ring，C 依赖不再需要 cmake/SDK env。
- **运行**：OHOS 原生二进制在 ci-runner 与 DockerHarmony 两种 OHOS 用户态均可运行 ✅。
- **冒烟**：`--version`、`inspect --json`、`doctor`（优雅降级）、`sessions list`（SQLite 初始化）、vendor rg 解包执行、zsh backend、PTY/TUI 启动 ✅。
- **必须的改动（测试中实证，2026-08-02 复核后最小集）**：
  1. **nix 0.26.4 补丁**（唯一源码层补丁）：`target_env = "musl"` → `any(target_env = "musl", target_env = "ohos")`（0.28/0.30 原生支持 ohos，勿动）；
  2. **关 jemalloc**（`--no-default-features --features sandbox-enforce`）；
  3. 运行时需 `libz.so`（libz-sys 动态链）—— 发布时静态链或走包管理器依赖。
- **已随用户提交消化的补丁**（复核构建不再需要）：
  - `libc` 补丁（4 个缺失常量）：nix 补丁把 glibc 分支门掉后 libc 常量不再被引用 —— **无需 vendor libc**；
  - sqlite-vec CFLAGS 宏：用户 vendor 的 `third_party/sqlite-vec`（0.1.10-alpha.4，musl 兼容 C 源码）已修复 —— **无需 CFLAGS**；
  - nono seccomp ioctls musl 兼容（`c1cf337`）：沙箱通知路径编译期受益。
- 测试方法备注：补丁打在**容器内 workspace 副本**（`/storage/Users/currentUser/grow-test`）与 registry 源码（cargo ≥1.83 不再校验 .cargo-checksum.json，可临时改源码验证）；仓库本体未改动。

## 9. 路线图：下一步（第一阶段完成后）

> 阶段定义：**第一阶段 = 仓库本体在 docker 编译通过**（已完成并验证）。
> **进展（2026-08-02）**：Step 0/1 已完成（nix vendor、aws-lc 移除、build 脚本、distro-pm 已合入 main）；**v1.1.1 已打 tag 并推送**（release workflow 已在 GitHub 运行 9 平台构建）；Step 3 的 formula 已用**真实 v1.1.1 tag tarball 端到端验证**（`brew install -s --include-test grow` ≈12min、`brew test` 通过、style/audit 零问题），草稿存于 `packaging/harmonybrew/grow.rb`，待提交 Harmonybrew/core PR。以下按依赖排序。

### Step 0 — 提交第一阶段改动（立即）
`third_party/nix-ohos/`（vendor）+ `Cargo.toml`（patch 条目）+ `Cargo.lock`（nix 0.26.4 → path）+ 本文档更新。没有这一步，后续全部为空谈。

### Step 1 — 代码闭环（让产物可分发，参照 atomcode）
| # | 改动 | 参照 |
|---|---|---|
| 1a | `update` 的 `detect_platform()` 增加 `ohos-aarch64` 分支（`cfg!(all(target_os="linux", target_env="ohos", target_arch="aarch64"))`，置于 linux 分支之前）+ 测试 cfg 门 | atomcode updater 的 `cfg(target_env="ohos")` 编译期判定 |
| 1b | 新增 `distro-pm` cargo feature：编译期关闭自更新（`/upgrade` 引导 `brew upgrade grow`）—— **Harmonybrew formula 的前置条件** | atomcode `--features distro-pm` |
| 1c | tools crate 的 rg 内嵌策略落定：standalone 渠道内嵌（构建时注入 + 先签后嵌）；formula 渠道不内嵌走 PATH | 本文 §4.2 双渠道表 |

### Step 2 — 发布链（standalone 渠道，参照 dockerharmony + atomcode）
- release.yml 新增 `asset_platform: ohos-aarch64`（`target: aarch64-unknown-linux-ohos`，`runner: ubuntu-24.04-arm`，`builder: ohos`：`docker run` ci-runner 容器内构建 —— dockerharmony README 的 GitHub workflow 模式；不要整 job 进容器，Node actions 需要 OHOS Node）；
- rg 产物：容器内 `cargo build` rg 15.0.0（或 ohos-ripgrep 15.1.0 产物）+ `GROW_TOOLS_BUNDLE_RG_PATH` + `GROW_TOOLS_BUNDLE_RG_SKIP_EXEC_CHECK=1`；
- 2 处 required 资产清单 9→10：`grow-${version}-ohos-aarch64.tar.gz`；
- **签名决策（需用户拍板）**：链接器签名（复用 Harmonybrew `ohos-sdk` formula 的 `ld.lld --code-sign` wrapper）或流水线二进制签名工具；密钥管理单独设计。

### Step 3 — Harmonybrew formula（主分发路径，per contribute-formula.md）
- 上游 homebrew-core **无 grow**（已核实 404）→ 手写 formula（B 路径，无命名冲突）；
- `depends_on "rust" => :build`、`depends_on "ripgrep"`（PATH rg，已签名）、`depends_on "zsh"`（持久 shell 后端，HiShell 自带；Posix backend 落地后可改 optional）；
- `cargo install`（或 build+拷贝），flags 与第一阶段验证一致：`--no-default-features --features sandbox-enforce`（+ Step 1b 的 `distro-pm`）；
- 构建必须在 ci-runner 容器内复现（环境对齐是硬性要求）；PR 遵循"一个 PR → 一个 commit → 一个 formula"；
- 备选策略：先录入上游 homebrew-core 再从上游搬运（贡献指南推荐路径；grow 可移植性满足，属可选加分项）。

### Step 4 — 真机验证（Tier 2 门槛，需鸿蒙 PC/开发板）
清单：HiShell 中 TUI/键盘/信号；HMDFS 文件语义（SQLite WAL/锁/mmap、原子 rename、`~/.grow/vendor` exec）；下载后签名执行；剪贴板降级提示；沙箱 capability；登录与浏览器跳转；时区。产出 device checklist + 冒烟脚本（复用容器冒烟脚本改造）。

### Step 5（可选）— lycium++ / HNP 打包（开发板渠道，per ohos-ripgrep）
HPKBUILD（`buildtools=cargo`、`source`=grow 仓库 tag tarball、`cargo build -p cli --bin grow --release --locked`、`package()` 装 bin + `hnp.json`、`hnpcli` 打包）；设备侧需自行签名。与 formula 渠道互补，非必须。

### 依赖与并行
```text
Step 0 ──▶ Step 1 ──▶ Step 2 ═╗（可并行）
                  └────▶ Step 3 ╝
Step 4 依赖 Step 2/3 的产物；Step 5 独立
```

### 需要用户决策
1. 主分发渠道：Harmonybrew formula（推荐，流水线签名）/ standalone release（需自管签名）/ 两者并行；
2. standalone 签名方案与密钥管理；
3. 是否先上游化 homebrew-core（可选加分项）；
4. 真机资源（鸿蒙 PC / 开发板）何时可用 —— 决定 Step 4 排期。

## 8. 对上一版草稿的修正记录

- [ ] 低估 Bash/Zsh 依赖 → 升级为阻塞 #1，给出三步走方案（§3.1）。
- [ ] "Formula 不嵌入 rg" 扩大化 → 修正为双渠道各自处理（§4.2）。
- [ ] `ohos-arm64` / `ohos-aarch64` 混用 → 统一 `ohos-aarch64`（§2）。
- [ ] 假设 4KiB 页大小 → 删除假设，第一版关 jemalloc（§3.7）。
- [ ] sandbox / HMDFS 结论过乐观 → 改为"编译≠可用，真机验证 + 降级路径"（§3.3/§3.8）。
- [ ] 容器验证与真机兼容性混为一层 → 分层验证表（§5）。
- [ ] `.cargo/config.toml` 硬编码 SDK 路径 → 只放 rustflags，工具链由 CI env 注入（§4.1）。
