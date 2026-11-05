## 1. Dependency and repair

- [x] 1.1 明确actor不拥有外部磁盘版本，single/batch共同owner，先修WS06b失败提交语义。
- [x] 1.2 WS06b冻结验收后，实施reject实际内容/存在性比较与stale错误，适配公共调用。
- [x] 1.3 真实Handle覆盖外部edit/create/delete及single/file/turn/all，正常对照和失败投影保持。

## 2. Acceptance

- [x] 2.1 独立review和06a/06b数据完整性里程碑通过，不声称非合作writer原子CAS。
- [x] 2.2 同步文档/验证/主规范，严格归档及backlog真实闭合。
