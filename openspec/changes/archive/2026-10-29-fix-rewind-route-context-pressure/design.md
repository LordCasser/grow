## Decision

provider 观察值只校准原 branch/route 的请求。rewind 开启新分支，ReplaceSamplingRoute 开启新路由；两者都使该观察值失去适用范围，应与冷恢复一样，以新 lane 实际可回放的 portable history 初始化。raw Surface 仍保留作后续 signed mutation baseline；portable input 和 projected pressure 使用相同估值。same-route reset 保留的已学习 portable reasoning 规则必须参与估算。

将已有冷初始化代码提取为 ChatState 内部方法，在 constructor、rewind durable ACK 后及 route replacement 后复用。rewind 不先发布 raw delta 的中间计数；ConversationReset 后只发最终 ContextPressureUpdated。普通 compaction/repair 的 signed delta 行为保持，普通采样参数更新保留 provider 锚点。

## Verification

在生产 actor 入口构造跨压缩 rewind：原始 reasoning 足以越窗，但 portable 正文很小；核对 rewind 前后的 Surface 和请求估值，切换三种 backend 后重复构建请求。另测真实正文超窗、route replacement 的即时通知、参数更新保留锚点及失败 rewind 不变。运行 chat-state 全部 library 回归和 OpenSpec 严格校验。
