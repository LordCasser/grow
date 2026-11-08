# Uninstall application boundary

父核 plugin.rs::uninstall_plugin、hooks_plugins.rs::Uninstall、pager/plugin_cmd.rs::cmd_uninstall 和 git_install::remove_repo_path。低层 remove 对 symlink/directory 执行实际删除，对其它存在类型可能直接 Ok；application 不能因此在真实 path 仍存在时解除记录。配置数据清理是既有独立 best-effort 语义，本修复不扩张它。

顺序：InstallRegistry::try_load_from(resolve_install_dir) → find/多插件确认 → remove_repo_path → fallible 确认 path 已不存在 → 原 keep_data 分支 → remove registry entry → fallible save → outcome。已缺失 repo path 可按原卸载意图清理记录；存在的非目录异常 path 不自动删除未知文件，明确报错并保记录。load/confirm失败前无副作用。remove错误可能已部分删除目录，返回说明保留记录/可能部分文件，不能保证底层删除原子性。save失败说明原 repo key/path 已删除、原登记可能保留，不能声称卸载完成或伪回滚。

复用 UninstallError，增一项 operation failure（detail含实际repo/path/原因即可）；NotFound/NeedsConfirm保原输入/确认逻辑。actor只验证plugin_id非空、取最后name并调用共享uninstall(name,confirmed,keep_data=true)，将NotFound/NeedsConfirm/error分别映射原NotFound/ConfirmationRequired/InternalError，失败无reload/restart。CLI新增错误分支并沿公开run返回Err；不发成功diagnostic。

Coder只交这一项：plugin.rs Uninstall、actor Uninstall、CLI相应消费方和同一行为测试；必须等前一安装/更新owner释放相邻文件，保护dirty。Tier3本地隔离：坏registry无删除，真实删除失败/异常存在path保记录，save故障保真实已删artifact和旧字节且ACP/CLI错误；正常、多插件confirm和keep_data回归。可用既有save fault，不以chmod在当前权限下必失败作证。优先有效旧红/新绿。不得顺便改Update、Install、cleanup_plugin_data或低层通用remove所有调用语义。父审查关键失败分支和真实ACP/CLI入口，最终插件里程碑使用同次CLI；独立数据完整性review覆盖操作结果。父维护主spec/docs/backlog/archive。

## 实施时的调用方复核

除 ACP modal 和 CLI 外，`slash_exec.rs` 的 `/plugins uninstall` 也消费 `UninstallError`；新增 operation failure 必须沿其既有 slash-command error 事件返回，不能发 success diagnostic。真实 remove 错误以隔离临时路径末尾 `/.` 在 Unix 上触发 unlinkat EINVAL，不依赖 chmod 或是否以 root 运行；这条路径允许底层已部分删除，只要求保留登记并如实报错。
