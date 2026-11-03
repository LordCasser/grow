# 验证

逐分支核对 dispatcher：CloseSession 重试优先；foreground turn/compact 使用取消；其他 Working 使用 stop_top_level_activity；NeedsInput 且无控制仍 busy；Idle 才关闭。导引与帮助文字没有新增控制动作。

新增真实 PTY dashboard_command_prefix_fork_cycle_and_close：先后打开 Model/Permission/Behavior 菜单，fork 无 worktree 副本，Ctrl+[ / Ctrl+] 循环，换页后再开 Permission，Ctrl+X 两次经 runtime close 返回 Dashboard；原页仍可 attach 且只剩一个本地页面。完整链 1 通过，9.67s。初次因复用旧 fixture 未 seed provider 而在欢迎前失败，补现有 seed_llm_config 与隔离项目后通过；没有改配置发现产品行为。日志 /tmp/grow-dashboard-journey-pty-configured.txt。

最终整体检查另附以下结果。

最终 binary 重跑组合 PTY 1 通过（15.87s），运行中退出与继续 PTY 1、stdio close/delete 各 1 通过。最终 Pager Dashboard 679（2 ignored）、fork 55、load 67、modal 17、delete 59、close 114、Ctrl+X 19、exit 44、quit 38 均通过；过滤有重叠，不汇总成唯一数。最终 build 使用 `--no-default-features --features sandbox-enforce` 通过，保留 sandbox；默认 jemalloc 本机 generated-header 构建问题未修改依赖规避。

最终 git diff --check 通过；未安装全局 binary、未提交 Git，保留工作区原有改动。

最终 OpenSpec 全量 strict 归档前 18、归档后 16 项通过；含归档 strict 778 项通过。清理了本轮失败 jemalloc 编译输出与自建共享验证临时 home，保留最终 binary、验证日志及其他工作区编译缓存。
