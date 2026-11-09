## 候选核验

- 基线为已发布的 v2.6.3，远端 main 与本地基线一致。v2.6.4 纳入事件驱动规划修复，代码和行为验证见相邻归档 deliver-child-planning-only-on-change。
- Cargo 1.93.1 的锁定 metadata 在 aarch64-apple-darwin 上解析成功，CLI 和 33 个继承 workspace 版本的本地包均为 2.6.4。
- 对最终提交执行注解 tag 静态预检和 OpenSpec 全量校验。官方发布流程负责同提交的回归、各平台构建和资产校验。
