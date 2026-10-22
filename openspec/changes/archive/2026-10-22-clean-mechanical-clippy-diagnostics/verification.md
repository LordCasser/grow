# Verification

基线为格式归档提交 `c71b0baa`；沿用 `clean-rust-build-diagnostics` 的独立 HOME/GROW_HOME、固定 Rust 1.93.1 与专用 target。source-hashes 记录本项最终源码；完整命令、所有中间失败及最后通过日志在 evidence。编译耗时包含 Cargo 锁等待，不用作性能基准。

## 最终结果

- `cargo fmt --all -- --check`、`git diff --check` 通过。
- workspace all-targets check 与独立默认 CLI check 通过；compiler diagnostics、非 JSON build stderr 中 warning/error 均为零。
- Clippy 全量扫描能够完成、退出 0，仍有 104 处去重 warning：47 disallowed_methods、20 large_enum_variant、18 type_complexity、8 result_large_err、7 await_holding_lock、2 suspicious_open_options、1 drop_non_drop、1 配置路径可达性。留待后续独立 change；本项未声称 `-D warnings` 已通过。初始基线因 lint error 中断了部分目标，故不能把初始 379 与本次 104 简单解释为精确删除数量。
- 9 组定向测试共 236 passed、0 failed、1 ignored（既有投影基准），含 Planning、PTY 字符/样式提取、WebFetch、HTTP hooks、response projection、Pager transcript/sticky/replay layout、Messages 流式协议。逐组见 tests.json。
- Messages 首次用了错误模块过滤器，运行 0 项，不计入验证；从实际源码模块确认后，复用刚编译的同一 test binary，以 `stream::messages::tests` 运行 29 项通过。保留两份输出。

## 对抗性复核与代价

只做等价表达式、借用、测试初始化和声明位置整理。三个大型文件的测试模块相对移动已逐模块比对：测试、断言及规范化内容无丢失，无 macro_rules 作用域变化。Option unwrap 改写保留光标替换与零宽字符处理；22 处测试初始化保留后续 map 插入、嵌套修改和 clone 消费点。默认 FIFO、toolset 字段默认值及权限值不变。

Clippy 自动修复并不总正确：enum_delegate 捕获默认 trait body，建议 `$crate` 会破坏原 trait 语法，按文件已有约定改为导入函数名；Range 单元素数组建议会误变成整数集合，改为显式 Range 切片比较。自动失败被回滚，未使用 broken-code。response projection 的迭代建议会遗漏第 n 个 tail 插入点，因此改为逐条迭代后显式追加尾部，保留空历史与原顺序，19 项投影测试通过。测试 mut 遗漏、Default 的 cfg 绑定/重复实现问题被全目标检查拦下，已修正并保留失败日志；最终测试专用 LeaderLock 构造器 cfg 不变，Default 统一委托 new。

未引入全局 allow 或关闭诊断；未改协议/持久化结构，未实施布局与资源生命周期重组。较大 diff 主要来自测试模块搬移。专用 target 约 11 GiB，继续供下一项复用，质量清理全部完成后删除；不动共享构建缓存和 release 候选。

严格校验：归档前 17/17；归档后全量 16/16、已归档 695/695，全部通过。
