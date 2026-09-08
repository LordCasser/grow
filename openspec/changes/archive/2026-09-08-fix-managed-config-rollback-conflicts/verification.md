## Evidence
原实现回归失败：新建文件被外部 write 后，rollback 将其删除，读取目标得到 NotFound。

最终 `cargo test --locked --offline -p config --lib managed_text --quiet`：26 passed，2.72s。冲突测试含2种初始状态 ×3个注入时点 ×6种编辑方式，共36组；已有文件均预先占用 backup hint，验证保留实际重试路径、原内容与其他事务的文件。覆盖原地写入、替换、同字节替换、删除、移走原文件后以符号链接指向同一 inode、权限变化。正常 post-publish 故障回滚及原超时/清理测试通过。

旧 CorruptTemp 测试不再假定所有损坏都属于本事务；现在要求保留未知来源字节及原始备份。Recovery 增加 backup_path 的消费者搜索只发现本模块构造与宽匹配，doctor 错误格式化无需专用分支。未修改真实用户配置，未做 Windows 实机验证。

全量严格校验16项通过；归档后再校验。target 152 MiB，可用76 GiB，保留当前小缓存供相关验证。
