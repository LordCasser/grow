# 验证

## 构建与实际测试

最终源码在 Rust 1.93.1 下执行 `cargo test --locked -p pager --lib --test settings_e2e --no-run --message-format=json` 成功。解析 Cargo 的 compiler-artifact，沿现有 `RUST_MIN_STACK=16777216`、Pager package cwd 直接运行产物，避免枚举阶段无关的重复编译。

- 完整 settings_e2e：235 passed、0 failed、0 ignored（原 231 + 4 个执行全表的测试）。
- Pager library `settings --test-threads=4`：333 passed、0 failed、1 ignored；包含两个新增 dispatch 生命周期/queued reset 测试。唯一 ignored 为既有 picker_visual_smoke_debug，不是本轮跳过失败。
- 新增每类用例全部实际执行，默认值与 writer 不改的范围见 coverage.md。生产问题另见 [默认值修复](../2026-10-20-align-preference-rollback-defaults/verification.md)。

## 失败记录与修正

首次编译新增 MouseEvent::new 不符合 crossterm API，已改为现有 struct literal。首次 library 生命周期暴露 fixture 的 page_flip cache 与 UiConfig snapshot 不一致；使用本用例独立初始化缓存后重新执行，没有改生产默认来迁就夹具。旧 test helper 的 permission_mode 改走 SetDefaultPermissionMode；模型 helper 通过真实 setter 返回可检查 effects，保留旧场景。之后真实 show_tips 回滚失败导致库组 332 passed / 1 failed，独立行为 change 修复后最终全部相关测试通过。

## 审查

独立只读审查复核 key→typed action、mouse 实际 renderer 命中、reset 确认入口及队列基线。审查中关于 Int d 不支持的初步判断被生产完整分支证据否定并撤回，未据此改测试。旧主题鼠标测试的过时说明修正为实际 select-only 或无 choice hitbox 的语义，保留原断言。

完整设置组已在现有 Core CI 运行，无需新增流水线、测试生成框架或发布版本。只提交本轮改动。

最终校验：修改 Rust 文件 rustfmt --check、git diff --check 通过；归档前 current/all strict 18 passed、0 failed；归档后 current/all strict 16 passed、0 failed，archived strict 666 passed、0 failed。
