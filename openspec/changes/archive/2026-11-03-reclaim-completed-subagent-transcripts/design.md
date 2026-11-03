# 设计

保留现有 AgentView 作为 child 路由、控制代次、草稿和交互 owner，卸载可从历史重建的 scrollback、AcpUpdateTracker 的正文/工具索引及关联渲染资源。不另建轻量 session 类型，也不删除持久历史。

回收条件是父索引已确认 child finished、child idle、当前未选中、历史已加载，且父/child 没有进行中的 replay/reload。已打开的查看器或传输交互仍保有展示 owner 时不卸载。回收重置原 child_updates_replayed，保持 ACP/Grow highwater 和 reconnect 身份；fresh_continuation 继续保证 EntryId 不复用。现有模型、行为可用性、权限、输入草稿和子会话索引不重建。

共用事件循环每次处理完事件后回到循环顶端时执行回收；因此 terminal 到达、离开 child 及后台 parent 都能收敛，不依赖帧率或新轮询器。只在实际丢弃重状态后调用原有 allocator release hook。

重开沿 ensure_subagent_child_replayed 及现有 canonical history projector 恢复任务、工具和终态；加载失败不设置已加载标记、不追加阻挡再次回放的 footer，显示既有 notice 并允许下次打开重试。实时正文不能在历史读取失败后抢先阻止后续回放。canonical writer、runtime retirement 与权限裁决保持原所有者。

后台任务的 Started/Completed presentation facts 也必须进入 child history stream，复用原 live 展示 reducer 重建其行和 EntryId 链接。回放前已存在的 BgTaskState 是当前 stdout、状态和取消 owner，不能被历史 Started 快照重建或回退；canonical Completed 可将仍 Running 的同一 owner 单向推进到终态，已终态则保留当前状态/时间。纯冷加载缺失的状态沿原 reducer 建立，不启动进程、不重新发送通知。

验证按真实 Spawned/Finished、隔离的 canonical history fixture、回收 reducer 和 fullscreen open 入口覆盖重复任务、选中保留/离开回收、重开不重复、draft/control/高水位保留、失败后修复与晚到事件；随后做独立对抗性源码审查。不得以 RSS 波动代替对象释放验证。
