# 设计

复用现有command-only实现，仅替换公开ID、工具类型及当前调用方引用。保留task_ids/timeout_ms与BackgroundTaskAction语义，描述继续明确background terminal command；不迁移模块、不新增包装层、别名或配置项。

使用已有真实Tool::run、registry和preset测试验证命令输出与子任务域隔离；Pager既有等待测试核对新名称，旧长名不得作为可调用入口。历史archive只作追溯，不全局重写。
