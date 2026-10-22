# Verification

类型清理源码基线 b02731ea；期间独立提交 1a7dcd77 同步旧 HookRegistry fixture。固定 Rust 1.93.1，独立 HOME/GROW_HOME，专用 target；不使用真实配置与 provider 凭据。命令、全部中间编译失败、机械 Box 补丁与最终源码哈希见 evidence，耗时包含 Cargo 锁等待，不作性能数据。

- fmt check、diff check、workspace all-targets check、独立默认 CLI check 通过，两个 check 的 compiler diagnostics warning/error 均为零。
- Clippy 完整扫描退出 0，剩余 58 处去重诊断：47 disallowed_methods、7 await_holding_lock、2 suspicious_open_options、1 drop_non_drop、1 可选 Tokio 配置路径不可达。原类型布局相关诊断已处理；四个有理由的局部 expect 见 review。不宣称全量 -D warnings 已通过，剩余项独立处理。
- 六个受影响库合计 15,022 passed、0 failed、35 ignored：ACP 16、ChatState 573、Pager 7,593、Shell 4,362、Tools 2,427、WorkspaceTypes 51。Tools 自身派生的两个子进程各执行一项测试，不重复计数。ignored 是既有基准/显式集成场景，不当作执行通过。
- 首次组合测试退出 101：前五库均通过，最后 WorkspaceTypes 为 50 passed/1 failed，原 HookRegistry fixture 缺既有必填 on_failure。未更改生产 DTO、默认值或删断言；独立 change refresh-hook-registry-wire-fixture 补齐样本并复跑该库 51/51。失败原始日志保留，本项没有把首次组合命令记为成功。
- 回归覆盖 Timeline replay/校验/持久化/输入图片清单，根与子会话恢复、admission 和取消，Workflow、工具错误，Pager 各类内容/图片/复制/历史布局和草稿。未改 replay 阶段边界、历史排序、就绪时机、存储版本、序列化字段或摘要算法。

标准 Box 增加被间接承载变体的一次分配，错误 Box 仅在错误分支产生；换取消息槽与 future 尺寸下降，不承诺 resume 速度提升。临时 journal 值与高频草稿快照保留内联以免无收益分配。专用 target 继续供下一项质量清理复用，全部完成后删除，不动共享构建缓存与已保留候选。
