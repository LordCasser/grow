## Why

现有 session/load 的 replay 用于恢复历史并继续执行，不能作为无副作用的演示播放器。用户希望用 CLI 在没有输入框的现有 TUI 展示中重演某个 session，以指定倍速观看流式输出和工具/状态变化；允许流式节奏近似，但内容必须来自历史。

本 change 依赖 [已归档的 export-session-transcript-tree](../archive/2026-09-29-export-session-transcript-tree/) 的只读来源、完整会话展示与 provenance。

## What Changes

- 新增 `grow replay <session-id> --speed <positive-finite-number>`，默认 1 倍；在 CLI 内启动独立只读 TUI。
- 复用已有 ScrollbackPane、RenderBlock、展示投影及必要布局组件，底部显示播放状态/时间/倍速；没有输入框、slash 提交、权限确认或执行 session。
- 支持暂停/继续、调速、滚动/折叠和退出；播放完成后保留最终可阅读状态。
- 从已保存时间证据安排事件，用仅内存存在的文本揭示过程模拟流式；缺少证据时使用确定性估算并标明“模拟流式”，不新增持久 token 记录。
- 源是固定只读快照，不追尾，不调用模型、工具、Hook 或任何 session/load 恢复执行路径。

## Capabilities

### New Capabilities

无新增 capability。

### Modified Capabilities

- `client-surfaces`：新增独立只读 TUI replay、倍速/暂停时钟、近似流式、历史状态与最终展示一致性契约。

## Impact

CLI 分发与参数、Pager 只读播放入口/状态、现有展示投影与滚动渲染的窄复用点。数据来源复用前序 change 的 `shell::session::storage` reader。

不改持久化格式，不新增执行会话、provider/MCP 连接、后台恢复或精确录像机制；不在此 change 实现多 agent 同步分屏、拖动 seek、网页播放器、导出视频或 transcript 导入。
