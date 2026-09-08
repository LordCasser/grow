## Scope
使用 locked/offline、禁用增量和 debug 信息、2 jobs 构建 cli/grow。只检查工作树产物，不替换用户安装目录。version/help smoke 仅证明入口可运行，不代替模型会话端到端测试。

## Disk
构建前 target 9.8 GiB、可用 67 GiB；当前无清理必要，构建期间不运行 cargo clean。记录结束用量。
