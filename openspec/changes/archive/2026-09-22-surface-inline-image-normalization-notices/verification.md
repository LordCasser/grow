# Verification

## Actual boundary

`handle_prompt` 已对 ACP attachments 使用 `normalize_images_with_notices`，但 query extraction 后直接调用 `normalize_images`，丢弃除 surviving images 外的结果。修复只让第二个入口使用同一 helper；没有更改 parser、图片预算、worker admission、tool-result normalization、权限文本生成或通知格式。

两个输入来源仍各自形成批次。`permission_text` 仍在清理后的 query 上计算，helper 将 runtime 说明写入 context，不把说明拼进直接用户授权文本。

## Regression evidence

使用真实 `handle_prompt`、本地 MockInferenceServer 的 Messages SSE、actor 通知 gateway 和 ChatState Timeline。测试 payload 均越过 `extract_base64_images` 的 1024 字符下限，不通过直接调用 helper 伪装 production 接线。

- 混合输入：两个同原因损坏图、一个 3000×2000 待压缩图、一个正常图。验证 wire 中仅两张 surviving images，dropped/compression 模型说明各一次，受影响索引正确，UI 各一个相应通知。
- 验证 User image group 内的图数量和顺序，正常图原 URI 保留；对实际 Timeline events 重新 fold 后再次验证图片和说明。这是 Timeline 冷 fold 验证，不是文件系统 cold-load 或远程 provider 测试。
- 正常内嵌图：wire 保留图片，不出现 dropped/compression 模型说明和通知。
- fallback 继续由同一个既有 helper 的 `re_encode_fallbacks` 分支处理。未新增为测试专用的生产 budget 开关；本次没有强制真实 prompt 触发编码器 fallback，边界由现有 normalization 回归及共享入口源码核对覆盖。

修复前：**1 passed / 1 failed**。混合场景在 request 中 `<image_dropped_notice>` 数量为 0（预期 1）处失败；正常场景通过。日志 `/tmp/grow-inline-image-before.log`。

修复后：**2 passed / 0 failed**，3.40 秒。日志 `/tmp/grow-inline-image-after.log`。

## Commands and environment

实际工具链为 Homebrew `rustc 1.98.1` / `cargo 1.98.1`，不声称执行了仓库声明的其他版本。

`CARGO_TARGET_DIR=/tmp/grow-pager-recovery-target CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUST_MIN_STACK=33554432 cargo test --locked -p shell --lib image_input_recovery_tests::inline_ -- --test-threads=1`。

定向测试仅使用本地 loopback，不接触外部模型服务。Linker 报既有大 debug test binary 的 `__eh_frame` warning，未导致失败。

## Final validation

受影响整包命令：`CARGO_TARGET_DIR=/tmp/grow-pager-recovery-target CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked -p shell -p pager --lib -- --test-threads=4`。

- Shell：**3,883 passed / 0 failed / 3 ignored**，89.08 秒，覆盖本次图片测试、已有 normalization/fallback 和持久化恢复回归。
- Pager：**7,242 passed / 0 failed / 11 ignored**，20.64 秒。
- 整包日志：`/tmp/grow-backlog-shell-pager-final.log`。
- 改动文件 rustfmt 与 `git diff --check` 通过；归档前严格 OpenSpec 为 **22 passed / 0 failed**。

独立 review 未发现 helper 接线的生产缺陷。review 后额外强化 mixed 测试：完整比较 live/replay 图片顺序与来源文本，比较权限证据并断言其精确等于 cleaned query（包含 parser 既有的 `<user_query>` 包装），确认图片 payload 不进入文本说明。加强断言初次遗漏这一既有包装，核对 `prompt_parser.rs` 后修正期望值；没有为此改动生产代码。最终定向 **2 passed / 0 failed**，3.26 秒，日志 `/tmp/grow-inline-image-reviewed.log`。UI 通知相互顺序不属于本次契约，未新增此顺序保证。

最终定向后再次通过六个改动 Rust 文件的 rustfmt、全工作树 `git diff --check` 和严格 OpenSpec（22 passed / 0 failed）。`cargo clean --target-dir /tmp/grow-pager-recovery-target` 删除本轮 9,993 个文件、**6.9 GiB**；清理后磁盘剩余 **26 GiB**。临时文本日志保留，不清理其他任务的构建目录。

已使用 `openspec archive surface-inline-image-normalization-notices --yes` 归档，向 `input-admission` 主规范合入 1 项 requirement。两项本轮变更归档后，全量严格校验 **20 passed / 0 failed**，archive 校验 **356 passed / 0 failed**；日志分别为 `/tmp/grow-backlog-openspec-final.log` 和 `/tmp/grow-backlog-openspec-archived.log`。未执行 Git 提交。
