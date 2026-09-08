# prompt-queue 逐包核查

包路径：`crates/codegen/prompt-queue`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/prompt-queue/Cargo.toml`
- `crates/codegen/prompt-queue/src/combine.rs`
- `crates/codegen/prompt-queue/src/lib.rs`
- `crates/codegen/prompt-queue/src/types.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Eligible contiguous prompt combining](../specs/input-admission/spec.md#requirement-eligible-contiguous-prompt-combining)：队列合并 SHALL 仅合并连续可参与的前缀：普通非 synthetic、非 skill 展开、非 bash 且 text 非空；首项可携带图片，后续项不得携带图片或处于 skip_ids。
- [Combined prompt presentation metadata](../specs/input-admission/spec.md#requirement-combined-prompt-presentation-metadata)：文本合并 SHALL 用两个换行连接非空片段，至少两个展示片段时才写 combinedDisplayTexts 元数据。
- [Structured queue wire state](../specs/input-admission/spec.md#requirement-structured-queue-wire-state)：QueueChanged SHALL 携带 session_id、队列项及可选运行中输入信息；QueueEntryWire 保留 id、version、owner、last_editor、kind、text、position 和组合文本，ForegroundSnapshot 明确携带 origin 与 turn_kind。

## 边界

- 合并在该项前停止，不越过它合并后面的输入。
- 空队列返回长度 0；不合格首项返回长度 1。
- stamp_combined_display_texts 写入字符串数组，可保留多个气泡的边界。
- 不新增 combinedDisplayTexts；join_texts 跳过空字符串。
- 失败，不将广播默认路由到任意会话。
- 按类型默认值解析；None 的可选字段在序列化时省略，wire 键使用 camelCase。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
