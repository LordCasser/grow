# Design

## Evidence and ownership

父已读只读调查并亲跑结构复核：failedrequest f5589fce-79a2-41e7-a4cc-bc4276e8d1df（timeline2137/frame748/offset308542）与cc7b9f26-2191-4c9a-9d28-d9d34937bc0c（frame512）均为added空output_text，part只有type/text；前一message item为in_progress，身份/indices匹配。成功sideband同阶段含annotations[]和logprobs[]，直到completed均有annotations。原失败未记录terminal，未知后续输出。原始证据和bounded复核保留于 `/tmp/grow-annotations-investigation-01a1ae61-3441-7c22-af81-f5101281e92e/`，不复制原prompt/正文。

SDK a2bae99 的ResponseContentPartAddedEvent.part是OutputContent；OutputTextContent.annotations为无default的Vec，logprobs为Option。这是typed decode失败，现client只回退usage细节与独立function-call初始arguments规范化。Sampler client持有原始SSE与解码副本，是本例外唯一所有者；L2不接收通用原始JSON补丁，sampling-types仍验证最终响应。

## Exact decode-copy exception

在现decode_response_event失败后的normalization链新增私有helper，仅满足以下全部条件才补 `part.annotations=[]`：顶层type为response.content_part.added；part为object；part.type为output_text；part.text显式为空字符串；annotations键不存在。保留原日志/evidence。已有空或非空数组直接由SDK解码，不重写；null/非数组、有非空初始text、其他part/event、done和terminal缺annotations不修。不得补item_id/indices/text或替换明确的错误。

Value重编码会吞重复key，已有arguments修复已证明该机制。此helper须保留SDK对该event/part已知字段的原有错误；可沿现局部Default+Deserialize/IgnoredAny唯一性检查，不用Option丢失null-first presence，不复制SDK字段类型/必填校验。event已知字段为type/sequence_number/item_id/output_index/content_index/part；OutputText part为type/annotations/logprobs/text。先用实际SDK控制证实风险，再按本helper必要边界处理，不能把检查扩大到其他规范化路径或创建通用parser。

后续合法delta、part.done、item.done、response.completed必须走现完整响应路径；EOF或原有identity/status/JSON错误不得产出Completed。无通用serialization重试，不把不完整流当成功。

## Alternatives and stopping point

全局SDK serde(default)会放宽完成项与其他事件，缺乏证据；吞掉整个content_part.added也会掩盖已知malformed字段。只按错误文字补字段不能保留尾部重复key。采用精确decode-copy例外，保留现校验，不预建其他缺省字段兼容。

本次在隔离worktree基于2.6.1最终源码实施，仅交付client补丁与此行为回归；父仍优先完成2.6.1全平台发布/资产验收。不得移动其tag、改2.6.1发行说明、推送或发布下一版本。后续版本编号未指定，不代为发布2.6.2。

## Review plan

父核原shape、精确gate与重复key错误保留、实际HTTP/SSE→client→L2的red/green，以及终态/EOF/error控制。保留旧生产路径的有效red，不将fixture编译错误当red。既有sampler库/集成、固定1.93.1生产check/fmt/diff应通过零warning；不重复无关全仓门禁。相关开发说明与验证放本change；特性验收与2.6.1发行验收分开记录，不新增独立审查。
