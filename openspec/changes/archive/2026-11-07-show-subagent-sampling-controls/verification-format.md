# 验证记录：本特性新增 hunk 的格式整理

范围：仅 `crates/codegen/pager/src/acp/tracker.rs` 中本 change（父会话 `set_subagent_model` 行投影）新增的 hunks 的 rustfmt 格式整理。不含语义、断言或公共契约变化，不改 Shell/Tools/Cargo 清单，不涉及 `src/scrollback/blocks/tool/other.rs`（其前缀行为未变，无需写入）。

## 整理对象

| 位置（整理后行号） | 内容 | 调整 |
| --- | --- | --- |
| `acp/tracker.rs:2565` | `subagent_model_tool_call_to_block` 内的 `request_field` 闭包 | 单行闭包改为 rustfmt 的块体形式 |
| `acp/tracker.rs:2594` | 同函数 `OtherToolCallBlock::new` 的行名 | `format!(...)` 单行调用拆为多行参数 |
| `acp/tracker.rs:8542` | `set_subagent_model_queues_in_place_on_the_same_row` | `assert_eq!` 首两个实参并回一行 |
| `acp/tracker.rs:8561` | 同上，identity 断言后的 summary 断言 | `assert_eq!` 拆为多行 |
| `acp/tracker.rs:8608` | `set_subagent_model_completion_without_typed_admission_is_not_queued` 的用例表 | `(…, "untyped shape")` 元组拆为多行 |
| `acp/tracker.rs:8710` 附近、`8721` | `set_subagent_model_title_lookalike_stays_a_generic_row` | 两处 `let … else`（`tool_call_to_block(&call, …)` 与 `&verified`）的 `else` 断行位置按 rustfmt 固定 |

共 6 个 hunk、24 行文本变更，净增 8 行；全部落在本特性的新增 hunk 内。

## 格式化基准与证据

工具链与配置：`env PATH="$HOME/.cargo/bin:$PATH" rustfmt +1.93.1`（报告 `rustfmt 1.8.0-stable (01f6ddf758 2026-02-11)`），`--edition 2024`（与 workspace `Cargo.toml` 一致），仓库根 `rustfmt.toml` 仅设 `use_field_init_shorthand = true`。

只用于对照，未整文件写回：

```sh
env PATH="$HOME/.cargo/bin:$PATH" rustfmt +1.93.1 --edition 2024 \
  --emit stdout crates/codegen/pager/src/acp/tracker.rs > /tmp/tracker.fmt.rs
```

逐项证据：

- 特性私有投影区（`/// Identity of the parent-bound sampling control tool` 起至 `subagent_model_tool_call_to_block` 结束，123 行）与 rustfmt stdout 对应片段 `cmp` 完全一致。
- 末尾七个 `set_subagent_model_*` 测试区（`fn set_subagent_model_queues_in_place_on_the_same_row` 起至文件末尾，319 行）与 rustfmt stdout 对应片段 `cmp` 完全一致。
- 整理前后整文件在忽略空白与逗号后逐字符相同：无 token 增删、无标识符/字面量/顺序变化，因此无行为或断言语义改变。
- `rustfmt --emit stdout` 退出码 0：整理后的本特性区段是可编译单元中的合法格式结果，未引入语法错误。

未做整文件 `rustfmt --write`：该文件仍存在本特性之外的既有 dirty hunk（`apply_advisory_result` 及其推荐卡片测试区，整理后行 654/664/697/1587/2654/7196 等）。这些区域属其他 owner 的在途改动，本次未触碰，故**不能**声明整文件 fmt 通过；只声明上述两个区段与其 rustfmt 输出等价。

## 未运行的检查及理由

- 未运行 `cargo fmt --check`：等同整文件校验，会伴随既有 dirty hunk 报告失败，无法区分本特性区段。
- 未运行 Cargo 测试/构建、CLI 或 PTY 用例：父已验收该特性 Pager 投影 7/7 与 tracker 180/180；同一时段 `crates/codegen/shell` 的 Cargo/target 由另一 coder 独占；本次为纯格式变更（Tier 0），且格式化不得改变执行语义，重跑不会提供新证据。最终跨模块 CLI 集成由父在里程碑验证执行。

## 边界

本记录只覆盖 `tracker.rs` 的本特性新增 hunk 及其格式等价证据，不覆盖该文件的其它 hunk、整文件 fmt 状态或跨模块链路验证。