# 发布状态回溯核对

2026-11-17收尾检查发现tasks.md仍保留两项未勾选，但发行已存在。GitHub权威数据确认：

- 正式非draft、非prerelease [v2.6.5](https://github.com/LordCasser/grow/releases/tag/v2.6.5) 已于2026-11-10T04:04:53Z公开发布。
- `refs/tags/v2.6.5` 为远端annotated tag，tag对象`dd614cdd8beeb187faf71ca782467a9acfda2e97`。
- 公开资产恰有linux五种、macOS两种、Windows两种、OHOS一种归档及SHA256SUMS，总计11项。

本次仅将已完成发布的任务状态与文档收拢并归档，不重新发布2.6.5或修改旧tag。原本地预检的逐次日志没有在本change保留；不把这次远端核对伪称为重跑旧候选。版本与发布内容以远端既有事实为准。
