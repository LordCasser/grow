## Decisions
复用现有值类型模块；工具定义与图片资格 key 放 tool-types，OriginClientInfo 与 ClientType 放 config-types。原模块只 re-export 同一类型，不复制定义。renderer cursor 接收调用方判定出的特殊行 index，所有 option identity 判断仍由 workspace 及 Pager 编排层负责。

VersionPolicy 的纯版本边界解析下沉 config，路径直接复用已有 config::grow_home/grow_application。CLI 从原 shell::util::config::load_config 读取 cli.auto_update 后传给更新器；CliConfig 仍由 shell/src/agent/config.rs 持有。channel 持久化由 CLI 调原 update_config 完成，显式 channel 切换意图继续传给安装决策；pinned 安装成功后才 await 调用方提供的 update_config future。没有新的写入器、写锁、缓存或配置类型，保留所有可写 section 的严格校验、未知字段、环境引用和原子写行为。

Core regression 在现有 workflow 中通过 cargo metadata 检查五条已消除的直接依赖边。它检查结构约束，不替代行为测试。

## Risks / Trade-offs
这只能消除列明的依赖边，不宣称整体构建速度提升。使用 cargo metadata 比较具体边，编译验证消费者；不复制 target，不运行全仓 release。构建串行、2 jobs，磁盘余量低于 20 GiB 暂停重构建并仅清理本轮新增缓存。
