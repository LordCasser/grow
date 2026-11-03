# 决策

依据前序 `analyze-dashboard-ux-alignment`，以现有 AgentView 为唯一会话编辑 owner。按四个顺序增量实施，每轮基线和定向测试通过后再进入下一轮；不并行多个特性实现。

1. attached 详情只增加导航标识；PageUp/Down、Esc/Left、编辑、Plan/Question/Permission 和 child 下钻沿用普通 AgentView。清除没有生产入口的旧 Dashboard popup 路由，不保留兼容入口。minimal 的能力差异不变。
2. 总览选择仅改变只读预览。回复/待处理入口打开真实会话/交互，不拥有第二套 composer。新建保留明确的 New session 输入；搜索使用独立 query/state，不能清掉新建稿或其 undo/附件。
3. Ctrl+X 在列表与详情统一为配置前缀，目标明确；重复前缀只停止会话工作，空闲不关闭/删除。显式会话动作提供 Close session 和 Delete history，永久删除独立确认并绑定原目标；复用既有 close ACK、delete request binding 与 retirement/draft owner，不重写后端。
4. 详情 cycle 使用稳定的打开顺序，与 filter/grouping/活动时间无关；总览来源和活动分开呈现，单一键盘焦点/弱 hover。宽窄屏保留目标和主动作，辅助预览可收缩。

独立 review 在实现与实际路径验证完成后进行；reviewer 不参与实现，对照本轮前快照区分旧改动，并检查数据归属、交错回执、自定义键位、模式与最小终端。发现项在同一 change 闭环，不把未知架构债务扩大实施。

## 命令与删除确认的落点

- 注册表只有一个全局 SessionCommands 前缀绑定。Dashboard 的本地选中行先 attach 实际 AgentView，再转发同一个前缀事件；无明确本地目标时解释不可用。首键不直接停止或设置默认配置。
- AgentView 独占前缀窗口；重复前缀/X 停止当前 root/child 工作，C 经现有 CloseSession ACK 关闭，D 请求永久删除。
- 删除确认由 AppView 独占，捕获 session ID、cwd、标题与本地绑定 epoch。它不复用前缀 TTL，不依赖后续 selection；确认时复查绑定和本地活动，后端保留 canonical/writer 检查。完成回执沿既有 retirement 和 draft owner 处理，不强制跳走当前页面。
- full TUI 和 minimal 复用同一确认数据与 ModalWindow 绘制；minimal 仅为此 app modal 分配既有的 18 行 live region，不改变普通会话能力。


## 总览焦点与稳定顺序

- 默认 Open 按稳定会话身份排列，显式 pin/reorder 仍可改变总览位置；State/Directory 是可选视图，不影响详情 IndexMap 打开顺序。
- 来源投影从本地 row identity 和 roster resident 事实区分 This window / Child session / Other terminal / Saved history；未打开的空闲会话显示 Idle，不把未加载当成已终止。当前会话单独标记 current。
- 输入与列表各有唯一焦点：空新建稿的 Enter/箭头不操作背后选中项；单击只选中预览，Enter/双击打开。搜索 caret 使用真实绘制矩形和 grapheme 列宽定位，保留独立草稿。
- 导航取消离开视图的 root/child 前缀，包含鼠标和 picker 切换；child scrollback 的 Esc 在前缀激活时先取消前缀。
- 清除已无生产调用的旧 reply/peek allocator 与零高度兼容字段，只保留 normal composer/chrome 几何。80×24 保留 New session 标签、来源/状态、管理 guide；只读预览不足空间时收起。
