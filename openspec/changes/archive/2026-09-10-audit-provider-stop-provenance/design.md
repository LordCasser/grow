## Decisions

1. 原始输出只从 observation 引用的有序 `artifacts/sampling/<hash>.bin` 读取；核对总字节与各块记录长度，不以 message/Turn 投影替代。
2. 同时核对当前代码和安装版本对应 `155780e2` 的采集位置。区分原始 response body 与 evidence metadata；`stream_end` 的元数据不能冒充原始 SSE。
3. 不修改用户正在执行的第二个会话；只分析已落盘前缀。找不到匹配停顿时报告覆盖边界，向用户请求句子/时刻，不反过来否定其观察。
4. 整体任务过早结束仍为开放问题。工具配对和文本化已修复，不意味着“冒号停顿”整体关闭。Responses phase 的 portable 丢失属于后续候选，不先验解释所有端点。
