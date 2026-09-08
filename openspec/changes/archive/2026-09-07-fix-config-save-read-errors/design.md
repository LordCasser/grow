## Evidence
save_config_locked 的 read_to_string Err(_) 创建空 TOML，随后写临时文件并 rename 覆盖目标。非法 UTF-8 可稳定复现而无需 chmod/root 假设。语法错误已有显式拒绝分支，保持。update_config 的 load_from_disk 已将 NotFound 正常处理，却再次将所有 Err 降为默认值。

## Decision
只有 NotFound 可继续；其余 IO 错误通过 anyhow Result 返回。save_config_locked 委托内部 save_config_at(config,path)，锁仍由既有公开入口持有。测试直接调用内部路径入口，校验原字节保留及缺失文件可创建。update_config 使用 ? 防止失败读取后执行修改闭包。

## Limits
不处理跨进程写冲突、临时文件清理或配置环境变量持久化问题；这些必须另立变更。
