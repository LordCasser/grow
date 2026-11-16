## Why

v2.6.15候选的两个Windows必需门禁发现Unix-only测试helper的cfg遗漏，发行被阻止。保留已推送tag不改写历史，按用户release最小版本的授权使用下一个未占用patch v2.6.16，正式上版仍是v2.6.14。

## What Changes

同步workspace/lock本地继承包为2.6.16，复用已验收恢复与gap功能的准确说明，明确2.6.15未发布。测试cfg单独在fix-history-index-test-cfg维护，不升级外部依赖。新候选完整执行官方十平台和全部门禁，验收公开资产后收拢文档。

## Scope

skip_specs：版本和发布维护不改变产品契约。正式发布前不以旧候选macOS通过替代新候选Windows门禁；不修改失败候选tag，不绕过门禁。
