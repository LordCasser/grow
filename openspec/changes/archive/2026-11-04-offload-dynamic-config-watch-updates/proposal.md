## Why

v2.6.0 本机门禁复现 MCP 热更新和目录重建失败，native sample 显示动态 ConfigFileWatcher::apply_session_update 在 ACP LocalSet 同步进入 FSEvents watch/run。初次注册虽已移到 blocking runtime，后续注册、释放与 Drop 仍可能阻塞协议 executor。ProjectDiscoveryWatcher 的 native start 已移出 SessionActor LocalSet，但运行在每会话 current-thread runtime 的 blocking pool；OS start/Drop 延迟会拖住该 runtime 的退出及 session writer retirement。Config watcher 还对每项 .grow 目录通知无条件重挂，普通目录事件也会重启底层 watch stream。

## What Changes

- 由现有 blocking runtime 的单一 owner 顺序处理现有 session 注册队列，保留取消、最终 watcher Drop 和初始目录就绪边界；ProjectDiscoveryWatcher 的 blocking worker 归属调用方 host runtime，不归属会话 dedicated runtime。
- Pager 与 daemon 共用入口，ACP executor 不再执行动态 native watch/unwatch；初始化队列建立后才发送既有 all-session startup reload。
- 配置目录复用已存在的 same_file/attach_new_refresh_dirs 机制，仅在目录实体替换时重挂。

## Impact

配置监听执行位置和生命周期，不改 source ownership、监听范围、MCP 许可或配置格式。OS 原生注册仍不可取消，blocking runtime 的最终退出仍可能等待 OS 返回。
