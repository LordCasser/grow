# Root cause and verification

## Root cause

Session `01a15c48-ab5b-7872-a263-15c28b8d8e69` 在 2026-10-21 17:44:24（Asia/Shanghai）的 prompt `f3c0763d-bcf3-4e72-8978-21e0f7f13bd6` 失败于本地 checkpoint，未出现对应 ACP send；原始日志位置见前项 [占位修复记录](../2026-10-21-release-unsent-image-submission/verification.md)（归档后相邻目录）。

`process_effects` 强制所有 prompt RPC 先经过 `checkpoint_input_intent`，但 `capture_agent` 明确以 Unsupported 拒绝含图片的 composer、uncertain submission 和 staged prompt。`sync_subjects` 记录捕获警告后没有生成 tracked snapshot，随后的 `persist_input_checkpoint` 返回 `input intent was not captured`。这是确定性的本地策略冲突，不是模型图片支持、图片格式或网络问题的证据。

`git log -S` 显示强制 checkpoint 与 uncertain 图片拒绝均在 `0a0dd07c` 的批量运行时提交中引入；这只能定位源码引入提交，不能单独推断用户二进制的构建版本。

此前 change 只修复残留 TurnSubmitting，本项修复首次发送根因。显式 images 使用已有 RAM-only 身份准备，仍通过异步结果的 session/binding/prompt 校验；日志区分 memory_only 与磁盘 checkpoint。服务端附件接纳仍走原路径。

## Verification

- 首次编译遇到新增测试缺少 `Arc` import（E0433），补齐后通过。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib first_send_ -- --test-threads=1`：4 passed，0 failed。新增三项测试覆盖实际首次普通/steer 的请求内容及成功/失败、准备后换绑拒绝、实际磁盘 I/O 失败；另包含既有 session bind 首次 checkpoint 回归。测试使用隔离 GROW_HOME。
- 同一编译产物 `target/debug/deps/pager-a429a183957e3f63` 执行 `app::root::dispatch::tests::turn_pipeline::`：10 passed；`image_loader`：4 passed，覆盖文件与内存内容一致、缺失/超预算/特殊文件拒绝及文字占位符不读取。
- `rustfmt --edition 2024 --check`（event_loop.rs）及 `git diff --check` 通过；没有引入新依赖。
- 归档前全量严格 OpenSpec：21 passed，0 failed。
- 归档后全量严格 OpenSpec：20 passed，0 failed；archive 校验：676 passed，0 failed。
- 复用已有 debug target；磁盘约 1.2 TiB 可用，没有新建完整构建目录。

## Review boundaries

- 显式附件判断不检查用户文字，纯文字图片路径不能触发读取或绕过 checkpoint。
- 不根据错误文本吞掉存储失败；普通输入仍走磁盘 gate。
- 内存准备完成后仍验证 view binding，重绑定的迟到结果不能发送。
- 图片内容在发送 effect 中继续经数量/文件/编码预算校验；请求失败沿用既有恢复，不自动派发新 identity。
- 不改变图片草稿无法跨重启恢复的限制，不扩大 256 KiB 格式，不新增附件缓存。
- 验证范围为真实 Pager event loop 到 ACP command 的本地受控传输；未调用远端模型，不推断 provider 行为。
