# Why
阻塞型 HTTP Hook 使用 response.text 完整缓存响应，超时前仍可能积累大量数据。响应参与 deny/allow/stop 决策，不能以截断的内容继续解析并意外允许操作。

# What Changes
流式读取响应并在追加前强制 64 KiB 上限，超限作为 Failed 走现有失败策略，不保存部分决策预览。

# Impact
Observe 模式仍只看状态码，不读正文；合法响应保留 UTF-8 lossy 解码，网络超时仍为 TimedOut。
