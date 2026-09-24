# Design

测试构建下由 worker 在线程退出接收循环后设置原子信号；测试持有该信号，Drop daemon 后在有界期限内观察退出。生产构建不包含字段或信号写入。

本测试覆盖空闲 daemon 收到 Drop stop 后退出的情况。它不模拟正在执行的大型 nucleo 匹配、UTF-32 转换或不可中断文件系统 syscall，因此不提供这些操作的退出时限。Drop 不等待阻塞操作的性质仍由既有测试覆盖。进程隔离不是本次目标：目前没有产品级硬截止期限需求，OS syscall 也无法由进程内协作取消强制中断。

`.openspec.yaml` 使用 `skip_specs: true`，因为改动仅为测试观测性，不改变契约。

## Backlog decision

关闭既有搜索 worker 泛化资源条目：history 大 corpus 与 fuzzy 空闲 worker 的退出均已直接观察，现有阻塞测试确认 Drop 不等待工作中的操作，队列已按最新请求合并。一个已经进入的 nucleo 调用或 OS 文件系统 syscall 不能由进程内 stop 标记强制满足 wall-clock 期限；目前没有产品契约要求该期限，也没有可复现的超时故障。为此新增 worker 进程隔离会扩大进程协议和索引状态所有权，收益没有证据支持。大 corpus/目录索引仍随用户数据规模增长，这是搜索能力的正常成本；未来若出现具体规模、延迟或内存故障，再以该 workload 建立新的有界验收，不保留没有阈值的开放债务。
