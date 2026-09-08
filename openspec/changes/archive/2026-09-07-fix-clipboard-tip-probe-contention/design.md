## Design
两个 UI 元数据入口 clipboard_change_count / clipboard_image_snapshot 使用现有锁的 try_lock；失败分别返回 None / (None,false)。后台 native_image_read 继续阻塞持有原锁，不能并发发送 NSPasteboard 消息。

poll 的无图片结果只有 outcome.change_count 存在才更新 last_seen，且记录分类自身的版本，而不是早先 cheap 的版本。不可用分类保留最后成功处理版本，下一次节流允许的机会重试。图片成功显示才提交去重的现有规则保持。

## Boundaries
不宣称原生调用或首次 AppKit OnceLock/dlopen 有超时；本次只去除等待后台读取所持锁的 UI 竞争。跨进程复制期间的一致快照政策与原生 API 总超时另行审计。无新增 worker、锁或配置。

## Validation
纯状态测试：cheap 有版本而 classify 不可用，下一次相同版本仍分类；cheap 与非图片分类版本变化时按后者去重。macOS 测试主动持有 PASTEBOARD_LOCK 再调用两个元数据入口，验证立即返回未知且不接触真实剪贴板。运行相关 pager 与 client-support 测试。
