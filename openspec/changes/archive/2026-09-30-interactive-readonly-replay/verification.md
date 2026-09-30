# 实施与验收记录

日期：2026-09-30。以下命令均在 `crates/codegen/` 执行；真实 PTY 使用本工作树编出的 `target/debug/grow`。测试没有把真实会话正文写进仓库。

## 实施核对

- `grow replay` 保持 CLI 早分支，只调用捕获树 reader、离线 projection 和专用 TUI；没有创建 SessionActor、provider、ACP 连接、工具、Hook 或外部 opener 的路径。详情使用保存的 entry；后台任务缺失的 stdout 明确说明，不读取当前任务输出文件。只有用户明确复制时调用剪贴板。
- `capture_tree_at` 一次确定委派拓扑和验证过的来源；逐节点 `read_session` 提取 Timeline、sideband、后台任务的活动依据。缺失后台任务时间锚点时，全树不宣称已确认 IDLE。节点导航不重新读取磁盘。
- 同一播放时钟驱动全部可导航节点；节点各自保留投影、滚动、折叠、选区和搜索。因果锚点取验证过的 owner spawn 与已交付父卡片；rewind 排除的节点不参与进度。每帧共享 256 条/约 8 ms 预算，显式下一记录保证目标边界至少交付一条。
- 来源时间 S 与播放时间 P 单调映射；已确认且严格超过 30 秒的空档缩为 1 秒 P，恢复空档单独标估算。显示时间按记录瞬间解析本机 offset，失败回退 UTC；缺时显示未知/估算。IDLE 提示依已交付 frontier 出现并保留。
- 专用面板替代 composer 和实时状态操作，详情只覆盖正文；响应式高度为 5/4/3/2/1，按 Unicode cell 宽度裁剪，热区与实际绘制控件一致。
- 面板沿用 `Theme` 的颜色与状态/快捷键视觉语义；没有直接嵌入 live `StatusBar` 或无鼠标热区的 `ShortcutsBar`，避免把执行动作带进只读页面。

## 自动验证

OpenSpec 归档为 `2026-09-30-interactive-readonly-replay`；归档前严格全量校验 15/15，通过后主规范合入 6 项新增、5 项修改要求。归档后严格全量校验 14/14；归档校验完成结果见本记录末尾。

| 命令 | 结果 |
| --- | --- |
| `cargo test --locked -p shell --lib session::storage::transcript -q` | 26 通过：来源/seed/rewind/恢复、Plan 等待、sideband 与后台任务活动 |
| `cargo test --locked -p pager --lib replay_cmd -q` | 39 通过：生产树调度、因果门控、嵌套导航、显式前进、IDLE、时区、面板、浏览及原有播放 parity |
| `cargo test --locked -p pager --lib transcript_projection -q` | 14 通过：跨 turn 工具归属、Goal/Behavior/Plan/Workflow、后台任务与晚到事件 |
| `cargo test --locked -p pager --lib export_cmd::tests -q` | 6 通过 |
| `cargo test --locked -p pager --lib views::block_viewer::tests -q` | 6 通过：普通与 Replay 共用的详情工厂及 raw/data 行为 |
| `cargo test --locked -p pager --lib scrollback::text_selection -q` | 102 通过：中文、跨行、边界和复制重建 |
| `cargo test --locked -p pager --lib scrollback::search -q` | 30 通过：查询 generation、可见内容索引与异步结果状态 |
| `cargo check --locked -p cli -q`、`cargo build --locked -p cli --bin grow -q` | 编译通过；移除共享 viewer 提取后遗留的未使用 import，测试链接时仅有 macOS linker unwind 警告 |

## 场景矩阵

