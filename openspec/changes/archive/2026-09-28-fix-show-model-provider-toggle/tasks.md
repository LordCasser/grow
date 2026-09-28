## 1. 修复与回归

- [x] 1.1 在 Settings Bool action 映射补齐 `show_model_provider`，用现有映射完整性测试确认该键返回 typed Action。
- [x] 1.2 在弹窗输入测试覆盖此行 Enter 开启、Space 关闭；运行两项定点测试确认 UI 入口可达。

## 2. 验证与归档

- [x] 2.1 更新开发者说明中的 Settings 操作方式，运行相关 Pager 定点测试、格式检查与 `git diff --check`，将实际结果写入 `verification.md`。
- [x] 2.2 核对全部 delta 场景，运行 `openspec validate --all --strict --no-interactive`；归档后再次运行全量规范和 archived 校验。
