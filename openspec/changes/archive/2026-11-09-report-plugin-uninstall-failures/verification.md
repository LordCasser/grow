# AC-08 uninstall persistence verification

日期：2026-11-08。实现提交 `bac1f546`，与集成源码 `ed2027c0` 的相关生产/测试文件一致。Linux、Rust 1.93.1，隔离 Grow home；无在线服务依赖。

## 运行证据

- Shell `test_plugin_install_persistence`：**5/5**，包含既有 Install 两项和新增共享卸载、真实 remove 错误、实际 ACP 卸载三项。退出 0，0.83s。
- Shell `test_acp_plugin_update_results`：**2/2**，用于同批插件持久化和混合 marketplace 回归。退出 0，0.81s。
- Pager `plugin_cmd::`：**10/10**。新 public `run(PluginArgs)` 测试以独立进程验证卸载/更新 save、parse、read、success、unexpected path/no-op，错误不能打印成功文案。退出 0，1.06s。
- `plugin-marketplace` 的 `transactional_update`：**5/5**，既有事务保存失败回滚行为保持。
- Shell 全 lib/tests Clippy 编译检查退出 0，保留既有 warnings。

共享 application 与真实 ACP extension→session handle→actor 均覆盖：

1. JSON 破坏或 registry 路径成为目录时，在删除前失败，原 artifact、错误证据与 plugin data 不变。
2. 非预期普通文件不被自动删除，存在检查失败保留登记。
3. Unix 临时 repo 路径末尾 `/.` 触发真实 unlinkat EINVAL，不依赖 chmod 或 root 权限；底层可已部分删除，但 application 返回错误并保留原记录。
4. 多插件未确认时不删除；确认后沿原范围处理全部 repo 插件。
5. repo path 已删、save fault 阻止注销时，错误含真实 repo/path 与不完整事实，registry 原字节不变；ACP 不要求 reload/restart。
6. 明确重试已缺失 path 可完成注销；ACP 固定 keep-data，CLI/shared keep-data 与清理分支保原政策。

## 有效旧红/新绿与审查

[同批差分程序和完整记录](../2026-11-09-report-plugin-update-persistence-failures/verification.md#窄范围有效旧红新绿)以原始基线 `plugin.rs` 和当前生产 module 对照。旧 save-failure 场景在 artifact 已删除、登记仍旧时返回 Ok；旧 unexpected-file 场景在文件仍在时删除登记。两项均触发预期断言失败，修复后相同断言通过。第三项覆盖 ordinary update。该窄范围控制不宣称完整旧 ACP/CLI 二进制重建。

独立数据完整性审查核对删除/不存在确认/保存的顺序、部分完成文案和全部错误消费方；新增 operation failure 在旧 `/plugins uninstall` 路径亦进入 slash-command error，不发送成功 diagnostic。该分支由类型检查覆盖，未独立运行 slash UI 场景。

完整 Shell lib-test 曾被外部终止 137，未运行的阶段不计通过；最终 focused runtime 使用单 job/lld 和命令行 CGU512，未改 CI/profile。详细命令与平台范围见上面的同批记录。

## 最终产品链

同次集成 `ed2027c0` / tree `bf0f734a` 的独立生产 `grow` 构建退出 0；同批 `cli-smoke.py` 的 **11 次 CLI 调用全部通过**。卸载覆盖 save-failure exit 1、移除故障后 retry exit 0、异常文件保留、坏 registry 早失败、多插件 confirm 门禁与正常成功；失败 stdout 无成功文案，磁盘与登记事实逐项核对。更新的完整对照亦在同一 binary 执行，见同批验证。远端完整 CI 状态由整合任务另行确认。


## 规范与归档

OpenSpec 1.11.0：归档前 all/strict 21 项通过；两个契约正常归入 `configuration-rules`，工具生成的归档目录日期为 2026-11-09。开发说明和 backlog 已同步。最终 all/strict **19/19**、archived **925/925** 通过，`git diff --check` 通过。首次归档复核只提示最后的归档复核任务尚未勾选；完成本步后重新运行，两项任务清单均 5/5。此规范验收不替代远端完整 CI。
