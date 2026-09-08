## 默认关闭路径核对

- LSP 工具：`Config::resolve_lsp_tools` 经 BoolFlag 在没有覆盖时为 false；`agent_ops.rs` 的会话构建实际读取开关并设置 LSP backend。Workspace 也有独立的配置加载和诊断进程路径，因此不能把“工具开关关闭”推导为“所有 LSP 进程都关闭”。本次沿配置链定位同名来源被不可信项目遮蔽的问题。
- web_fetch：`Config::resolve_web_fetch` 无覆盖时为 false，`prepare_web_fetch_config` 消费结果并形成 Disabled/启用配置；存在生产入口，不是删除候选。网络请求及内容转换边界尚待下一轮深入。
- ZDR access：全仓名称检索仅命中 remote 字段和孤立 resolver，未发现调用。已列为根目录临时删除候选 R3，等待确认；没有删除或补接这个开关。

Atlas 已 project(open)，作用域查询确认 LSP resolver；最终路径与调用点以当前源码及全仓名称检索核对，不从符号索引缺失单独推断死代码。此表只覆盖列出的开关，不是全仓未启用功能覆盖声明。
