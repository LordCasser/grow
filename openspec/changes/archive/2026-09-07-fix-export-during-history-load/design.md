## Decision
复用 session.loading_replay 这一已有历史加载窗口，在渲染和任何目标写入之前检查。不要使用 prompt_history_loading（输入历史列表，不是 transcript），也不新增导出状态机。

## Evidence
ExportConversation router 直接进入 dispatch_export_conversation，原 dispatcher 无加载检查；session/load 初始化与成功/失败收尾维护 loading_replay。一般加载期间 scrollback 接收回放，reload staging 时也可能尚未换入新历史，因此加载窗口内不承诺完整导出。

## Verification
真实 dispatcher 使用临时文件，加载期间已有首条消息时不生成文件；标记回放完成并加入剩余消息后导出包含全部内容。剪贴板分支位于同一前置检查之后，静态核对，不访问真实剪贴板。
