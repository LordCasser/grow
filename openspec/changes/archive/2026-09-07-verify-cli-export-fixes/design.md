## Scope
使用 locked/offline、禁用增量和 debug 信息、2 jobs 构建 cli/grow。只操作工作树产物，不替换用户安装程序，不读取或导出真实会话，不访问剪贴板。

## Verification
入口 smoke 验证参数路由与启动；此前模块回归提供具体导出行为证据。不能用帮助命令成功代替真实会话端到端测试。记录 SHA-256 区分同一 Git 哈希下的未提交构建。