| 场景 | 本次核对的证据 |
| --- | --- |
| V01–V04 普通详情、组、选区、搜索与焦点 | Browser 使用同一 ScrollbackPane/BlockViewerPane；buffer 鼠标测试区分展开组头与首成员；真实 PTY 鼠标点击打开详情，Tab/Enter、帮助、搜索粘贴、逐层 Esc、Ctrl-C 均操作过；拖动取消点击与 resize 保留稳定 entry 身份有测试，选区算法 102 项通过。 |
| V05–V08 子树、异常时间、来源失败与 rewind | 生产 `Replay` 测试 root→child→grandchild→返回、父卡片门控、child 时间早于 spawn 和不可达分支时长；shell 读取测试验证 seed、缺失子来源失败及 rewind 分支；真实旧格式来源在接管终端前明确报身份错误。 |
| V09–V15 IDLE、并行活动、恢复与元数据 | 30 秒严格阈值、反向映射、许多空档、高倍速、子节点执行保护、Plan 已证实等待、冷恢复边界、未终结活动、sideband/后台任务保护及 Goal 元数据穿过同一空档均有单元或生产树测试。普通工具无证明等待时保持保护，无法判定的长等待可显式前进。 |
| V16–V18 中断、Goal/Behavior/Plan/Workflow | 当前运行的 projection/replay 测试覆盖打断→追加输入→晚到旧 terminal、Goal pause/clear/replace/late usage、Plan rejection/confirmation、Workflow 退出模式后 Run 状态独立。播放器与历史业务状态分开。 |
| V19–V22 时区、积压、显式前进、窄屏 | offset、UTC fallback、DST 重复小时测试；生产树共享预算和暂停态显式前进测试；panel 尺寸类覆盖 120×40、80×24、60×15、40×8、零尺寸，buffer 检查深浅主题、标准及紧凑尺寸的控件热区，真实 PTY 做过 resize、F8、鼠标面板点击与退出。 |
| V23–V24 搜索旧结果与最终一致性 | 搜索对象归属于各 Browser，关闭即丢弃，后台结果只在所属活动页 poll；replay/export 当前运行的测试比较不同速度、暂停/调速后的最终投影。 |
| V25 只读隔离 | CLI 早分支及 Browser 调用路径审查；真实根会话 PTY 前后 `summary.json`、`timeline.jsonl`、`updates.jsonl` SHA-256 一致；隔离 `GROW_HOME` 复制真实会话并放入故意无效的 `config.toml`，Replay 仍进入 TUI 且 Ctrl-C 后退出码 0；不触发历史命令/审批。 |
| V26 正常调用方 | 共享 viewer 工厂 6 项与正常 Scrollback 选区 102 项当前运行通过。 |

## 真实 PTY 与资源

- 大会话 `01a0eaf7-3acf-7872-8084-f4432d1c57a2`：100×30、16× 与 1× 启动；接管前有阶段提示，接管后可退出。执行 F8、面板鼠标 press/release、帮助、搜索、bracketed paste、40×8 resize、`]` 和 Ctrl-C；粘贴后仍存活，退出码 0。一次 16× 冒烟历时约 16.45 秒，macOS `ru_maxrss` 为 512,720,896 字节（约 489 MiB）。上述三份根来源文件前后哈希相同。
- 子会话 `01a0eafb-6bc1-78f3-b1aa-ae5732d377c1`：Tab/Enter 打开详情，Esc 关闭；真实 SGR 鼠标在正文第 2 行打开详情，退出码 0。帮助、搜索粘贴、缩窗和 Ctrl-C 均经过真实 PTY。
- 含六个直接后代的另一真实根会话 `01a0ee47-1ad4-70f0-9c87-d0319a70d814`：120×30、4096× 完成后搜索已交付的 `Regenerate C4 checked profile`，退出搜索层，对对应卡片先展开组、再 Enter，底部路径出现子会话 `…b243dcd6`；Esc 返回根路径，Ctrl-C 退出码 0。子页历史状态与根页分别显示（子页 Behavior normal/Goal none，根页 Behavior goal/Goal active），说明导航没有混用状态。搜索结果层中的 Enter 专用于刷新已显示内容，需先 Esc 关闭才能打开卡片。
- 隔离配置探针：把根与子会话复制到临时 `GROW_HOME`，写入故意无效的模型配置，Replay 仍进入 TUI，Ctrl-C 后退出码 0；临时目录测试结束后移除。直接将 `sessions/` 设成符号链接会被来源 reader 拒绝，因此探针使用真实复制，未绕过来源校验。
- 一个较旧会话因 `update 2 has a different or missing session identity` 在接管终端前失败，符合损坏来源 fail-closed。`--speed 0`、`NaN`、`inf` 与过大数值在参数阶段拒绝；负数由 clap 作为无效参数拒绝。

PTY 是实际终端输入/输出，而 buffer 和生产树测试负责断言状态。增量终端绘制不总重写完整 `Paused` 字样，所以 F8/面板的精确状态转换由生产树测试断言；PTY 只证明对应控制序列可进入应用、不会导致意外退出。来源哈希是写入检测，不单独当成“没有执行”的证明。

## 旧数据与有意边界

旧记录不持久化所有人工等待的起止、精确进程离线时刻或逐 token cadence。普通工具持续时长因此保守保护，恢复空档明确标估算；用户可手动跨过未知等待。没有可靠末端的尾部不加虚构等待。固定树是各会话验证后读取的截点，不保证跨会话全局原子瞬间。历史外部文件、媒体和后台 stdout 缺少已捕获内容时不从当前工作区补取。以上是 delta 的降级契约，不是待补录的伪精确历史。

归档收尾：`openspec validate --all --strict --no-interactive` 为 14/14，`openspec validate --archived --no-interactive` 为 558/558，`git diff --check` 通过。PTY 用的本次 `target/debug/grow` 构建产物在测试后移除（约 510 MiB）；共享 `target/debug/deps` 和 `incremental` 未删除，以免影响工作区其他任务。未安装或替换用户 PATH 中的 `grow`。
