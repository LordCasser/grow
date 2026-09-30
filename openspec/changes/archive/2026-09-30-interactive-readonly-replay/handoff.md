# 实施交接与当前状态

更新时间：2026-09-29。本文件是用户要求保留并标注的实施前草稿清单，不代表当前状态。后续实现与验收见 [verification.md](verification.md)。

## 1. 先读什么

1. [proposal.md](proposal.md)：范围及主规范改动。
2. [design.md](design.md)：固定树、全局时钟、时间映射与只读边界。
3. [ux.md](ux.md)：skill 依据、5/4/3 行布局、完整键鼠优先级。
4. [audit.md](audit.md)：来源限制、草稿缺口和 V01–V26 验证矩阵。
5. [tasks.md](tasks.md)：实施顺序。除文档准备外，功能任务全部未勾选。

本 change 的代码与文档都没有提交或归档。工作区此前已有 export/replay、模型采样等其他未提交变更，不能批量 restore/reset；尤其既有 `replay_cmd.rs`、`transcript_projection.rs`、`storage/transcript.rs` 本身仍是 untracked，不能误当成本次新草稿删除。

## 2. 现有实现、草稿和未实现的区分

以下路径均相对 `crates/codegen/`。

| 分类 | 路径/内容 | 已做到 | 尚未证明/待完成 |
| --- | --- | --- | --- |
| 此前已有实现 | `pager/src/replay_cmd.rs`、`transcript_projection*`、`shell/src/session/storage/transcript*` 基线 | 离线 replay、模拟流式、完整投影、固定来源、中断/Behavior/Goal 修复 | 以此前归档记录为依据；当前草稿修改后必须重新回归，旧测试结果不能代表本 change |
| 已写完局部改动，未验收 | `pager/src/views/block_viewer.rs` | `for_entry` 纯工厂、`apply_pending_entry_actions` raw/data/复制后处理和 factory 测试 | 只做过该文件及两个调用方的 rustfmt/diff check；没有编译、没有运行测试 |
| 已写完局部改动，未验收 | `pager/src/app/root/dispatch/transcript.rs`、`app/agent_view/viewer.rs` | 普通 UI 调用共享 helper；BgTask runtime 数据仍由正常调用方处理 | 需要 V26 正常 UI 回归；这三文件是本轮 helper 提取修改范围 |
| 代码草稿 | `pager/src/replay_cmd/browser.rs` | passive viewer、局部搜索、基本键鼠和子页动作 | 正文拖选、稳定命中、焦点/普通键语义、搜索 generation、复制反馈等尚不完整 |
| 代码草稿 | `pager/src/replay_cmd/tree.rs` | 捕获树、每节点 Player/Browser、共享时钟、路径栈、基本轮转 | 因果锚点、共同已交付 frontier、活动保护、IDLE 映射、可见播放集合及新 panel 尚缺 |
| 代码草稿 | `pager/src/replay_cmd/time.rs` | 本地 offset 格式化与 UTC 分支；固定 offset 单测源码 | 未编译/未跑测试；真正 resolver 故障、DST 与未知混合时间仍需验证 |
| 代码草稿 | `pager/src/replay_cmd.rs` | run 改委托 tree；Player 改为接受协调时间，旧测试临时改为单节点 harness | 生产 tree 路径未验证，不能只跑 test-only harness 后宣称完成 |
| **未闭合，已知构建阻塞** | `shell/src/session/storage/transcript.rs` | 添加 `mod activity`、`TranscriptSession.activity` 字段及 `activity::protected_intervals` 调用 | `storage/transcript/activity.rs` 当前不存在；该引用不能解析。其他编译问题尚未检查 |
| 尚未实现 | IDLE 分类/分段映射/下一记录；Replay panel/帮助/尺寸降级 | 仅有本 change 文档和 delta | 按 tasks 实施；不得将上述草稿算成这些功能已实现 |

上述保留范围在用户最后“已实现的改动可以保留，但是标注”后没有再修改功能代码。此后只补充 change 文档和一项 backlog 债务。保留代码不等于保证当前工作区可构建；已知缺 module 必须作为接手的第一项处理。

## 3. 与原契约的差异

Delta 完整修改了旧的五项要求：顶层退出/搜索输入边界、单时钟暂停语义、默认不压缩 IDLE、完成态 parity、粘贴处理。新增加被动交互、树导航、panel、本地时区、IDLE 分类和显式下一记录要求。

此前默认“真实 idle 只随倍速缩放”将被本 change 的“>30s 压缩到 1s P 并提示”取代。固定时间是倍速作用前的播放时间；提示常驻到下次替换，避免以最低 wall-clock 停留时间引入第二个时钟。

源码中的任何当前行为都不能覆盖这些目标；实现与目标存在差异时在 change 内记录并修正，不把当前 WIP 反写到主规范。

## 4. 本轮做过与没有做的验证

已完成：仓库/规范只读调查、ui-ux-pro-max 必须的 design-system 与两项 UX 检索、Luna 子 agent 的只读底部组件核对。局部 helper 修改曾完成 rustfmt 与 diff check；未运行其单元测试。

文档验证结果：

- `openspec validate interactive-readonly-replay --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：15 项通过，0 失败。
- `git diff --check`：通过；change 内 Markdown 本地链接及行末空白检查通过。
- 5 个 MODIFIED requirement 的旧 Scenario 标题保留检查：通过；修改正文承载新退出/时间/粘贴语义，未用删除旧场景掩盖冲突。

这些检查只证明文档结构和基本一致性，不证明功能可用。**未运行 Cargo 编译、Rust 单元/集成测试、CLI/PTY、视觉验收和真实 session smoke；未修改安装的 grow；未归档 change。** 保留草稿的模块缺失是静态检查确认的阻塞，不是假装运行编译得到的日志。

磁盘：调查时共享 `target` 约 8.2 GiB，本轮未产生 Cargo 构建产物，不清理其他任务的共享 target。实施时为本次验证记录专属产物路径，只清理自己创建的构建残留。

## 5. 建议给实施 agent 的任务

> 按 `openspec/changes/interactive-readonly-replay/` 实现。先看 handoff 的代码状态与缺 module 阻塞，保留工作区其他改动。按 design/ux/delta 补全被动详情与嵌套子 agent、统一时钟及 IDLE 分类、本地时区和底部面板。尤其覆盖用户打断后追加输入、旧 terminal 晚到、Goal 设置/退出/clear 与并行子任务。旧记录缺少精确等待事实时遵守保守与估算策略，不启动 live session 补历史。每项验证通过再勾 tasks，完成后按仓库流程归档；不要只跑旧 test-only Player harness。
