# 用户旅程验收映射

对应前次 [50 条触发路径审计](../2026-11-03-analyze-image-user-journey-triggers/analysis.md)。这里区分真实进程/终端入口与故障注入：ACP 测试启动 fresh grow，通过 JSON-RPC 和 loopback provider 工作；PTY 测试操作真实终端；actor 测试驱动真实 handler/run loop、HTTP 与 durable JSONL，但不冒称剪贴板或物理焦点验证。所有新执行结果集中记录于 verification.md。

| 旅程 | 修复后的行为与验收入口 |
| --- | --- |
| J01 | 普通文本仍沿原采样流程；真实 ACP foreground/flush 对照，recap builder 回归。 |
| J02 | typed ACP 图片经实际接纳进入视觉请求；PTY 用隔离 ACP 建会话后继续回放原图，不操作宿主剪贴板。 |
| J03 | 实际 read_file 读取隔离 PNG，工具结果携图进入视觉主请求，再由纯文本 Sideband 恢复。 |
| J04–J05 | `text_route_rejection_projects_image_and_survives_process_reload` 验证模型切换、明确拒图、视觉描述、无图重试；主路径 image_input_recovery_tests 验证已知拒图 preflight。 |
| J06–J10 | 既有主恢复 HTTP/实际 OCR 子进程测试覆盖描述成功、缺失/失败、OCR、typed removal、durable ACK 失败及取消清理；共享生成阶段提取后重跑。辅助失败不能使用主路径 removal。 |
| J11–J13 | 主/辅助已有描述选择、切回视觉路由和 typed removal 的原证据保留沿原契约；既有 image_input_recovery/cold-restore 测试回归。 |
| J14–J15 | 能力始终绑定 prepared model/backend/endpoint/query；跨 provider flush 进程测试断言 endpoint、认证、实际 request model 与 backend。别名不可解析不回退。 |
| J16 | 真实 `grow/btw` 请求与 actor overload 重试保持冻结来源；已有描述直接复用，未描述路径按下面 J29–J32 恢复。 |
| J17–J19 | parent/child 与 peer 共享 receiver inquiry handler；真实 `grow/coordination/ask` 经 source/target 会话到 provider，actor 覆盖方向、等待、取消与模型准备快照。 |
| J20 | Full/Minimal `/recap` PTY 到 extension、通知展示；三 backend × 用户/工具图片的 HTTP 矩阵保持 backend reasoning 和完整证据。 |
| J21 | Full/Minimal 真实 CSI FocusLost/Gained，三轮图片会话切换纯文本路由，实际等待 181 秒越过生产阈值，再检查屏幕、canonical Sideband Result/Completed 及无主 ImageProjection。 |
| J22 | Full/Minimal `/flush` 到 host executor、provider 与真实 Markdown；busy slash actor 回归保持 foreground、队列和主历史。 |
| J23 | 真实进程持有 foreground HTTP 的同时 flush 独立完成并写记忆，释放后 foreground 正常结束；与 slash 统一准入，不再经过普通 busy 拒绝。 |
| J24 | `idle_memory_flush_tests` 从真实 run_session timer 到 HTTP：只读查询不延后空闲、前台/活动忙不请求、完整空闲间隔后触发、0 秒禁用。 |
| J25–J27 | 三个真实压缩入口的 memory-off 门禁回归分别走 run_compact、background boundary、overflow recovery；正常压缩+preflush 快照组合单列 compact_flush_journey。 |
| J28 | 主图片转换与 durable ACK 等待、失败和 session/inquiry cancellation；provider 回答不持投影锁，失败写入的 fail-stop 不能被缓存绕过。 |
| J29 | 已知拒图、选中窗口仍含原图：在准入前中立描述/OCR；不可描述明确失败，不丢图、不发非法回答请求。48 组合 HTTP 矩阵和 missing-description 测试。 |
| J30 | 未知路由第一次明确拒图：记录该实际路由，恢复冻结图片，最多一次文字重发；四个真实 ACP 辅助入口与三 backend HTTP 矩阵。 |
| J31 | 重问同一来源可复用 neutral description，已知能力避免再次裸图请求；普通 400 不学习能力、不调用恢复。并发输入不能混入旧回答。 |
| J32 | 视觉 primary 从不拒图、显式纯文本 flush_model：完整跨 provider 解析，拒图后只恢复选中图片，最终文字结果落实际 memory 文件。 |
| J33 | 预算外旧图不触发描述；同 URL/同文本重复图片也按最终保留的来源顺序选择，不能描述已省略旧 turn。描述展开仍服从原 consumer 输入上限。 |
| J34 | 主路径混合描述/removal 原子提交和辅助 tool evidence 矩阵回归；辅助恢复自身始终禁止 removal。 |
| J35–J36 | 旧 D/U snapshot 之后新输入、实际压缩替换：flush 保留原 input range/revision/IDs；真实 JSONL 重载核对两次 attempt 的来源，Markdown 仅含旧证据。 |
| J37–J39 | 后台未提升失败保留既有规则；提升后 1210→恢复→新压缩事务由 promoted_image 验证；transport 切换的旧拒图不标记新路由。 |
| J40–J42 | completed description、已确认描述/OCR、无当前辅助配置和 legacy/future/wrong provenance 的 cold-restore 回归；缓存只复用较早 revision 的同一不可变 source/content/prompt。 |
| J43 | 真实进程 restart/load 同一已知 transport 后使用描述；fork 的新能力域仍允许首次乐观原图，明确拒图后复用持久描述而不重做视觉调用。rewind 后依同一域/未知域规则选择表示；idle run loop 的真实 Rewind command 重置成功水位基线。 |
| J44 | receiver 关闭、询问取消与 session Sideband cancellation 分别终止等待/描述/回答；held description attempt 在取消后不 dispatch，写锁释放。 |
| J45 | `/memory off` 前后 snapshot wait、durable attempt ACK、已发出的 HTTP response 以及三个压缩触发点：无后续 flush 请求、无 storage 复活或 memory 文件；Sideband 按真实执行阶段终结。 |
| J46 | provider 失败/锁冲突不推进成功水位，同一历史下个完整空闲周期可重试；NO_REPLY/重复是成功 checkpoint，ACP 失败不再返回空成功回执。 |
| J47 | btw、flush、手动 compact 的真实 extension dispatch 等待已有 session/load 屏障；load 完成后才发 SessionCommand，关闭 channel 明确错误。cold-load 自动 recap 另修复原 0 idle clock：从已验证主 Request 恢复，再做真实 focus 验证。 |
| J48 | provider 成功与 memory 写入分别观察；ACP 错误/锁冲突返回失败，written 与 nothing-to-store 由完成通知 result/path 区分，测试核对实际文件。shutdown metadata 不冒称 LLM flush。 |
| J49 | 真实 headless ACP prompt `/flush` 经 input admission 与 builtin 执行到新的 memory Markdown，区别于 `grow/commands/execute` 的接纳回执。 |
| J50 | 普通 prompt 中的 `/btw`、`/recap` 仍不是相同 Shell builtin；真实辅助验证调用各自 extension，PTY 验证 Pager 命令路由，不混淆入口。 |

不把宿主系统剪贴板实现、外部 provider 在线可用性或全部终端模拟器的物理焦点行为算入本轮验证。三分钟焦点测试使用终端生产事件、生产门槛和隔离的本地 provider，没有新增产品测试开关。
