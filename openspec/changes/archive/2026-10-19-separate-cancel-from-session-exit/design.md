## 不变量
- Ctrl+C 不生成 Quit、退出确认、树挂起或强退；输入框已有清理/选区等局部操作优先。活跃回合沿现有 CancelTurn 路径，取消中可再次请求取消；空闲执行会话不退出。
- Quit 默认 Ctrl+D，第一次仅以既有 PendingAction 一秒期限等待确认，保留草稿与回合。第二次同一键才请求挂起；过期或其他键打断须重新确认，不把 Ctrl+C 当作第二次。
- Welcome 也对默认退出键双按确认；显式菜单退出、OS quit、更新重启等既有入口仍沿各自准入进入相同挂起责任。既有 Ctrl+Q alias 可保留，但默认主键统一为 Ctrl+D。
- Quit 已在执行时，重复退出键不增加请求、不重置总期限或转成强退；未确认状态仍由 r/f 显式决定。

## 实现边界
删除 root 对单次 Ctrl+C 的全局退出拦截及回退，清除 agent 在 cancelling/minimal idle 时将 CancelTurn 升级成 Quit 的路径。复用现有 Quit registry、PendingAction 和 begin_exit_suspension，不新增手势状态。全局退出键在局部控件前确认，避免 composer、modal 和滚动键吞掉默认退出键。同步原真实进程挂起夹具，使用两个 Ctrl+D；另保留 Ctrl+C 取消且进程仍存活的反例。
