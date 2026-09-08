# Design
使用 RespawnError::Superseded 与 Failed(String) 表达现有两类事实，避免解析错误字符串。actor 已有两次 generation/配置比较在失配时返回 Superseded；恢复循环立即退出。其他启动/握手错误用 From<String> 保留 Failed。不添加新的配置身份计数器，不只保护工具删除而留下错误状态推送。

回归在第三次尝试模拟已识别的配置替换，断言仅前两次真实失败产生状态、无耗尽清理；正常三次失败测试仍必须通过。该回归证明循环处理，actor 的generation检查由源码核对。
