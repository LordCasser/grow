## 1. Diagnose and Specify

- [x] 核对失败日志、当前 main、删除/清理调用链、真实 writer ownership 和相关规范。
- [x] 编写最小提案、设计和 writer 退役契约增量。

## 2. Implement and Regress

- [x] 用真实存储入口的确定性继承描述符探针证明旧实现失败。
- [x] 在原共享 owner 边界显式释放 writer 锁，增强原断言诊断，并加入正式 JSONL 回归。
- [x] 复验公共入口与正式测试的类型/Clippy 检查；记录完整单元测试编译的内存限制，不冒称全套通过。

## 3. Record Verification

- [x] 更新开发说明，执行严格规范/归档校验。
- [x] 留存已执行验证、明确限制和精确提交的后续 CI 验收要求。
