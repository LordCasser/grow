# Classify standard Messages stream errors

## Why

Backlog中独立的Anthropic `api_error`/`timeout_error`分类尚未闭合。已归档官方对照的固定文档与SDK列出二者：api_error是500内部服务错误，有指数退避建议；timeout_error表示处理超时，HTTP可504或500。当前共享 `SamplingError::from_stream_error`把二者都当未知500并加retry veto，timeout身份未进入已有504语义。该问题是本地分类缺口，不是已复现用户真实route故障。

## What changes

在现有sampling-types错误owner中识别两个标准精确code：api_error映射逻辑500，timeout_error映射逻辑504，均保留原type/message/requestID，不对已知服务错误附未知code veto。逻辑status用于Grow既有错误分类，不伪称流内错误携带了真实HTTP504。它们仅取得既有有限retry的候选资格，仍遵守attempt结算、不可逆输出/provider效果限制、共享次数/期限及caller veto；不新增重试循环。HTTP非2xx按真实status保持，未知code和普通畸形帧不放开。

## Impact

生产改动限定 `sampling-types/src/error.rs`；代表性测试可覆盖sampler实际Messages SSE→L2与既有retry/recovery。无新增公共类型、依赖或provider capability表。开发说明同步链接归档契约。实施/验证等待当前问答产品链的独占构建窗口释放，不能在其固定构建期间修改依赖源码。
