# 首个实现增量：咨询问答的被动上下文回写

用户已授权 demo→修正规划→派发实现。本任务只实施消息投递内部的流程控制/上下文边界分层，以及采用该语义的 ask 回写。其他上下文变更由原机制管理。

## Goal and boundary

使 ask_parent、ask_subagent、ask_session 的成功问答在回答方下一获准请求可见，保留真实 Sideband/快照来源；不抢占、不主动唤醒 idle、不伪造用户权限、不向提问方重复注入工具结果。失败/拒绝/取消不生成成功上下文。外部 API 不向来源 Session 合成输入。

允许回改：`chat-state` 现有 Notification source/fold/验证和 Timeline 查询 plumbing；Shell inquiry 成功证据、相关 SessionActor 初始化、通知接纳/消费、interjection 最终回复候选边界、真实 turn admission/sampling 调用点；必要测试、开发者说明、本 change 记录。允许同一现有职责目录中的必要小 helper，不新建平台、总线或数据库。

本闭环必要触点超过六个源文件，主要来自 enum 穷尽匹配、查询 plumbing 和错误传播；保持单一功能，不为拆文件交付半语义。不得改独立 model/Behavior/compaction/rewind/fork 的行为。不可 reset/revert 其他正在进行的工作，发现同文件并行编辑须协调。

## Concrete decisions

- 复用 `SidebandRun::complete` 已返回的 `TimelineRangeRef`，内部保存 `{outcome, proof}`；不把 source proof 塞进公共工具回答文本，或修改询问 wire outcome 以便内部传值。
- 新 canonical 成功 proof 绑定原回答 target identity 和实际完成 result ref；原 Sideband manifest 拥有冻结来源。旧 audit 无证明不回填，fork 的他属证明不形成新同步义务；验证 proof/答案/owner 对应，不能由文案或当前 actor 推断。
- 新 Notification 来源为 context-only，使用原提问 session + inquiry identity 去重、原成功 terminal identity 作 source version；复用 Received/Consumed/immutable payload，不借用 ParentMessage/AgentReply，不另建 applied/outbox。
- 成功 terminal→Received 不是假事务。生成收据失败不把已提交 Answered 改成 Failed。原 writer startup、已有 outcome 重试及真实采样/最终候选边界协调缺口；只读观察不修复。
- Timeline 查询新增按 seq 后缀查询，复用现有 query plumbing；一个非权威的 actor cursor 初始化为0，只有捕获前沿内义务全协调完成后才单调推进。失败不推进，无每步全历史 clone/scan、无第二 pending ledger。注意 await 交错，推进不得跳过未完成义务；正确性来自 canonical Timeline 和收据幂等。
- bool 消费路径必须为新增义务保留错误到真实请求准入。可直接改为 Result 并同步调用方/测试，不保留吞错的生产 fallback。当前请求前未完成必需消息消费确认时禁止 dispatch。
- 先在相应执行 gate 检查当前 owner/预算/控制资格；无资格保留 pending。Consumed 后才失去资格时保留已提交 Surface，禁止新请求，不反向“撤回消费”。
- 最终回复决策在 gate 内固定精确 candidate IDs。只用这些候选决定是否续同 Turn；候选允许后继后，新到收据可在下一正常边界消费；不可逆 final-close 胜出才等待未来 Turn。不要新增 min_turn 路由，不在 gate 释放后重查所有 pending 冒充旧候选。
- 既有 steer 继续是 request 软抢占+同 Turn 输入。被动来源不进入 pending_interjections，不设 parent interrupt，不修改当前 stream/tool。

## Validation

先核对当前基线和相关现有测试；每次失败先分本改动/已有/环境原因。使用 deterministic actor/provider fixture 实测：真实询问完整 Q/A/来源；无 PermissionEvidence；blocked stream/tool 不被 inject 抢占；idle 无 Turn/请求；final candidate 三种交错；资格撤销；terminal/Received/Consumed 故障和 ACK 不明；live/冷恢复不重采样、不重复消费；已有 outcome 重试；fork/历史无回填。不能用手工拼出的中间状态替代全部真实入口验证。复用已有 provider、persistence、ContainedDirectory 测试工具。

最低 Tier 3，完成后由独立 reviewer 审查实际 diff、错误路径和证据。运行受影响 crate 的定向测试、最终相关 package 全量 lib tests、fmt/diff check 与 OpenSpec strict validation；具体测试范围以最终改动为准，未执行项如实记录。构建保持低并发与磁盘约束；若需 native 工具，使用本进程 PATH 前置 `/usr/bin:/bin:/usr/sbin:/sbin`（此机 PATH 内的 DevEco diff 会破坏 Autoconf）。不全局改环境、不安装或发布 release。

## Result handoff

报告实际变化、最终生产文件、实际命令/结果/失败、覆盖范围、未解决限制和 source 状态。实现完成只表示待独立代码 review，不能自行把当前设计/模型通过当运行时完成。仅在对应验证完成后勾任务；不要提前 archive。下一轮是否还需要增量由实际结果决定，不预建未来接口。
