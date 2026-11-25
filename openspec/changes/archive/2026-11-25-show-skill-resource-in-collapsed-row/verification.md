# 验证记录

Pager 定向回归从 ACP 调用构造真实折叠行，检查 80 列下的单行文本。加载主说明显示技能名；先后加载 `references/handoffs.md` 和 `examples/handoffs.md` 时，两行各有对应相对路径。给起始调用合入只带终态的更新，再按历史中已合并的调用形式重绘，资源名和 `Loaded` 同时可见。原有结构化结果用例继续核对带参数的主说明、展开的绝对来源及已准备正文。

仓库锁定工具链下运行 `cargo test --locked -p pager --lib skill -- --test-threads=4`，85 项通过，零失败，零忽略。首次编译时修正了测试夹具构造 ACP 元数据映射的写法。

`cargo fmt --all -- --check`、`git diff --check` 和归档前 `openspec validate --all --strict --no-interactive`（19/19）通过。

归档后全量严格校验 18/18，历史归档校验 989/989。
