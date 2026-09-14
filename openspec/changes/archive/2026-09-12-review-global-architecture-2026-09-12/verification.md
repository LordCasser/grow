# 验证记录

本次只审计并编写方案，未实施产品修改。验证对象包含审计开始前已有的工作树修改；源码版本见 [evidence.json](evidence.json)。报告中的未来验收目标不代表已经通过。

## 已执行

| 验证 | 结果 | 范围与限制 |
| --- | --- | --- |
| `cargo metadata --no-deps --format-version 1 --locked` | 成功 | 57 个 workspace member，197 条内部 normal dependency，包含 optional 边；不代表最终平台编译闭包 |
| `cargo check --locked -p cli` | exit 0，1m26s | 当前主机 CLI 编译检查；未执行 CLI 交互或其他平台 |
| 核心四库测试，命令见下 | exit 0，1062 passed / 1 ignored | 不含 shell、pager、pager-minimal 和平台集成测试 |
| 用量恢复探针 | exit 0，复现 live/replay 校验差异 | 真实 ChatState API，内存 Timeline；异常记录由 probe 构造，未读取用户会话、未调用 Provider |

核心测试命令：

```sh
RUST_MIN_STACK=16777216 cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p workflow -- --test-threads=4
```

实际摘要：

```text
chat-state:     480 passed; 0 failed; 1 ignored; finished in 13.81s
sampler:        241 passed; 0 failed; 0 ignored; finished in 12.84s
sampling-types: 276 passed; 0 failed; 0 ignored; finished in 0.02s
workflow:        65 passed; 0 failed; 0 ignored; finished in 1.14s
```

库测试覆盖已存在场景。冲突账单恢复是本轮另行构造的新场景，测试通过不能抵消该复现。

## 用量探针

[usage-restore-probe.rs](usage-restore-probe.rs) 是独立实验源码，不属于任何 Cargo target；[usage-restore-probe-output.txt](usage-restore-probe-output.txt) 是本轮实际运行输出。早期探针脚手架未形成可运行证据，已舍弃；报告只采用最后成功执行的版本。

准备阶段曾执行 `cargo build --locked -p chat-state -p sampling-types`，exit 0，约 40.7s，复用现有 target。收到磁盘提醒后的成功 probe 直接链接已有 rlib，没有再次运行 Cargo：

```sh
rustc --edition=2024 -C panic=abort -C debuginfo=0 -C strip=symbols \
  -L dependency=target/debug/deps \
  --extern chat_state=target/debug/libchat_state.rlib \
  --extern sampling_types=target/debug/libsampling_types.rlib \
  --extern tokio=target/debug/deps/libtokio-22b650220ad01ecf.rlib \
  --extern serde_json=target/debug/deps/libserde_json-4af805cc350f79ee.rlib \
  /tmp/grow-architecture-probes/confirmed_probe.rs \
  -o /tmp/grow-architecture-probes/confirmed_probe
/tmp/grow-architecture-probes/confirmed_probe
```

以上是执行时的具体路径；rlib hash 与机器/构建配置相关，不能作为跨机器固定命令。复现时使用本机相应依赖产物，并将本目录 probe 源码作为输入。临时可执行文件不进入仓库。

## CI 与未执行范围

核对 6 份 workflow：

- [core-regression.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/core-regression.yml)：Ubuntu 上 8 个核心库和 CLI build。
- [local-coordination.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/local-coordination.yml)：macOS/Linux/Windows 库测试及独立进程协调测试。
- [windows-session-storage.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/windows-session-storage.yml)：Windows 定向存储场景。
- [openspec.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/openspec.yml)：规范与归档验证。
- [build-one.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/build-one.yml) 与 [release.yml](/Users/lordcasser/workspace/projects/grow/.github/workflows/release.yml)：跨平台构建、版本与协调 smoke。

本轮没有运行这些远端 CI。未发现 PTY resume 交互/性能测试接入常规 CI，不等于项目没有 PTY 测试。

未运行：shell/pager 全库、真实终端 resume、session_load_perf ignored benchmarks、session load RSS 集成、真实 Provider、Windows/Linux 运行时、崩溃注入全矩阵。没有真实用户慢 session 的 wall-time profile，报告不提供加速倍数或主瓶颈占比。

## 工作树、规范与磁盘

源代码摘要用于确认本轮没有修改既有产品代码。Atlas 局部 Focus 提供了结构导航，但部分行号与磁盘代码不一致，最终引用已回到源码核对；没有运行全仓 Atlas index。

主规范共 14 个 capability、314 个 `### Requirement:` 标题（机械计数）；现有 3 个活跃 change 及其未完成任务保持原状。全量依赖盘点和源码摘要不是对每个 requirement 的逐场景验证证明。

磁盘测量时 Data 卷约余 87 GiB，target 约 6.2 GiB；没有开始前测量，不能据此计算本轮 build 增量。没有新建 worktree、target 目录或大型会话副本。探针临时目录约 7.1 MiB，交付仅保留小型文本证据，临时二进制及中间文件在交付前清理。

最终文档和归档验证结果见下。

## 交付检查

- 本目录 Markdown 的 47 个本地链接均存在，带行号的引用未越界；新增 backlog 索引指向归档后的报告。
- 75 份证据源文件的 SHA-256 与读取版本一致；相对于开始时的 Git status，仅增加 backlog 文档修改和本次审计目录，原有修改状态全部保留。
- 归档前 `openspec validate --all --strict --no-interactive`：exit 0，18 passed / 0 failed。
- `openspec archive review-global-architecture-2026-09-12 --skip-specs --yes`：exit 0，仅归档文档，无主规范更新。
- 归档后全量严格验证：exit 0，17 passed / 0 failed（14 个 spec + 原有 3 个活跃 change）。
- 第一次归档校验：322 passed / 1 failed，仅本审计的最后一个“归档交付”任务尚未勾选；完成归档交付记录后再次运行 `openspec validate --archived --no-interactive`：exit 0，323 passed / 0 failed。没有修改其他归档任务以消除失败。
- 临时 probe 源码和输出已复制为本目录的文本证据；自建临时二进制、初稿、metadata 和验证中间日志在交付时删除。target、原有工作树、真实会话和其他缓存保留。

本审计任务完成不表示任何优化已实施。后续先取得用户对批次的批准，再分别建立产品 change。
