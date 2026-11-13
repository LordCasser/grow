# Analyze resume and exit working-state costs

## Why

用户要求分析 resume 和 Ctrl+D 保存速度，目标是历史持续落盘，退出仅保存现场，恢复现场后历史从最近一页开始按需加载。此前修复只将挂起树执行释放移到加载准备末尾，没有消除随历史增长的工作。

## What Changes

仅记录当前实现证据、成本来源、建议的状态边界、分步实施顺序和验收方法。不修改生产代码、持久格式或当前规范。`skip_specs: true`，因为本次交付是分析；设计中的目标不是已实现契约，也不是本次实施任务。

## Impact

分析涉及 ChatState、Shell persistence/storage、树挂起与恢复、子任务结算和 Pager。未来实施需要分别修改 session-timeline 与 client-surfaces 契约，尤其是全历史校验、全历史 ready 和分页限制。本次不更改这些要求，不触碰真实会话，不运行 provider 或 resume。
