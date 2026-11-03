# 设计

提取主图片恢复已有的描述生成阶段，输入为冻结 items、原 Surface IDs、revision 和选中的图片组，输出带来源的 ImageShadow。主路径继续负责 ImageProjection 和 UnsupportedModel 处置；辅助路径只应用成功描述到请求副本，缺失描述直接失败，不能移除图片后伪装为成功。

已知拒图路由在请求准入前恢复最终选中窗口；未知路由保持首次乐观。精确拒图后持久记录同一准备配置的能力，结清原失败 attempt，按原来源、revision 和 assembly strategy 记录唯一一次恢复 attempt。每个 attempt 继续服从冻结预算：btw/询问使用准备模型窗口，recap 复用既有 recap_input_budget，flush 复用既有 MAX_FLUSH_INPUT_TOKENS。提前准入两种表示形式共用的真实输入上限，不把首个图片估算值当成文字表示上限；若文字展开仍超过该预算，明确失败，不能暗中扩大已准入上限。描述缓存和 durable 结果复用原坐标，主历史并发前进不改变本次来源。

Sideband 取消覆盖等待、描述、OCR 与重发。主图片写锁串行化描述缓存工作，但辅助描述不写 ImageProjection；provider 回答不持锁。涉及预算之外的图片不生成描述。

内存描述缓存允许复用较早 revision 的同一不可变 source/content/prompt，始终拒绝未来 revision；与已有 durable 查询规则一致。
