## Why

`grow export` 只读取指定 session 的 ACP updates；子 agent 的完整对话保存在独立 session，父级 Task 行又被展示层隐藏，现有导出因此遗漏委派工作。现有 Markdown renderer 还主动丢弃 thinking、Grow 展示和工具详情，不能作为用户会话记录的完整导出。

本 change 是已确认的「会话树导出 + 只读 TUI replay」的第一阶段；第二阶段见 `../replay-session-transcripts/`。

## What Changes

- **BREAKING**：`grow export <session-id> [output-dir]` 统一输出目录。默认根目录为调用时工作目录下的完整 session ID，显式参数直接指定输出根目录；移除 CLI `--clipboard` / `-c` 与 transcript stdout 模式。
- 每个 agent 写 `transcript.md`，直接子 agent 写入 `subagents/<child-session-id>/`，按真实委派父子关系递归。记录 session 身份、子文档链接及可恢复的会话正文。
- 从已验证的 Timeline、展示更新及所需持久引用建立只读会话树；保留现有 response reconciliation、去重、rewind 和用户可见内容选择规则。
- 增加完整的 Markdown 会话展示导出，覆盖已持久且可恢复的消息、thinking、工具结果与 Grow 会话展示；复用现有展示语义，不将模型 Surface 或 trajectory 调试行直接转成对话。
- trajectory 页面增加“导出对话”下载入口。浏览器下载包含相同目录树的 `<session-id>.tar.gz`，不让 HTTP 请求向任意本地路径写文件。
- 已有目标拒绝覆盖；在私有临时目录完成整棵树后再发布，失败不留下被误认成成功导出的最终目录。
- TUI `/export` 和 `/transcript` 继续输出当前 root/child 视图的单份完整 transcript，保留剪贴板与路径语义。只抽取新入口确实需要的共享展示转换，不借此重组整个 Pager。

## Capabilities

### New Capabilities

无新增 capability。

### Modified Capabilities

- `client-surfaces`：新增 CLI 会话树导出、完整对话展示、trajectory 下载及目录发布契约；删除 CLI clipboard export 契约；明确既有单文件原子提交要求的 TUI 适用范围。

## Impact

主要涉及 `pager/src/export_cmd.rs`、`pager/src/app/cli.rs`、`pager/src/scrollback/export.rs`、相关展示投影 helper、`shell/src/session/storage/`、`shell/src/session/trajectory.rs` / `.html` 与 `pager/src/trajectory_cmd.rs`。入口仍由 `cli/src/main.rs` 组合。

不新增事实日志、持久 schema、运行时 agent、模型依赖或 crate。`session-timeline` / `extension-runtime` 现有 authority 和 observational 契约作为约束，当前不需要 delta。实现同步更新用户指南；归档前不将 delta 写入主规范。
