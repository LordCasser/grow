# Grow 移植 OpenHarmony（OHOS）arm64 分析

> 状态：分析报告（含容器内实测结论）。日期：2026-08-02。
> 参考：[Harmonybrew 文档](https://atomgit.com/Harmonybrew/docs)、[DockerHarmony](https://github.com/hqzing/dockerharmony)、[ohos-ripgrep](https://gitcode.com/OpenHarmonyPCDeveloper/ohos-ripgrep)、[atomcode](https://github.com/hqzing/dockerharmony)（本地仓库 `/Users/lordcasser/workspace/projects/atomcode`）。

## 0. 结论摘要

- **可行性高，编译期改动极小**。Rust 的 `aarch64-unknown-linux-ohos` 目标在 rustc 中报告 `target_os = "linux"`（`target_env = "ohos"`），因此 grow 中所有 `#[cfg(target_os = "linux")]` 的代码路径在 OHOS 上**原样生效**，无需逐处改写。grow 的核心依赖（reqwest+rustls+webpki-roots、gix、ratatui/crossterm、nix 等）均可在 OHOS 工具链下编译。
- **主要工作量在工程链而非源码**：OHOS 宿主 Rust 工具链（1.93+）、OHOS SDK 交叉/原生编译环境、自更新与发布链路增加 ohos 平台、运行时功能降级（剪贴板/休眠抑制/沙箱）、打包分发（Harmonybrew formula 或 HNP）。
- **三个必须处理的点**：
  1. `crates/codegen/update` 的 `detect_platform()` 在 OHOS 上会 `bail!`（现有分支只匹配 `musl`/`gnu` env），需增加 `ohos-aarch64` 分支；
  2. `.github/workflows/release.yml` 的资产清单（3 处 `required` 列表）需从 9 个平台扩到 10 个，并新增 ohos-arm64 构建条目（arm64 runner + ci-runner 容器）；
  3. 内嵌 ripgrep（`GROW_TOOLS_BUNDLE_RG_PATH`）在 OHOS 设备上解包到 `~/.grow/vendor` 后是**未签名 ELF**，鸿蒙 PC 上无法执行 —— OHOS 构建应改为不内嵌、运行时走 PATH（由 Harmonybrew 的 `ripgrep` formula 提供已签名二进制），并新增 `depends_on "ripgrep"`。
- **分发路径推荐**：Harmonybrew formula（生态主路径，流水线自动签名），辅以官方 release 资产（供容器/开发板/自更新场景）。

## 1. 平台事实（来自参考资料，已尽量实测验证）

### 1.1 OpenHarmony 系统环境（Harmonybrew docs FAQ）

- **内核**：Linux 内核；`uname` 返回 `Linux`（容器内实测确认），大量软件可自动走 Linux 构建分支。
- **用户态**：musl libc（`/lib/ld-musl-aarch64.so.1`）+ toybox + mksh。**无 glibc、无 systemd、无 X11/Wayland、无 ncurses/terminfo**。
- **文件系统**：鸿蒙 PC 上用户家目录是 `/storage/Users/currentUser`（HMDFS），是唯一可写且跨应用共享的目录；系统目录只读。Harmonybrew 把安装前缀统一固定在该目录下（开发板/容器中同样如此，以求生态路径一致）。
- **Shell**：HiShell = zsh（`/usr/bin/zsh`）；`/bin/sh` 是 mksh；**无 `/bin/bash`**。
- **代码签名**：鸿蒙 PC 对 ELF 有验签。两种方式：链接器签名（OHOS SDK lld 支持 `--code-sign`，Harmonybrew 的 `ohos-sdk` formula 用 wrapper 默认开启，编出的程序可直接在 PC 上运行）与二进制签名工具签名（分发用，覆盖链接器签名照顾不到的场景，如 go 编译、构建后再修改的二进制）。**容器/开发板（Tier 1）不强制**。
- **Tier 1 = 社区版 OpenHarmony（开发板、容器）**，Tier 2 = HarmonyOS PC。Harmonybrew 仅 arm64。

### 1.2 DockerHarmony（hqzing/dockerharmony）

- 把 OpenHarmony mini rootfs 打包成 Docker 镜像（arm64 only），rootfs = musl + toybox + mksh + curl。
- 用于在普通 Linux 服务器/CI 上运行、测试 OHOS 命令行程序。`aarch64-linux-musl` 编译的软件大多可直接运行（例：Alpine 的 `make`）。
- GitHub Actions 用法：`runs-on: ubuntu-24.04-arm` + `container: hqzing/dockerharmony:latest`，需为 actions 准备 Node（挂载卷方案）。
- **注意**：`aarch64-unknown-linux-musl` 静态产物在鸿蒙上**不可运行**（atomcode 注释明确记载：STATIC-musl 构建在 HarmonyOS 上 `permission denied`），必须使用 `*-linux-ohos` 目标产物（动态链系统 musl）。

### 1.3 Harmonybrew（Homebrew 的 OHOS 移植）

- 将 OHOS 视为"特殊的 Linux 发行版"，业务逻辑全部走 Linux 路径；`superenv` 垫片 + `cc/gcc/ld` 软链到 ohos-sdk 的 LLVM（容器内实测：`/usr/bin/clang` 指向 SDK clang-15，默认 target 就是 `aarch64-unknown-linux-ohos`）。
- 强制链接系统 musl libc；剔除 sudo；流水线构建产物统一用二进制签名工具签名。
- **`rust` formula 是特例**：直接分发官方 rust-lang 的 `rust-<ver>-aarch64-unknown-linux-ohos.tar.xz`（当前 1.97.1）。因为官方 dist 的 cargo 动态链接 `libssl.so`/`libcrypto.so`/`libz.so`（实测确认 NEEDED），formula 用 `patchelf --add-rpath` 指向 `openssl@3`、`zlib-ng-compat`，并包装 cargo 设置 `SSL_CERT_FILE`。caveats 建议 `rustup toolchain link system "$(brew --prefix rust)"`。
- **`ohos-sdk` formula**：安装 OpenHarmony SDK（26.0.0.18），并把 `ld.lld` 换成 `exec ... lld --code-sign "$@"` wrapper（链接器签名）。
- 开发/构建**必须在 ci-runner 容器**内进行（`swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest`，DockerHarmony 二次封装，预置 ohos-sdk + brew + GNU 工具链 + make/perl 等）。流水线只接受原生编译（不接受交叉编译）。
- formula 准入：开源、可源码构建、优先搬运上游 homebrew-core 公式并最小修改；新 formula 需过 `brew style`/`brew audit`。C/C++ 代码可用 `__OHOS__` 宏（编译器自动传入）。

### 1.4 ohos-ripgrep（OpenHarmonyPCDeveloper，lycium++ 路线）

- HPKBUILD：`buildtools=cargo`，`archs=("arm64-v8a")`，`makedepends=("cargo" "rustc")`，C 依赖（pcre2）用 lycium 预编译；核心编译命令：
  `cargo build --release --locked --target "${OHOS_RUST_TARGET:?}"`（= `aarch64-unknown-linux-ohos`）。
- lycium++ 的 `build_hpk` 会注入 `OHOS_RUST_TARGET`、`PKG_CONFIG_PATH`、`CARGO_TARGET_*_LINKER`、`OPENSSL_*` 等环境变量。
- 产物：`bin/rg` + `hnp.json`，`hnpcli pack` 打成 `.hnp`（需 SDK `toolchains/hnpcli`，部分 SDK 版本不带，会跳过）；**设备侧运行需二进制签名**。

### 1.5 atomcode（同生态 Rust 大项目的完整适配范例 —— grow 的最佳参照）

- **发布矩阵**：`scripts/release.sh` 增加 `ohos-arm64` 资产；`latest.json` 增加 `binaries."ohos-arm64"` 条目；npm 包用 `optionalDependencies` 分发 `@atomgit.com/atomcode-ohos-arm64`；`install.sh` 识别 HarmonyOS → `os=ohos`。
- **自更新目标识别**（`crates/atomcode-updater/src/lib.rs`）：运行时 `std::env::consts::OS == "linux"` 会误判为 `linux-arm64`（STATIC-musl，跑不了），因此用**编译期** `cfg(any(target_os = "ohos", target_env = "ohos"))` 锁定 `ohos-arm64` 资产。
- **Harmonybrew formula**（`Formula/a/atomcode.rb`）：`depends_on "rust" => :build` + `cargo install` + `--features distro-pm`（编译期关闭自更新，`/upgrade` 引导用户走 `brew upgrade`）。

## 2. Rust 工具链事实（本次实测）

| 事实 | 验证方式 |
|---|---|
| rustc 1.92 内置 4 个 ohos target：`aarch64-unknown-linux-ohos` / `armv7-unknown-linux-ohos` / `loongarch64-unknown-linux-ohos` / `x86_64-unknown-linux-ohos` | `rustc --print target-list`（macOS 宿主实测） |
| `aarch64-unknown-linux-ohos` 的 cfg：`target_os="linux"`、`target_env="ohos"`、`target_family="unix"`、`target_arch="aarch64"` | `rustc --print cfg --target aarch64-unknown-linux-ohos`（macOS 宿主实测） |
| `rustup target add aarch64-unknown-linux-ohos` 在普通宿主上可用（rust-std 有分发） | macOS 宿主实测通过 |
| **OHOS 宿主工具链**：rust-lang 官方 dist 从 **1.93.0 起**才发布 `rust-<ver>-aarch64-unknown-linux-ohos.tar.xz`（1.92.0 = 404） | `curl -I` 实测 |
| rustup 在 OHOS 上把宿主误判为 `aarch64-unknown-linux-musl`，其 rustc 依赖 `libgcc_s.so.1` + `_Unwind_*`，OHOS musl 没有 → **不能用 musl 宿主工具链** | 容器内实测（加载失败） |
| rustup 安装 ohos 宿主工具链需 `--force-non-host`；1.97.1 可装 | 容器内实测 |
| 官方 ohos dist 的 `cargo` 动态链接 `libssl.so` / `libcrypto.so` / `libz.so`（系统只有改名版 `.z.so`，且符号版本不全）→ 必须像 rust formula 那样 rpath 到 openssl@3/zlib-ng-compat | 容器内实测（LD_LIBRARY_PATH 直链改名库仍因符号缺失失败） |
| Harmonybrew `rust` formula 安装即用（rpath 已补） | 容器内 `brew install rust` 实测成功（1.97.1, 599MB） |
| OHOS SDK（ci-runner 内 `/opt/ohos-sdk/ohos/native`）提供 `aarch64-unknown-linux-ohos-clang` 包装（`-target aarch64-linux-ohos --sysroot=.../sysroot`）、`sysroot/usr`、`build/cmake/ohos.toolchain.cmake`、`build-tools/cmake/bin/cmake` | 容器内实测 |
| 容器 SDK 的 `ld.lld` 是裸 lld（**无** `--code-sign` wrapper）；wrapper 只在 brew `ohos-sdk` formula 里 | 容器内实测（对比公式源码） |

结论：**OHOS 构建必须使用 ohos 宿主工具链（≥1.93，推荐跟随 Harmonybrew rust formula 的 1.97.1），并通过 rustup 以 `system` 名义链接**。grow 的 `rust-toolchain.toml` 固定 1.92.0，OHOS CI 需要用 `RUSTUP_TOOLCHAIN=system`（或环境变量覆盖）绕过该 pin。

## 3. Grow 源码适配清单

### 3.1 必须修改

| # | 位置 | 改动 | 理由 |
|---|---|---|---|
| 1 | `crates/codegen/update/src/auto_update.rs` `detect_platform()` | 增加 `cfg!(all(target_os = "linux", target_env = "ohos", target_arch = "aarch64"))` → `Ok("ohos-aarch64")` | 现在 OHOS 上会 `anyhow::bail!("this compile target has no Grow release asset")`，自更新直接不可用 |
| 2 | 同文件 `detect_platform` 测试的 cfg 门 | 增加 `all(target_os = "linux", target_env = "ohos", target_arch = "aarch64")` 分支 | 测试覆盖 |
| 3 | `crates/codegen/tools`（或 shell 配置） | 新增 `distro-pm` 类 feature：编译期关闭自更新（参照 atomcode `--features distro-pm`）；现有运行时开关为配置 `auto_update = false` | Harmonybrew 安装的 grow 不应自更新（会覆盖包管理器文件、且无签名问题风险） |
| 4 | `crates/codegen/tools/build.rs`（或新增 feature） | OHOS 构建**不设置** `GROW_TOOLS_BUNDLE_RG_PATH` → 不内嵌 rg；运行时 `rg_path()` 回退 PATH | 内嵌 rg 解包后无签名，鸿蒙 PC 无法执行；PATH 上的 rg 由 brew `ripgrep` formula 提供（已签名） |
| 5 | `.github/workflows/release.yml` | 见 §4.2 | 发布链路 |

### 3.2 建议修改（运行时降级，非编译阻塞）

| # | 位置 | 现状 | 建议 |
|---|---|---|---|
| 1 | `client-support/src/clipboard.rs`（linux 路径） | arboard / wl-copy / xclip，运行时探测 DISPLAY/WAYLAND_DISPLAY，失败返回 `Err` | OHOS 无 X11/Wayland：确认所有调用点对 `Err` 都优雅降级（不弹窗/不 panic）；可加 `target_env="ohos"` 时直接返回"不支持" |
| 2 | `pager/src/notifications/sleep.rs`（linux 路径） | spawn `systemd-inhibit`，失败置 `platform_unavailable` 后不再尝试 | 已自带降级，**无需修改**（OHOS 无 systemd，静默失效即可） |
| 3 | `sandbox` crate（seccomp） | 子进程网络过滤走 `prctl(PR_SET_NO_NEW_PRIVS)` + seccomp | OHOS 内核默认开 seccomp，但容器/设备策略可能不同：需设备验证；失败路径应降级为不隔离并告警 |
| 4 | `grow-http` | rustls + webpki-roots（bundled）为主，OS roots 加载为兜底 | **无需修改**（bundled roots 不依赖系统证书目录） |
| 5 | `mermaid` 字体 | 内置 Roboto 兜底 | **无需修改** |
| 6 | `config/src/paths.rs` | `home_dir()` → `~/.grow` | OHOS 上 `HOME=/storage/Users/currentUser`（HiShell 设置），**无需修改**；容器内 root 用户为 `/root` |
| 7 | PTY（portable-pty / ptyctl） | forkpty/openpty | 需在真机/容器验证 `/dev/ptmx` 与 forkpty 可用性（`docker exec -it` 有 PTY，容器内可测） |

### 3.3 依赖级评估（编译）

- **C 编译依赖**（随最终二进制链接，全部需 OHOS clang + sysroot）：`ring 0.17.14`（arch 驱动，aarch64 走预生成 asm，可编译）、`aws-lc-sys 0.39.1`（**原生支持 OHOS**：`target_env=="ohos"` 分支、`ohos.toolchain.cmake`，需 `OHOS_NDK_HOME` 环境变量 + cmake）、`tikv-jemalloc-sys`、`zstd-sys`、`libgit2-sys`（vendored，经 agent/codebase-graph/diagnostics/fsnotify/memory 的 `git2`）、`libsqlite3-sys`（bundled，经 fast-worktree `metadata` feature ← shell 默认启用）、`libmimalloc-sys`（codebase-graph）、`libz-sys`（libgit2 链路）。
- **页大小**：aarch64 OHOS 默认 4KB 页，jemalloc 无需额外 env；若目标设备 16K/64K 页，需 `AARCH64_UNKNOWN_LINUX_OHOS_JEMALLOC_SYS_WITH_LG_PAGE`。
- **`.cargo/config.toml`**：增加 `[target.aarch64-unknown-linux-ohos]` 段，rustflags 与 linux 段一致（`force-unwind-tables` + `-Wl,-z,relro,-z,now,-z,noexecstack` 加固）；linker 由环境变量 `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER` 提供（或配置 `linker = "aarch64-unknown-linux-ohos-clang"`）。
- **纯 Rust 大件**（无风险）：reqwest(rustls)、gix、ratatui/crossterm、nix 0.30（`target_os=linux` 分支全部生效）、notify(inotify)、image/resvg/tiny-skia、syntect、pulldown-cmark、rhai 等。
- macOS/windows 专属依赖（core-foundation、windows crate 等）由既有 cfg 隔离，不影响。

### 3.4 无需修改（编译与运行）

- `cfg(target_os = "linux")` 全量生效（§2 表格）。
- `sqlite-journal`（fcntl 自实现）、`fsnotify`（inotify）、`paths`、`gethostname`、`reflink-copy`（ioctl）等 Linux 路径原样可用。

## 4. 发布链路

### 4.1 release.yml 改动

1. `build` 矩阵新增：
   ```yaml
   - asset_platform: ohos-arm64
     target: aarch64-unknown-linux-ohos
     runner: ubuntu-24.04-arm          # GitHub arm64 公共 runner（partner runner 免费）
     builder: ohos                     # 新 builder：docker run ci-runner 容器内原生构建
     smoke: false                      # 宿主无法执行 ohos 二进制
     rg_repository: OpenHarmonyPCDeveloper/ohos-ripgrep   # 或容器内自编 rg
     rg_archive: ripgrep_15.1.0.tar.gz
     rg_runnable: false
     rg_strip_components: 0
   ```
2. 新增 `ohos` builder 步骤：`docker run --rm -v workspace -v cargo/rustup 缓存 swr.../ci-runner:latest` 内执行
   `RUSTUP_TOOLCHAIN=system cargo build --locked --profile release-dist -p cli --bin grow`（用 brew rust；或装 rustup+ohos 宿主工具链）。
3. `GROW_TOOLS_BUNDLE_RG_PATH=/opt/.../rg` + `GROW_TOOLS_BUNDLE_RG_SKIP_EXEC_CHECK=1`（容器内 rg 可执行，跳过检查仅为保险）。
4. **3 处** 9→10 资产清单（`Verify staged binaries` 与 `Verify and publish` 的 `required` 数组 + 资产校验段）：`grow-${version}-ohos-arm64.tar.gz`。
5. 资产命名与 updater 契约：`grow-{version}-ohos-arm64.tar.gz`（与 `detect_platform()` 返回值拼接），内含单个 `grow` 可执行文件。

### 4.2 分发路径对比

| 路径 | 说明 | 签名 | 适用 |
|---|---|---|---|
| **A. Harmonybrew formula**（推荐） | 手写 formula（无上游可搬运）：`depends_on "rust" => :build`、`cargo install`（或 `cargo build --profile release-dist` 后拷贝）、`depends_on "ripgrep"`（提供 PATH rg）；加 `distro-pm` feature 关自更新 | 流水线二进制签名（自动） | 鸿蒙 PC 用户 |
| B. 官方 release 资产（自更新路径） | release.yml 矩阵 + `latest.json` + `/upgrade`；与 A 冲突（自更新会覆盖 brew 文件），需 `distro-pm` 区分 | 自行处理：SDK 的 lld 若带 `--code-sign` wrapper 则链接即签名（当前容器 SDK 裸 lld，需 wrapper 或构建后签名） | 容器、开发板、无 brew 环境 |
| C. lycium++ / HNP | HPKBUILD（`buildtools=cargo`、`source`=grow 仓库 tag tarball、`build()` 里 `cargo build -p cli --bin grow --release --locked --target $OHOS_RUST_TARGET`、`package()` 安装 bin + `hnp.json`、`hnpcli` 打包）；依赖 pcre2 等 C 库需先编 | 设备侧需自行签名 | 开发板、无 brew 设备 |

## 5. 构建环境搭建（容器内实测通过）

```sh
docker pull swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest   # arm64 宿主机
docker run -itd --name grow-ohos-ci -v "$PWD:/workspace/grow" \
  swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest sh

# 容器内
brew install rust                                   # ohos 宿主工具链 1.97.1（rpath 已修）
rustup toolchain link system "$(brew --prefix rust)"
export RUSTUP_TOOLCHAIN=system
export PATH="$PATH:/opt/ohos-sdk/ohos/native/llvm/bin:/opt/ohos-sdk/ohos/native/build-tools/cmake/bin"
export OHOS_NDK_HOME=/opt/ohos-sdk/ohos
export CC_aarch64_unknown_linux_ohos=aarch64-unknown-linux-ohos-clang
export CXX_aarch64_unknown_linux_ohos=aarch64-unknown-linux-ohos-clang++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER=aarch64-unknown-linux-ohos-clang
cd /workspace/grow && cargo check -p cli --locked     # 或 cargo build --profile release-dist -p cli --bin grow
```

- 宿主 == 目标（`aarch64-unknown-linux-ohos`），无需 `--target`。
- 运行验证：`docker run --rm -it -v 产物:/opt/bin hqzing/dockerharmony:latest sh`（OHOS 用户态直跑）。

## 6. 验证实验记录（2026-08-02，OrbStack arm64）

| # | 实验 | 结果 |
|---|---|---|
| 1 | 宿主 macOS：`rustup target add aarch64-unknown-linux-ohos` | ✅ 成功（std 可装） |
| 2 | 宿主 macOS：`cargo check -p auth ... --target aarch64-unknown-linux-ohos` | ❌ ring C 编译缺 sysroot 头（`assert.h`）—— 预期内：**必须用 OHOS SDK clang** |
| 3 | 容器：rustup 默认宿主 `aarch64-unknown-linux-musl` 的 rustc 加载 | ❌ `libgcc_s.so.1` 缺失 + `_Unwind_*` 重定位失败 —— 必须用 ohos 宿主工具链 |
| 4 | 容器：官方 dist 1.92.0 ohos 宿主 | ❌ 不存在（404）；1.93.0+ 有 |
| 5 | 容器：rustup `1.97.1-aarch64-unknown-linux-ohos --force-non-host` | ✅ 安装成功，rustc 可跑；cargo 因 `libssl.so`/`libcrypto.so`/`libz.so` 缺失不可跑 |
| 6 | 容器：LD_LIBRARY_PATH 指向 OHOS 改名库 | ❌ 符号版本不全（`SSL_get0_group_name` 缺失） |
| 7 | 容器：`brew install rust`（1.97.1，599MB） | ✅ 成功，cargo 可运行（rpath 已补） |
| 8 | 容器：`cargo check -p cli --locked`（OHOS 原生，全依赖图） | ⏳ 待完成 |

## 7. 风险与遗留

1. **PTY/终端**：portable-pty 的 forkpty 与 HiShell 终端对 TUI（TERM、颜色、鼠标）的兼容性 —— 需真机（鸿蒙 PC）验证；容器内仅能验证 PTY 基础能力。
2. **代码签名**：自发行（路径 B）需要 lld `--code-sign` wrapper 或构建后签名工具；formula 路径（A）由流水线处理。
3. **内嵌工具**：rg 不内嵌后，无 brew 环境（裸容器/开发板）里 grow 的搜索工具降级为 PATH 查找 —— 文档化。
4. **性能**：arm64 runner 容器内全量 release 构建时间长（参考：本机 aarch64 全量构建约 1-2h），CI 超时需放宽。
5. **rust 版本**：OHOS 工具链最低 1.93；`rust-toolchain.toml` 的 1.92.0 pin 与 OHOS 的 1.97.1 差异需在 CI 中显式处理（`RUSTUP_TOOLCHAIN`）。
6. **沙箱/seccomp 与休眠抑制**：降级路径需在真机过一遍。
7. **hnpcli**：当前 SDK（26.0.0.18）toolchains 无 hnpcli，HNP 打包需单独获取工具。
