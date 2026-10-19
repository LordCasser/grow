## Ownership at delivery

已有 ConfigWatchState 增加启动时永久目录集合（grow_home 和 extra_paths 的父目录），不增加新服务或注册协议。逐路径逻辑由现有锁内的私有方法处理：.grow 目录补挂依然要求活跃项目 root；config.toml 只接受全局/显式永久目录或仍被项目 root 引用的 .grow。无所有者的迟到事件丢弃；已经进入接收队列的事件不追溯撤回。

unwatch 释放最后一个项目 root 时只移除无永久所有者的目录 watch。共享祖先的已有 project_roots 引用检查保留，重复 unwatch 仍幂等。复用现有 paths_equal/dirs_contain 处理路径别名。静态 extra 目录晚创建或重建的发现能力不在此扩展。

## Tests

生产逐路径处理方法在实际 watch/unwatch 前后接受同一排队路径，确定性验证迟到 config.toml 与 .grow 不发布；真实 OS 用例验证额外配置目录和全局配置在项目关闭后继续工作。原跨目录共享祖先、别名、晚创建/删除重建继续跑。已观察的 hosted 断言并未记录收到事件的具体路径，记录源码可证明的缺口与最终验证，不声称看到了未采集的 OS 事件历史。
