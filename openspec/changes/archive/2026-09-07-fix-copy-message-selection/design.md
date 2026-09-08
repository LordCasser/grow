## Decision
使用计数和可选选中字符串，找到目标后立即退出遍历。不构建历史消息 Vec<String>，数量不足时遍历所得计数用于原错误反馈。0 在遍历前返回使用说明。

## Evidence
旧 dispatcher 的每个 AgentMessage 都调用 copy_text(false)，随后才取第 N 项。Action 是 usize；当前生产 slash 解析器已经拒绝 0，因此零索引属于内部边界加固，不宣称普通 /copy 0 现有可崩溃。

## Verification
真实 dispatcher 验证倒序 1/2/3、夹杂非 assistant 消息、0 和超出范围不写文件。性能改善以移除多余格式化的控制流为证据，不虚构基准数据。
