# Evidence
slash/commands/export.rs::list_path_completions调用read_dir后filter_map错误、跳过点开头名称，只有items.push后检查items.len>=1000。全隐藏目录不会触发break。suggest_args是同步UI调用，注释“local directory listing is sub-millisecond”不是可保证的事实。

# Design
将真实目录迭代器传给私有候选收集函数，在filter_map和隐藏过滤之前take(1000)，不引入异步框架或可配置预算。保留符号链接目录检测、typed_prefix、目录优先排序、100项输出。测试可控迭代器反复产出隐藏DirEntry/Err并计数，证明不取第1001项；真实临时目录覆盖普通文件/目录/前缀和输出上限。文件系统单次next/metadata仍可能慢，不声称墙钟预算。

# Adjacent audit
/debug所有构建注册，release仅不列入补全；scroll/fps/log都有真实动作与消费者。/scroll-debug是独立注册但等价于/debug scroll的hidden alias，已列R22，待用户确认，不在本项删除。scroll recorder增长/慢盘债务已在backlog登记，本项不重复实施。
