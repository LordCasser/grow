## Decision

沿现有 validation outcome 增加父策略拒绝事实。先确认定义存在，再判定全局 toggle 和父 filter；候选名称只显示当前允许的完整 ID。工具文案和真正派发共用错误格式。

全局 toggle 以共享当前快照供 coordinator、在途 spawn context 和会话重建读取。watcher 只比较 toggle，合法新表或删除表才发布，错误类型保留旧策略。daemon 与 in-process Pager 都通过同一内部 reload 扩展通知会话。既有模型 reload 的同一 Step 控制队列负责目录刷新，空闲会话直接排空，忙会话在下一安全边界应用；不另建调度器或权限层。

Agent 重建记录用于构建的 toggle 快照。领取控制前若候选已过期，通过原 preparation worker 重建，文件发现不占取消控制锁；已领取事务期间到来的更新保留在下一安全边界处理。同名 Agent 的目录变动也必须实际发布，新会话在注册前核对构建快照以补齐启动间隙。Workflow 的冻结 Agent 集合继续使用原 Run 契约。此变更不取消运行中子任务，不热更新 enabled、max_depth 或 permission_mode。

## Verification

覆盖 toggle=true 而父名单拒绝、未知短名/完整路径、显式 false、空允许目录的关闭/恢复、开关删除、非法 toggle、主/子会话传播、busy Step 与准备中的 Agent 替换。个人文件读回确认四项委派名单。对未覆盖的真实终端/线上模型验证明确记录，不以单侧文案测试替代跨层测试。
