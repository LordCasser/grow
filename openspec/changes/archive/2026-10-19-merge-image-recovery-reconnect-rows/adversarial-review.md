# 对抗性审查

检查 cursor reload 前 Started、reload 中 Completed、tail 合并、完成后的迟到 progress，以及已终态旧行与 running tail 的反向交错。合并以 operation identity 更新原 EntryId，维护 running index 与 native committed 集合；不重开终态，不追加副本。root/child 各自 scrollback 的生命周期仍隔离。

交叉审查未发现生产路径的 native commit 缺陷，但指出原测试没有直接覆盖 committed 状态；收尾回归补上该断言。相关运行结果见 verification.md。
