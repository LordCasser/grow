# Slash 命令反馈

一次用户操作只保留一个结果出口。状态、进度和结果不是同一种信息，不能互相推导。

| 信息 | 归属 | 展示 |
| --- | --- | --- |
| Goal 等领域状态快照 | 领域状态投影 | 更新面板，不生成操作记录 |
| 排队、执行中 | transient `UiNotice`，Progress tone | live status 原位更新，不写 Timeline |
| 命令结果 | 持久化 `UiNotice`，Command category | 一条摘要；原因、下一步默认可见，目录说明和诊断详情折叠 |
| 自主生命周期变化 | 后台提交后的 Lifecycle notice | 独立持久化，例如预算耗尽；不能被普通命令上下文吞掉 |
| 查询结果 | 对应查询的内容或视图 | 展示查询内容，不额外加“查询成功” |

`invocation_id` 标识用户调用，`event_id` 标识不可变事件。不能用文案或当前 Behavior 去重，也不能把相同 correlation 的不同事实全部删掉。TUI 仅用调用 ID 判断晚到的 RPC 错误或进度是否已被后台结果取代。重复执行相同命令仍有不同调用 ID。

交互式 `/export` 和 `/copy` 的显式文件任务在派发时冻结活动视图的内容、cwd、Agent ID 与 session ID。后台写入完成后，状态反馈按这两个身份查找原根视图或子 Agent 视图；切换当前视图不会转移反馈，原视图移除或重绑后也不会向新会话显示旧结果。任务队列仍按提交顺序继续。行为契约见 [子视图文件反馈](../../openspec/specs/client-surfaces/spec.md#requirement-child-transcript-file-notices-retain-the-export-origin)。

Minimal 的 `/transcript` 也在请求时冻结根或子视图及 session 身份。逐帧渲染始终读取该视图的条目、cwd 和媒体路径；切换焦点不改变来源，原视图重连则从最终正文重建，移除或重绑则放弃旧构建。分页器失败反馈仍由请求时的视图接收。行为契约见 [Minimal transcript 归属](../../openspec/specs/client-surfaces/spec.md#requirement-minimal-transcripts-retain-the-selected-view-owner)。

`grow/commands/execute` 的 `accepted` 只表示受理。排队的 Goal 修改保存原调用身份，完成、拒绝、clear 和正常关闭都沿原身份结算。进度不进入原生终端历史，因此 Minimal 不需要重写已经提交的行。

手动压缩的成功、失败、取消由 Shell 发布，RPC 不再复制终态；手动成功携带本次耗时和 token 数，立即展示。自动压缩仍可等待本轮模型确认 token 数，不能让手动操作遗留到下一轮。

Memory browser 只响应当前 live invocation。失败不发送空列表；结果是 transient，不参与历史回放。旧请求、重复结果、重连、Esc，以及已有其他模态窗口时，不能突然打开浏览器。

断线不是“执行失败”的证据。没有后台终态时提示结果未知，不自动重试变更；已有终态则忽略晚到的 RPC 错误。重连只恢复事实，不合成新的操作确认。

## 后续独立处理

插件 registry 的本地副本刷新和 hooks/MCP/skills 加载仍有仅写日志的内部警告，现有返回值主要是计数，无法完整表达组件级部分成功。本次合并 add/remove 的输出，并明确提示无 registry 时“路径已改，但需新会话生效”；加载器的结构化诊断应在插件模块单独收敛，不借 UI 去解析日志或推测状态。

进度不持久化。若需要重连后立即恢复排队 Goal 控制的进度，应从 actor 的在线队列生成当前快照，不能回放旧进度当作运行证明。
