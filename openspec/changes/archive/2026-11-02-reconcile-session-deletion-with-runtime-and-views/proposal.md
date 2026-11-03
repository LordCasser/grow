## Why

grow/session/delete 在资格检查前 teardown，存储拒绝后旧页仍绑定已消失的 actor；失败回执只有 toast。成功 after=Stay 又提前返回，跳过同 id AgentView 退休。两个结果都让 session 旅程与真实 runtime 不一致。

## What Changes

- 在现有 lifecycle transaction 内先做只读 canonical 删除资格检查，已知拒绝不取消/关闭 runtime；实际删除仍在 writer lease 下重新核对。
- 删除错误携带本请求是否使原 runtime binding 失效的明确事实。前端可证明未失效时保持原页；已失效或 transport unknown 时解除旧绑定并保留历史/草稿供显式重新打开，避免 resume dedup 聚焦死页。
- 成功公共收敛先退休所有同 id 本地页，再按 after 导航；Stay 保留无关页面/Picker，仅在当前页被删时选有效落点。
- 移除 Slash Delete 先行 Cancel/Kill effects，停止与 writer drain 统一归 backend 删除事务。

## Capabilities

Modified: `client-surfaces`。

## Impact

storage preflight、ACP delete lifecycle/error metadata、Pager effect/result 与导航/草稿边界。无新删除账本或自动重试。不隐式删除关联 session。
