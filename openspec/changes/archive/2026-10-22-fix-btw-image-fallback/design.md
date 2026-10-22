# 设计

沿用主采样的共享 ImageProjection：它是已确认纯文本 transport 的可复用媒体事实，不是 /btw 对话写入。描述保留原图，无法恢复时沿用现有 typed removal；不增加独立转换或存储系统。

固定 prepared config 的 ModelImageInputKey/client/model，不能从切换后的当前模型推导拒绝身份。每个冻结上下文拥有独立 Sideband；失败 Sideband 终结后才恢复图片并建立新冻结源。超载仍沿用原最多三次尝试；图片恢复最多一次，避免无限循环。已知纯文本路由发送前先转换。选择描述在 portable projection 前完成，原始媒体仍由 Timeline 保留。转换失败不得发送未确认的文字请求，普通 400 不标记能力。

共享图片投影可与主 turn 并发，复用已有 exact-revision ACK 和 SurfaceChanged 有界重试，不暂停主 turn，也不把 side question 写入主会话。恢复后的请求基于新的明确冻结源，不冒用旧源。

拒绝后的恢复绑定失败请求的 Surface revision；若主 turn 已修改 Surface，直接返回 SurfaceChanged，不转换新来源。投影执行期间的 revision 变化由既有持久 ACK 校验拒绝。超载 backoff 在图片转换前后共享，两阶段合计最多四次请求（一次明确能力拒绝、最多两次超载重试和最终请求），每个 Sideband 仍最多三次。
