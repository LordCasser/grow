# 设计

复用现有 ChannelBackend 和 durable reference reader。Shell 为 ACP 和模型绑定同一来源查询，返回带谱系的 inspection，snapshot 由同一事实投影；ACP 非阻塞只需一次 inspection，阻塞在同一最多30秒控制预算内继续 query；通道关闭也可读取原期限内可验证的 durable 终态，不能以失联推断运行或死亡。prompt_status 只验证 canonical reference 和精确 Spawn/Seed/Result/通知链接，不要求加载无关采样正文；诊断维护的读额度保持原职责。

get_task_output 从公共入口建立一个绝对期限：poll 五秒，显式等待 min(timeout_ms, 现有 cap)，涵盖 terminal 探测、批量识别、backend 等待及结果读取。各 ID 共享期限，不逐项重开预算。等待复用 backend 的分段事件等待，Running 返回继续等；保存每项已确认快照，不在期限后启动新一轮查询。等待错误保持观察未知，正文按真实耗时和原因生成。各项 future 由原调用直接 join_all 持有，无额外 spawned 等待任务。保留取消时 terminal block_waited 和 completion_delivery.defer_wait 所有权。

公共 TaskOutputResult 的终态规则供单项、列表和完成摘要共用；timed_out、output_unavailable 是执行终态，observation_* 是观察未知。中断等待对全部 ID 返回 observation_interrupted，既不伪造 Running 也不丢身份。select 中正常工具结果优先与通知收据转移保持。

缺少 progress 的 active child 用显式缺样本状态返回，身份和活跃责任已知，计数未知；list_running 保留原 active 范围、授权过滤和正常项，不扩成全任务清单。sampler 的同一 child token 覆盖 signals snapshot await，取消释放其 owner，无新超时/缓存。

按增量实施并验证：真实等待/期限与终态、查询入口、列表、中断、采样。使用公共入口、真实 coordinator、虚拟时钟和 canonical fixtures 验证成功/失败/取消/越权；构建复用共享 target，不删除其他任务产物。

主模型按需发现复用 get_task_output：仅显式空 task_ids 且非等待时调用绑定 source 的 list_running，返回 list_running 模式和已有 TaskOutputResult。保持 active-only，不将遗漏等同已清理；无 backend 的 terminal-only 入口继续要求非空ID，不枚举共享 Terminal.list_tasks，以免跨parent/child泄漏命令。无直接路由的拒绝说明无法证明任务终态，不放开直接父级消息权限。没有新产品工具、后台roster缓存或周期注入。
