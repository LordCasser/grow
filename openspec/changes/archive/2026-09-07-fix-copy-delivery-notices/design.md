## Decision
新增 CopyDelivery::summary_message：Clipboard 有备份时使用 result.message_lead 和备份路径，无备份使用 result.message；文件回退/失败沿用 toast_message。该方法用于保留在 scrollback 的通知，所以与短 toast 不同，确认成功时也保留备份位置。

/copy 直接使用摘要加统计；/export 添加 Conversation 前缀。这样 tmux 目标也保留后端原有语义，不被统一改称系统剪贴板。

## Verification
纯反馈测试通过现有 ClipboardFeedback 构造 Confirmed/Unverified，覆盖有无备份；另覆盖 File/Failed。核对两个 dispatcher 都使用共享摘要，不触发真实剪贴板。
