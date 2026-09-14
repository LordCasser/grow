## 1. 收敛依赖
- [x] 1.1 迁移共享工具值类型，验证 sampling-types 不再依赖 tools，serde 与采样测试通过。
- [x] 1.2 迁移客户端身份值类型，验证 grow-http 不再依赖 workspace/sampler，UA 与 metadata 测试通过。
- [x] 1.3 Pager 传入权限特殊行语义，验证 renderer 不再依赖 workspace，cursor 测试通过。
- [x] 1.4 更新模块使用配置所有者，验证 update 不再依赖 shell，配置与更新策略测试通过。

## 2. 集成验证
- [x] 2.1 串行运行受影响 crate 测试及 CLI 构建，记录磁盘和依赖对照。
- [x] 2.2 更新开发说明、审计债务，完成严格规范校验与归档。

