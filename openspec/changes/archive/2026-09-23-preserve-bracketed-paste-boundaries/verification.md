## Evidence

- Crossterm 0.29 的 `parse_csi_bracketed_paste` 在结束标记到达前返回 `None`，结束后才发出包含完整字符串的 `Event::Paste`；`Event::Paste` 的公共文档也定义为一次 pasted string。Windows 输入解析路径未生成该事件。原先把相邻按键视为 bracketed paste 残片没有协议依据。
- `collect_input_batch` 只确定同一次 drain 的公平性窗口，不能证明事件属于同一次 paste。`coalesce_rapid_keys` 的旧分支会合并 Enter/字符、丢弃控制键与方向键，两个 `Event::Paste` 也会合并。

## Verification

- `cargo test -p pager --lib bracketed_paste -- --test-threads=1`：13 passed，0 failed，覆盖 Enter/字符/方向键/Ctrl+C、连续 paste、Release 过滤和未 bracketed 多行粘贴。构建仅有 macOS linker 的 `__eh_frame` 警告。
- `rustfmt --edition 2024 --check crates/codegen/pager/src/app/root/event_loop.rs`：通过。
- `openspec validate preserve-bracketed-paste-boundaries --strict --no-interactive`：通过。
- `openspec archive preserve-bracketed-paste-boundaries --yes`：成功合入 `client-surfaces`。
- `openspec validate --archived --no-interactive`：373/373 通过。全量 strict 待并行的 MRU change 完成后复查。
