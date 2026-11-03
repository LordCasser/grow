## Why
用户要求清理全部未明确暂缓的 backlog，并在完成后编译本地 release。已完成记录与未验证候选混排，需逐项以证据闭合。

## What Changes
逐项修复和验证非暂缓项；行为变更分别建立最小 change。本 change 仅汇总证据和精简 backlog，不改变产品契约，因此 skip_specs。明确暂缓项保留原因和启动条件。最后编译工作区 release，不发布、不替换全局安装。

## Impact
OpenSpec backlog 与本地构建产物。
