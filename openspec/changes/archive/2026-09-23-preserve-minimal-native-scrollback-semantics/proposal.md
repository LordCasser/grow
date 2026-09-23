## Why

Grow minimal 模式将稳定条目印入终端原生 scrollback，但当前 `commit.rs` 将渲染后的稠密 cell grid 交给 vendored `ratatui-inline::insert_before_with_links`。布局补齐的空格、每个物理行被当作硬换行的输出，会进入终端复制内容；长路径/YAML 等逻辑一行可能被切断。上游 grok-build 新增语义行和 native wrap 写入解决共同问题，但 Grow 还有 OSC 8 LinkSpan、不同的 print-once frontier 与错误重试语义，不能直接搬用。

## What Changes

- 从 Grow 已有 ScrollbackEntry/BlockOutput 的行 joiner 导出软换行 provenance，与用于 commit 的 renderer、宽度和布局偏移一致。
- minimal commit 离屏绘制后生成语义行：仅去除无语义的布局尾部填充，保留真正可见空格、宽字、样式、截断 footer 及 OSC 8 链接身份。
- 为 vendored `ratatui-inline` 增加带链接的语义插入路径，满宽且确为软接续时使用原生 autowrap，其他行使用硬换行；保持 viewport、滚动区、跨屏分块和写失败 frontier。
- 用 serializer/terminal/PTY 测试验证复制内容及渲染，而不是仅比较 Buffer。

## Capabilities

### Modified Capabilities

- `client-surfaces`: minimal native scrollback 保留源文本逻辑行、样式与链接，而不复制布局填充。

## Impact

- 入口：`crates/codegen/pager/src/scrollback/wrappers/entry_renderer.rs`、`crates/codegen/pager-minimal/src/{commit,full_view}.rs`、`crates/codegen/ratatui-inline/src/terminal.rs` 与对应测试。
- 保持 `pager-minimal` 的 active/tail 高度计算、committable 判定、打印 frontier、transcript 生成与 resize 所有权；不借机重写渲染框架或普通 fullscreen 界面。
- 与 Kitty 退出 fence 只共享 PTY 验证设施，不共享生产代码；实施时保留并行 worktree 修改。
