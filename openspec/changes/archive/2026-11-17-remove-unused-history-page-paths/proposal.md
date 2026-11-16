## Why

v2.6.16官方Windows分发构建在-D warnings下失败：TranscriptProjection.finish_display_page与ScrollbackState.prepend_entries_from已无生产调用。前者完全无调用，后者只被本轮gap测试的旧路径分支调用。主历史窗口已使用apply_history_page与replace_history_prefix，保留旧草稿接口没有运行时职责。

## What Changes

删除两个旧入口；gap前插回归保留真实replace_history_prefix路径的全部身份、viewport和行位置断言。明确正式release-dist/deny-warnings本地检查，禁止用allow(dead_code)或修改发布门禁掩盖残留。

## Scope

skip_specs：纯删除不可达代码及对应过时测试分支，不改变产品行为、历史安装契约、数据或持久化。两个模块分别交付，原连续历史与gap要求继续由真实路径验收。
