# Prepare Grow 2.6.2

## Why

2026-11-07 用户要求继续完成 backlog 中非延期项目，额外修复长上下文双 Ctrl+D 五秒退出超时，并准备下一个 2.6.x 版本。当前最新本地 tag 为 v2.6.1，workspace 为 2.6.1，因此候选为 2.6.2。

## What Changes

保留当前工作区中上一轮已验收修复；逐项完成仍开放且满足启动门槛的非延期项。退出修复和各项行为分别建立 change，此 change 仅记录版本、发布说明、最终源码身份和门禁，不改变产品契约，故 skip_specs:true。

本轮完成本地可审查候选和发布准备。当前用户措辞为“准备发布”，不据历史文档中另一会话的发布授权推断本轮可以 push、创建共享 tag 或发布 Release。正式发布前先交付已验证候选；仓库/用户明确延期和需要真实目标证据的条件继续保留，不把未证实问题标为已修复。

### 2026-11-07 范围收拢

用户最新要求不扩散，本节收拢上文“逐项完成仍开放且满足启动门槛的非延期项”：候选只收已进行完成项与发布必要项。随本版本收尾：Skills session/epoch 与 AC06 两入口、ACP Update、普通 Install 错误传播、SDK lifecycle 验收、crash identity 与已启动 PB06 最小 raw 限制、PTY Session 及必要 CLI/server 接线。发布主线为长历史 Ctrl+D 退出超时，真实 291MiB 固定副本的测量、修复与 CLI 证据完成前不出候选。Uninstall、普通 Update 保存失败、Markdown 性能、Workflow 测试/Agent 公共 API 用途清理未启动，保留 backlog，不进入本版本。announcements 依赖由父接受并归档；crash-handler PB05 已实现并通过独立 review，CLI tests 待父确认，不据此把新修复整体记为完成。本轮交付本地已验证候选与发布准备，未获新的外部发布授权，不 push、不建共享 tag、不发布。

## Impact

根 Cargo.toml、继承 workspace version 的本地 lock records、2.6.2 changelog 和本目录验证记录。依赖版本、官方发布 workflow 与既有门禁保持；只有本轮证据揭示实际失败时另行修复。
