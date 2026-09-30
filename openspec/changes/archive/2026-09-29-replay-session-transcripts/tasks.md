## 1. 来源与只读入口

- [x] 1.1 确认前序 export-session-transcript-tree 已实现并验证；复核其 reader、完整投影和时间 provenance API，用共享 fixture 比较导出节点正文，避免复制一套读取/可见性规则。
- [x] 1.2 增加 CLI replay 参数和离线入口；验证默认 1 倍、合法浮点倍速、非法/不可换算值、无模型配置与来源缺失路径，错误不遗留 raw terminal。
- [x] 1.3 以生产 reader 捕获指定 session 固定快照，不进入 app::run/session/load；用计数或 fail-on-call 桩验证无 provider、工具、Hook、ACP request、scheduler/workflow 恢复，并比较源/工作区前后内容哈希。

## 2. 时钟与渐进展示

- [x] 2.1 实现单一虚拟时钟与 Playing/Paused/Finished；用假单调时钟验证倍速、暂停/恢复、调速连续、相同/倒退时间及绘制延迟不累计漂移。
- [x] 2.2 将历史业务 elapsed 和必要 motion 样本接到虚拟时间；验证暂停冻结、终态历史 duration、真实日期展示与长 idle，不把合成 now 作为历史时间。
- [x] 2.3 实现绑定稳定条目的文本揭示，复用 canonical 正文与 grapheme 边界；验证 Unicode/Markdown、无 timing 估算、独立事件交错、依赖边界收束和高倍速合帧，不生成每字符无界队列。
- [x] 2.4 覆盖 discarded/quarantined/rewound candidate、只有最终工具结果和读到未完成尾部的 fixture；验证不复活排除正文、不虚构输出/成功终态。

## 3. TUI 与集成验收

- [x] 3.1 复用 ScrollbackPane/blocks 与纯布局建立无输入框页面和只读 keymap；验证 Space、调速、滚动/折叠/选择、阅读时不抢视窗、Finished 保留及 q/Esc/Ctrl-C 退出。
- [x] 3.2 通过真实 PTY 验证无 composer/提交控件，Enter/slash/paste/权限/任务操作不会产生执行请求，正常/错误/中断退出均恢复终端；不把隐藏输入框测试当作 no-execution 的唯一证据。
- [x] 3.3 对同一真实 reader fixture 在多个倍速、暂停和 child ID 下完成播放；归一化比较对应完整导出节点的消息、工具与 Grow 内容；覆盖慢终端和大正文的有界控制响应。
- [x] 3.4 更新会话用户指南与 CLI 帮助，说明模拟流式、快照边界、播放控制和一次一个 session；运行 `cargo check --locked -p cli`、相关 pager lib/PTY 与共享 shell reader 回归，检查磁盘并清理本次不再需要的 Cargo 产物。
- [x] 3.5 逐项记录真实验证结果与未覆盖边界；运行 `openspec validate --all --strict --no-interactive`，只有任务与场景落实后才归档，再校验全部规范与 archive。
