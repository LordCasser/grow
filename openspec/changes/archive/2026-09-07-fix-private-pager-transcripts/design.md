## Design
复用 tempfile 依赖，以 Builder 前缀 grow-transcript- 和格式后缀创建 NamedTempFile，写完转 TempPath。Unix 从创建起为 0600；写入失败时 RAII 删除。AppView.pending_pager_path 从 PathBuf 改为 TempPath，事件循环借用路径调用子进程，超时把同一 owner 放回，其他分支释放 owner。无需额外清理服务。minimal seam 接收正文并调用共享函数。

## Scope
不迁移同步写入或渲染，不变更 $PAGER 解析。SIGKILL/崩溃不执行析构，文件系统拒绝删除时也不保证删除成功；这不是持久化导出，不增加 fsync。

## Validation
检查 Markdown/ANSI 内容与 Unix 权限、替换和 drop 清理；注入部分写入失败验证无残留；原有挂起重试测试和 minimal 编译/相关测试。
