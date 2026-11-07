# Complete nondeferred backlog

## Why

用户于2026-11-06要求“完成修复和处理，处理backlog的非延期项目”。这重新授权启动上一里程碑停止后尚未实施的非延期事项，并要求处理刚分析的child收尾问题。旧2026-11-05停止点继续作为历史，不再阻止本轮已授权项。

## What Changes

本change只记录实施范围、依赖与整体验收，skip_specs:true，不自行改变产品契约。每一项行为修复仍在自己的change先建delta/design/tasks，再交单一owner实施、审查和验证。原规范、旧实验证据与当前源码有差异时保留记录。

范围：backlog“本轮全仓审计发现，待独立修复”表、采样解码后续、官方跨协议对照、child收尾分析中的非延期项；明确点名的child SessionEnd顺序也在本轮范围，尽管旧文放在“明确暂缓”。其他明确暂缓事项继续保持原启动条件。依赖真实目标/硬件或用途证据的候选先满足本地可验证门槛，不凭一次静态分析实施新框架；无法满足时明确结果和剩余依赖，不伪造关闭。

不改变公开2.6.1tag/资产，不修改正常用户会话、全局安装或配置，不为调查接管未知live writer。当前工作区保持可审查状态；正式提交、共享tag或发布需要与用户本轮授权一致。

## 2026-11-07 继续实施

用户再次要求继续完成backlog非延期项目，并额外修复长上下文双Ctrl+D五秒退出超时、准备下一个2.6.x版本。当前公开latest与本地latest均为v2.6.1，候选准备见[2.6.2变更](../2026-11-07-prepare-release-v2-6-2/proposal.md)。原tasks第五项的“正式发布”是此前会话记录，不能据此将本轮“准备发布”扩大成共享发布操作；本轮先交付已验证候选。

父保留所有既有dirty修复，只新增逐项行为：AC-03b Shell/Pager来源与回包绑定、AC-06两入口配置刷新、AC-07更新结果采用、AC-08按Install/Uninstall/Update分别传播失败、MCP-F2注册生命周期、PB-03 Session及Service、PB-05 capture owner、PB-06地址归属，以及已列容量/性能/测试隔离与用途核对。退出窄查询见[长历史退出](../2026-11-07-avoid-exit-history-replay/design.md)。每项仍由对应模块单独交付，Cargo窗口串行；不把一项局部通过当整轮里程碑。

依赖真实目标或现场状态的条目只接受相应证据，明确延期的项目不启动。代码投影/机制实验和实际部署分别报告；未满足证据门槛时保留限制，不为关闭清单而编造兼容策略或删除失败输入。

## 2026-11-07 范围收拢

用户最新要求范围不扩散：只完成 backlog 中 P0/必要或已进行的修复与发布必要项，然后准备发布。上节“继续实施”的宽范围保留为历史，剩余实施以本节为准；未实施 change 与用户文件不删除。

- 本轮完成已进行项：Skills session/epoch 与 AC06 两入口、ACP Update、普通 Install 错误传播、SDK lifecycle 验收、crash identity 与已启动 PB06 最小 raw 限制、PTY Session，以及兑现该生命周期必需的 CLI/server 接线。
- 发布主线为长历史 Ctrl+D 退出超时：真实 291MiB 固定副本的测量、修复与 CLI 证据必须完成。
- 未启动项本轮不实施、保留 backlog：Uninstall、普通 Update 保存失败、Markdown 性能、Workflow 测试与 Agent 公共 API 用途清理。
- ACP 容量只收尾已启动的有界测量，不派生 policy 生产改造。
- 本轮目标为本地已验证的 2.6.2 候选与发布准备；未获新的外部发布授权，不 push、不建共享 tag、不发布。
- 父已接受 announcements 依赖并归档；crash-handler PB05 已实现并通过独立 review，CLI tests 待父确认，不据此把本轮新修复整体记为完成。
