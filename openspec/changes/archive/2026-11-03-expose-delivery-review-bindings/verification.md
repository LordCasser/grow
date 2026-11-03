# 验证记录

工具链使用 rustup 的 Rust/Cargo 1.93.1，按 rust-toolchain.toml。Cargo 验证设置 CARGO_BUILD_JOBS=2、RUST_MIN_STACK=16777216，复用原 target。

- `cargo test --locked -p tools --lib implementations::grow_build::todo::tests -- --test-threads=2`：26 passed，含从模型摘要解析完整 resultId/null 和当前 revision。
- `cargo test --locked -p shell --lib planning_ -- --test-threads=2`：33 passed。新用例覆盖摘要/请求上下文的相同绑定、accepted/released 提交、冷 Timeline replay、null/seq 猜测提交前拒绝、未展示第九项的准确错误绑定。既有当前要求复核、无结果释放、活动执行不伪造终态和重复阻塞停止回归通过。首次编译中的测试夹具 EventSeq 类型和借用错误已修正后重跑通过。
- `cargo test --locked -p shell --lib interjection -- --test-threads=2`：17 passed，含关联长输入修复；未弱化完成门禁。
- 本次涉及 Rust 文件的 `rustfmt --edition 2024 --config skip_children=true --check` 与 `git diff --check` 通过。
- `cargo fmt --all -- --check` 检出本次范围外既有格式差异，包括 agent/config.rs、pager 的辅助图片 PTY fixture、shell subagent/session_admin/task；不顺带重排这些工作区改动。
- `openspec validate --all --strict --no-interactive`：归档前 19 passed、0 failed。
- 两项 change 归档后：全量严格校验 17 passed、0 failed；`openspec validate --archived --no-interactive` 824 passed、0 failed。首次 archive 校验只因两项归档收尾 checkbox 未勾选失败，按实际完成状态更新后通过。

以上使用生产 PlanningCommand::Write 和 Timeline fold；没有调用线上模型评估成功率，没有修改用户现场 session、安装或替换正在运行的二进制。原 session 的五项义务保留，由运行新代码的 Agent 根据实际结果显式处置。

## 场景对应

- Review a delivered result from the Todo summary：`review_binding_from_todo_summary_accepts_and_cold_replays`。
- Release a terminal delivery with an incorrect result identity：`mismatched_review_result_is_rejected_with_exact_binding_before_commit`（含 null/单独 seq 的 Released）。
- Release a task without a delivered result：tools 的 null binding 回归及既有 `released_required_undispatched_scope_switch_succeeds_without_faking_result` 等 actor 回归。
- Locate a review beyond the bounded summary：`rejected_review_for_link_outside_summary_still_reports_its_binding`。
