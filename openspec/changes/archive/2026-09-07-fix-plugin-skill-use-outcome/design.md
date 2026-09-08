## Decision
展开函数返回提示正文和与输入引用顺序一一对应的 loaded 布尔向量。只有成功生成正文块才为 true，目录项消失与读取错误为 false。admission 保留原有尝试/派发/激活归属事件，展开后逐项上报 PluginUsed.success；plugin.used span 同时记录 success。interjection 只消费正文，不新增 PluginUsed 事件。

这个内部结果对象避免第二次读取文件来猜测成功，也不引入诊断回调参数或全局状态。成功指正文展开成功，不代表模型最终完成任务。

## Verification
已有临时文件测试增加成功、缺失文件、消失目录项及空输入结果断言；运行完整 slash_commands 测试。代码核对 admission 逐项对应与 interjection 无额外事件。
