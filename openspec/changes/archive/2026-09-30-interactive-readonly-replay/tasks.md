## 0. 本轮调查与交接（只做文档）

- [x] 0.1 核对 OpenSpec 主契约、已归档 replay/中断/Behavior 变更及当前调用方。
- [x] 0.2 按用户指定运行 ui-ux-pro-max 设计系统和 UX 检索，形成 TUI 适配、布局与焦点规则。
- [x] 0.3 调查固定树、详情/选区、活动时间依据和缺失历史边界，形成 audit/验证矩阵。
- [x] 0.4 标注保留代码的完成度与已知阻塞，保持功能实施和验收任务未完成。
- [x] 0.5 严格校验本 change 和全量 OpenSpec，记录文档检查后停止，交给用户安排实现。

## 1. 接手与来源准备

- [x] 1.1 按 handoff 清点工作树，处理 `mod activity` 缺文件的已知构建阻塞；不覆盖其他未提交工作，不把草稿视为实现完成。
- [x] 1.2 在同一 TranscriptSnapshot 提取所需时间依据、owner spawn 锚点、sideband/后台活动；保留现有全树来源预算与只读边界。
- [x] 1.3 明确捕获树与有效可导航播放集合，处理 rewind 分支、child ID 直接播放和缺失/坏子来源（V06–V08）。
- [x] 1.4 准备期阶段提示、一次读取与释放中间副本；核对内存放大和默认 Ctrl-C 行为。

## 2. 全树时钟与 IDLE

- [x] 2.1 实现全树共同 frontier、每帧共享预算、同时间公平推进和父子因果门控；生产调度器直接受测（V05、V20）。
- [x] 2.2 活动区间分类与全树并集：模型、普通工具、Hook、Plan 等待、sideband、后台任务及未知 Workflow 状态（V09–V12、V15、V18）。
- [x] 2.3 分离 S/P 时间轴，>30s→1s 的压缩映射、duration 提示、暂停/换速和元数据保序（V09、V15）。
- [x] 2.4 中断恢复空档估算、未知/无末端来源降级；保留原 outcome，不猜测所有权限等待（V13–V14）。
- [x] 2.5 显式下一记录、paused 前进仍 paused、finished 禁用；保留语义边界/reveal 和最终 parity（V16、V21、V24）。

## 3. 被动浏览与正常 UI 复用

- [x] 3.1 审核并验证已保留的 viewer 工厂/后处理，保证正常 AgentView、BgTask 和 raw/data/copy 不回归（V01、V26）。
- [x] 3.2 补全正文拖选、稳定 entry ID、selection overlay、组头/首成员命中、点击失效和 copy 错误反馈（V02–V03）。
- [x] 3.3 搜索仅基于已投影内容；匹配 owner/query generation；paste、详情、Esc/q/F8/Ctrl-C 焦点优先级（V04、V23）。
- [x] 3.4 子页路径栈、future gate、各节点滚动/折叠/选中保留；缺历史输出明确说明（V01、V05–V08）。

## 4. Replay 面板与本地时间

- [x] 4.1 按 ux.md 实现纯 Replay layout，5/4/3/2/1 行降级、Unicode cell 宽度与 resize 失效（V22）。
- [x] 4.2 沿用 Theme 与状态/快捷键视觉语义，按焦点生成真实 hints、按钮 rect 与只读帮助；移除旧两行长字符串和重复顶栏。
- [x] 4.3 Playing/Paused/Finished、全局进度、追赶记录、历史时间、IDLE/估算提示、跟随状态按既定优先级显示（V17–V22）。
- [x] 4.4 核对 local offset resolver、UTC 回退、非法时间与 DST；调度和原始时间不受格式影响（V19）。

## 5. 综合验收与收尾（实现 agent 执行）

- [x] 5.1 跑 shell transcript、pager projection/replay 全子模块、export、block_viewer 与正常调用方的针对性回归；记录命令和结果。
- [x] 5.2 合成来源矩阵覆盖 V01–V26，尤其用户打断→追加输入→晚到 terminal、Goal 切换/退出、Plan/Workflow 的组合，不删失败场景。
- [x] 5.3 CLI 编译后运行真实 PTY：鼠标、详情、嵌套子页、paste、resize、多倍速、暂停、下一记录、终端恢复；核对深浅主题和无鼠标浏览。
- [x] 5.4 记录全树高负载下调度响应与峰值资源；结构/探针/文件哈希联合验证无执行通道、无源写入，不用哈希代替隔离证明。
- [x] 5.5 更新 `pager/docs/user-guide/17-sessions.md`、`docs/development.md` 的实现说明与主规范链接；说明 IDLE 默认值、未知等待与快捷键。
- [x] 5.6 完成 verification 记录、逐场景验收后才勾选任务；严格校验、archive 本 change，再全量及 archived 校验。只清理本次专属 Cargo 产物，不删共享 target。
