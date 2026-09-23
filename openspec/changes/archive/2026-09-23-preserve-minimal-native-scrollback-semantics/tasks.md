## 1. Baseline

- [x] 1.1 复核 BlockOutput joiner、EntryRenderer 布局、commit frontier、两种 inline viewport/scrolling-regions 路径和 OSC8 测试；记录与并行文件改动的边界。

## 2. Implementation

- [x] 2.1 为本次 renderer 导出可靠的逐行软接续 provenance；单测 vpad、group header、skip_rows、恰满宽硬行和不同 joiner。
- [x] 2.2 将 commit 的 offscreen Buffer 与 LinkSpan 投影为语义行，限定尾部 pad、宽字、背景、footer、链接和 `commit_h`；不改变稳定条目/打印 frontier 判定。
- [x] 2.3 在 vendored `ratatui-inline` 新增真实的语义行插入 API，保留 viewport 位移、跨屏分块、OSC8/SGR 和两种 scrolling-regions 配置；错误出口尽力清理控制状态并保持 DECAWM 启用。

## 3. Verification

- [x] 3.1 运行 provenance 与 serializer 纯单测、terminal RecordingBackend 字节/滚动量测试、minimal frontier/error/resize 和 transcript 回归。
- [x] 3.2 实际运行至少一个 PTY/VTE 的复制、链接、跨屏和 resize 场景；不能运行时记录限制，不以 Buffer 快照代替。
- [x] 3.3 更新开发者说明并将命令、退出码、场景证据、已知终端差异记入 `verification.md`。
- [x] 3.4 核对 delta、执行全量 strict OpenSpec、归档本 change，再执行全量和 archived 校验；未验证项不勾选。
