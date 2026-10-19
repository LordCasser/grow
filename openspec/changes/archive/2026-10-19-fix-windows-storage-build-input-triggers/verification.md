# 验证记录

- `python -m unittest scripts.tests.test_release_regression_contract -v`：8 项通过。
- `python -m unittest discover -s scripts/tests -v`：18 项通过。
- `openspec validate --all --strict --no-interactive`：17 项通过，0 失败。
- 未运行 Cargo 或 hosted Windows workflow；本变更只调整触发路径并静态检查发布门禁。

## OpenSpec 归档核对

2026-10-19 已通过 `openspec archive <change> --yes` 归档；全部 tasks 完成。归档前全量严格校验 23/23；归档后 `openspec validate --all --strict --no-interactive` 16/16，`openspec validate --archived --strict --no-interactive` 634/634。F2 先于 F3 归档，核对主规范同时保留 cursor merge 与 child lifecycle/实际历史顺序场景。开发说明和 backlog 已同步，无 MCP 实现修改。
