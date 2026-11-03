## Why

Dashboard 的 Working 投影包含后台 child、Goal 和 Workflow，但 Ctrl+X 停止只处理非 Idle 前台、后台命令、loop 和本地队列，用户无法将对应行停到可管理状态。

## What Changes

对选中 root 的真实活动来源派发既有控制：前台/child 使用 cancel，Goal 使用现有 pause 语义，Workflow 通过 host control 按 run id 停止；保留原后台任务与 loop 控制。不因发出请求就把界面伪造为空闲或允许删除。

## Capabilities

Modified: `client-surfaces`。

## Impact

Pager Dashboard stop dispatcher、相关按键到效果测试及开发说明；复用 ACP cancel、Goal pause 和 Workflow host command，不新增后端协议或持久状态。
