## Context

代码位于独立工作树，HEAD 为迁移起点。61 个 package 中 shell、pager、tools 体量显著大于其他包，需要模块级遍历；不能仅记录 crate 根模块或注释。

## Decisions

1. crate-inventory.json 从实际 Cargo.toml 枚举，保留 workspace membership、features、源码规模和核查状态。包数量以当前 manifest 为准，不把扫描结果等同核查完成。
2. reviews/<crate>.md 记录每包已读取入口、功能清单、条件、错误边界与证据。功能必须链接现有 requirement 或当前 delta；未映射功能列为待办。
3. spec 仍按用户可观察能力或明确工程契约组织，不机械为每个 crate 增加实体。测试、fuzz 和 vendored 包记录真实职责、运行条件与验证入口，不虚构产品特性。
4. 大包按照 public command/tool 注册、模块树、配置开关和平台条件建立覆盖清单，再逐模块阅读实现；每项功能以源码符号和可检验场景落入规范。
5. 归档前对 61 包完成状态、所有功能映射、来源存在/哈希、主规范一致性、链接及 OpenSpec 格式执行检查。未完成全部覆盖前不归档、不宣称目标完成。

## Risks / Trade-offs

全仓库规模大，工作跨轮持续；过程进度必须持久化。源码注释与实现冲突时以实现为准，发现的无关债务仅登记。平台测试、外部 Provider 和网络依赖无法由静态阅读证明，明确区分已确认代码路径和动态验证范围。
