## 方案

沿用 workspace 根目录的统一版本号，以及 `crates/codegen/shell/changelogs/<version>.md` 发行说明。发布 tag 使用 `v2.2.2`，由现有 release workflow 校验并构建全部平台资产。

## 边界

本 change 不修改产品代码或已归档行为契约。发行说明只总结本次发布包含的已提交变更；跨平台构建和资产校验由官方 workflow 负责。
