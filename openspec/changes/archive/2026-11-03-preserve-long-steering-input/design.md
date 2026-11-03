## Evidence

Session `01a1a0fc-f83a-70e0-aefa-339ca4f054b4`，物理 journal seq 2920 的 Input::Submitted 引用 265278 字节 payload。seq 2932 的 Input::Consumed 中完整 permission_evidence 不代表模型收到全文：item.content 合计 25090 UTF-8 字节，以 `... [truncated]` 结尾，既没有尾部指导也没有全文引用。

`tools/src/interjection/format.rs::format_interjection` 是明确丢失点；Shell 的 explicit steer、自动 followup 和工具侧共享 buffer 都调用它。它把普通 prompt 的 25000 字节预算照搬为纯裁剪，没有普通 prompt 的持久 blob/read_file 路径。

## Design

纯 formatter 不承担有损输入预算：删除硬截断，原样包裹已由输入接纳边界接受的文本。已存在的请求构造、context pressure/compaction 和 provider admission 继续处理上下文容量，不能为满足局部字节数在消费时静默删正文。不新增上限、配置、临时重打包或第二份输入持久化。超出模型容量的请求仍遵守原有明确失败边界，本次不承诺任意长度输入都可由任意模型处理。

## Verification

先用新的 UTF-8 长文本 formatter 回归复现旧函数失败，再验证修复通过。Shell 回归经过真实 steer admission 和安全 drain，核对模型正文含完整中后部、Input::Consumed identity 不重复以及冷 Timeline 恢复保留全文；同时运行既有 interjection 与完成阻塞回归。

## Separate audit finding

普通 prompt 的 `run_turn_via_sampler` 在局部持有 PromptBlobExport，采样返回时其 TempDir 被删除，随后模型工具调用可能无法读取它收到的路径。现场长输入走 steer，未引用该路径，不能用此代码疑点替代现场归因。后续应在普通 prompt 的真实 sampler→read_file 链路复现，并按既有 artifact owner 确定文件寿命；本 change 不扩大到普通 prompt/export 架构。
