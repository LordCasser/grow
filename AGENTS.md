# 项目开发约束

遵循如非必要，勿增实体和第一性原理的理念，不考虑后向兼容，以长期架构优雅为主要考虑点。每一项的更改都需要确保完全理解架构思路以及周边代码后进行。简单需求必须保持最小闭环；审计发现的架构债务应记录并拆分处理，禁止混入当前改动导致范围失控。

## OpenSpec SDD

- 开发前先读 [OpenSpec 索引](openspec/README.md)、相关 `openspec/specs/*/spec.md` 和现有 `openspec/changes/`，再核对实现、调用方及测试。
- `openspec/specs/` 是已归档行为契约的唯一文档权威；代码是核对实际实现的证据。发现偏差必须明确记录，不能通过猜测、旧设计稿或删掉失败场景掩盖差异。
- 行为、接口、状态、安全或持久化变化：先建立 change，写 proposal、delta specs（含 WHEN/THEN）、design、tasks，再实现。用户已明确的范围不重复请求批准；有影响实现的歧义时先解决歧义。
- 纯文档、工具维护或不改变行为的纯重构也保留最小 change；`.openspec.yaml` 设置 `skip_specs: true`，写明不改契约的理由，禁止虚构 delta。小需求不扩成新框架。
- 完成相关验证后再勾选 tasks，运行 `openspec validate --all --strict --no-interactive`。归档前核对每项场景与验证记录；`openspec archive <change> --yes` 合入规范，再执行全量规范与 archive 校验。
- 新增提案、设计、任务、审计、验证记录只放 `openspec/changes/<change>/`；无关债务登记 [backlog](openspec/backlog.md)，不能顺带实施。
- `docs/` 保留开发者入口、架构解释、构建和调试说明；涉及行为时链接规范，不维护第二份需求或进行中的任务清单。既有历史文件只作追溯资料。
- 修改契约时在同一 change 更新相关开发者说明。不要把尚未完成的 delta 提前写入主规范。

命令和验证方法见 [开发指南](docs/development.md)。
