复用现有 ActionRegistry 与 Dashboard navigation actions；replay gate 内识别导航动作和已经绘制的 overlay frame hit，不派发任何 backend admission。普通文字、粘贴、滚动、Ctrl+X、Enter 维持 replay 冻结；全局 Quit 的已有挂起优先级不变。导航不会取消 replay 或退休 owner。

SessionLoadFailed 从 request 的 load_return_view 读取原返回点，但只在 failed view 仍 foreground 时恢复该位置；统一使用现有 retire_agent_view checkpoint/remove 边界清理路由/子页/草稿。后台失败不抢焦点；成功回执继续只更新所属页。Tier 2：真实 load dispatch -> AppView key/mouse -> navigation dispatch -> success/failure。
