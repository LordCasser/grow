## Why
图片URI生产端直接拼路径，消费端先percent-decode。字面%20/%2F文件名会变成空格/分隔符，已有图片去重可能定位错误文件。当前“兼容两种形式”的注释不能消除歧义。
## What Changes
图片URI统一使用url::Url的file-path生成和解析，百分号转义一次，不按文件是否存在猜测URI编码。更新实际发送、共享恢复和仍保留的旧builder生产点，保留原始placeholder路径文本语法。
## Impact
client-support加入已在workspace图中的url直接依赖；图片URI helper和调用点。非图片file://用途不混入。无需历史歧义URI兼容层。
